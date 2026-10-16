//! Syntax highlighting for the fenced code blocks of a `TextView`, kept per
//! block and brought up to date incrementally while a block streams in.
//!
//! A streamed code block reaches the highlighter as a new block for every
//! chunk, each one the previous text plus the chunk. Highlighting each from
//! scratch costs time in proportion to the whole block, so a long block
//! streamed in small chunks costs time in proportion to its length squared,
//! all of it on the UI thread. Here a block's parse, injected layers and
//! highlights are kept, found again by the text they were made for, and a
//! block that extends a kept one only pays for what changed:
//!
//! 1. The tree takes an append edit and reparses incrementally, reusing
//!    every node the new text left alone.
//! 2. Highlighting starts again from the first byte whose highlights can
//!    differ: the earlier of the line the appended text continues and the
//!    first node the reparse changed (see [`first_change`]).
//! 3. Injected layers ending before that point are kept as they are. Only
//!    the injections found from it on are parsed again, from their old trees.
//! 4. The highlight query runs over that stretch alone, and its highlights
//!    replace the kept ones from that point on.
//!
//! Highlights are kept as the highlight names that cover each stretch of
//! code, not as colors. They are turned into the colors of a theme when
//! they are asked for, so a theme change is a lookup per stretch, not a
//! parse.

use std::{cell::RefCell, collections::HashMap, ops::Range, sync::Arc};

use gpui::{HighlightStyle, SharedString};
use gpui_base::{
    input::{HighlightStyleResolver, RopeExt as _},
    text::CodeBlockHighlights,
};
use ropey::Rope;
use smallvec::SmallVec;
use sum_tree::Bias;
use tree_sitter::{InputEdit, Parser, Point, QueryCursor, StreamingIterator, Tree};

use super::{
    LanguageRegistry, SyntaxHighlighter,
    compiled::{
        CompiledLanguage, HighlightName, HighlightQuery, Readiness, Waiter, compiled_language,
        compiled_language_now, layer_query, layer_query_now, ready_generation,
    },
    highlighter::{
        InjectionLayer, MAX_INJECTION_BYTES, MAX_INJECTION_RANGES,
        MAX_NON_COMBINED_INJECTION_PARSES, TextProvider, bounding_byte_range,
        captured_injection_language, edit_range, injection_range_len,
        injection_ranges_within_limits, merge_highlight_style, normalize_combined_injection_ranges,
        parse_input_bytes, should_include_injection_range,
    },
};

/// The most code blocks whose highlighting is kept, per thread.
const MAX_KEPT_BLOCKS: usize = 64;
/// The most source bytes of kept code blocks, per thread. Past it the least
/// recently highlighted blocks are dropped, down to the block in use.
const MAX_KEPT_BYTES: usize = 8 * 1024 * 1024;
/// How far before the appended text highlighting starts again at least: the
/// start of the line it continues, but never more than this many bytes back.
/// [`first_change`] finds what the reparse changed, and this keeps a margin
/// around the edit.
const LINE_MARGIN: usize = 256;

/// The highlight names covering a stretch of code, in the order they are
/// layered: where they disagree, a later one wins.
type NameStack = SmallVec<[HighlightName; 2]>;

/// A stretch of code and the [`NameStack`] covering it, as an index into
/// [`CodeBlockHighlighter::stacks`]. Stack 0 is empty: no highlight.
#[derive(Debug, Clone, PartialEq)]
struct Segment {
    range: Range<usize>,
    stack: u32,
}

/// The colors of [`CodeBlockHighlighter::segments`] in one theme.
struct Resolved {
    theme: Arc<super::HighlightTheme>,
    /// The style of each name stack, resolved when first needed.
    stack_styles: Vec<Option<HighlightStyle>>,
    /// The styles of the first `styles.len()` segments, shared with the
    /// code blocks they were handed to.
    styles: Arc<Vec<(Range<usize>, HighlightStyle)>>,
}

/// The highlighting of one code block, kept to be extended.
pub(crate) struct CodeBlockHighlighter {
    language: Arc<CompiledLanguage>,
    parser: Parser,
    /// The code the highlighting is for.
    source: SharedString,
    text: Rope,
    tree: Option<Tree>,
    /// Injected layers, by start.
    layers: Vec<InjectionLayer>,
    /// Where each parse of a non-combined injection starts and ends, in
    /// document order, to stop at the same parse budget as a highlight from
    /// scratch.
    attempts: Vec<(usize, usize)>,
    /// The ranges collected for each combined injection language, before the
    /// separators of [`normalize_combined_injection_ranges`].
    combined: Vec<(SharedString, Arc<HighlightQuery>, Vec<tree_sitter::Range>)>,
    segments: Vec<Segment>,
    stacks: Vec<NameStack>,
    stack_ids: HashMap<NameStack, u32>,
    resolved: Option<Resolved>,
    /// Compiles the languages this block waits for in the background, and
    /// tells the application when they are ready. Without one, languages are
    /// compiled on the calling thread.
    waiter: Option<Waiter>,
    /// Whether an injected language was not ready when the layers were last
    /// found, as of [`ready_generation`] then.
    layers_pending: Option<u64>,
    #[cfg(test)]
    parses: usize,
}

impl CodeBlockHighlighter {
    /// A highlighter of `language` for `code`.
    fn new(
        language: Arc<CompiledLanguage>,
        code: &SharedString,
        waiter: Option<&Waiter>,
    ) -> Option<Self> {
        let (mut parser, grammar) = LanguageRegistry::singleton().parser(&language.name).ok()?;
        parser.set_language(&grammar).ok()?;
        let empty = NameStack::new();
        let mut this = Self {
            language,
            parser,
            source: SharedString::default(),
            text: Rope::new(),
            tree: None,
            layers: Vec::new(),
            attempts: Vec::new(),
            combined: Vec::new(),
            segments: Vec::new(),
            stacks: vec![empty.clone()],
            stack_ids: HashMap::from_iter([(empty, 0)]),
            resolved: None,
            waiter: waiter.cloned(),
            layers_pending: None,
            #[cfg(test)]
            parses: 0,
        };
        this.reset(code);
        Some(this)
    }

    fn len(&self) -> usize {
        self.text.len()
    }

    /// Brings the highlighting up to date with `code`.
    fn update(&mut self, code: &SharedString) {
        // An injected language that was not ready may be now.
        if self
            .layers_pending
            .is_some_and(|pending_at| pending_at != ready_generation())
        {
            self.reset(code);
            return;
        }
        if *code == self.source {
            self.source = code.clone();
            return;
        }
        if self.tree.is_some() && code.starts_with(self.source.as_ref()) {
            self.append(code);
        } else {
            self.reset(code);
        }
    }

    /// Highlights `code` from scratch.
    fn reset(&mut self, code: &SharedString) {
        self.source = code.clone();
        self.text = Rope::from_str(code);
        self.layers_pending = None;
        self.layers.clear();
        self.attempts.clear();
        self.combined.clear();
        self.tree = self.parse(None);
        let start = self.update_layers(0, None);
        debug_assert_eq!(start, 0);
        self.highlight_from(0);
    }

    /// Highlights `code`, which extends the current source.
    fn append(&mut self, code: &SharedString) {
        let old_len = self.len();
        let start_position = ts_point(self.text.offset_to_point(old_len));
        self.text.insert(old_len, &code[old_len..]);
        self.source = code.clone();
        let new_len = self.len();
        let edit = InputEdit {
            start_byte: old_len,
            old_end_byte: old_len,
            new_end_byte: new_len,
            start_position,
            old_end_position: start_position,
            new_end_position: ts_point(self.text.offset_to_point(new_len)),
        };

        let mut start = old_len
            .saturating_sub(start_position.column)
            .max(old_len.saturating_sub(LINE_MARGIN));
        let mut old_tree = self.tree.take();
        if let Some(tree) = old_tree.as_mut() {
            tree.edit(&edit);
        }
        self.tree = self.parse(old_tree.as_ref());
        match (&old_tree, &self.tree) {
            (Some(old_tree), Some(tree)) => {
                let query = &self.language.highlights_source;
                let named = |kind: &str| query.contains(&format!("({kind}"));
                start = start.min(first_change(old_tree, tree, named));
            }
            _ => start = 0,
        }
        let start = self.update_layers(start, Some(&edit));
        self.highlight_from(start);
    }

    fn parse(&mut self, old_tree: Option<&Tree>) -> Option<Tree> {
        #[cfg(test)]
        {
            self.parses += 1;
        }
        let text = &self.text;
        self.parser.parse_with_options(
            &mut |offset, _| parse_input_bytes(text, offset),
            old_tree,
            None,
        )
    }

    /// Brings the injected layers up to date with the tree, which is the
    /// same as before up to `dirty`, and returns where highlighting has to
    /// start again: `dirty`, or the start of a layer whose highlights can
    /// have changed before it. `edit` is the edit the tree took, if any.
    ///
    /// Mirrors [`SyntaxHighlighter::compute_injection_layers`], which does
    /// the same for a whole tree.
    fn update_layers(&mut self, dirty: usize, edit: Option<&InputEdit>) -> usize {
        let Some(injections) = self.language.injections.clone() else {
            return dirty;
        };
        let Some(tree) = self.tree.clone() else {
            self.layers.clear();
            return dirty;
        };

        // Injections are found again from `from`: `dirty`, moved back to the
        // start of every layer and combined range that reaches past it, as
        // its node may have grown or shrunk. Nothing before `dirty` changed,
        // so a layer that ends by `from` is what a highlight from scratch
        // would build again, unless its node now goes on past `from`: the
        // query finds that node again, and the layer it builds supersedes
        // the kept one (same language and start). These use the old offsets:
        // the edit stretches a range that ends where the appended text
        // starts, whether or not its node goes on.
        let mut from = dirty;
        loop {
            let layer_starts = self
                .layers
                .iter()
                .filter(|layer| !layer.combined && layer.byte_range.end > from)
                .map(|layer| layer.byte_range.start);
            let range_starts = self
                .combined
                .iter()
                .flat_map(|(_, _, ranges)| ranges)
                .filter(|range| range.end_byte > from)
                .map(|range| range.start_byte);
            let next = layer_starts.chain(range_starts).fold(from, usize::min);
            if next == from {
                break;
            }
            from = next;
        }
        let (mut kept, mut replaced): (Vec<_>, Vec<_>) = std::mem::take(&mut self.layers)
            .into_iter()
            .partition(|layer| !layer.combined && layer.byte_range.end <= from);
        // The replaced layers move to the edited text, so their trees can be
        // reparsed from and compared with the new ones.
        if let Some(edit) = edit {
            for layer in &mut replaced {
                edit_layer(layer, edit);
            }
        }
        self.attempts.retain(|(_, end)| *end <= from);
        for (_, _, ranges) in &mut self.combined {
            ranges.retain(|range| range.end_byte <= from);
        }

        let ready_at = ready_generation();
        let waiter = self.waiter.clone();
        let mut layers_pending = false;
        let text = &self.text;
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(from..text.len());
        let mut matches = cursor.matches(&injections, tree.root_node(), TextProvider(text));
        let mut resolved_languages: HashMap<
            SharedString,
            Option<(SharedString, Arc<HighlightQuery>)>,
        > = HashMap::new();
        let mut new_layers = Vec::new();
        while let Some(query_match) = StreamingIterator::next(&mut matches) {
            let mut language_name: Option<SharedString> = None;
            let mut combined = false;
            for prop in injections.property_settings(query_match.pattern_index) {
                match prop.key.as_ref() {
                    "injection.language" => {
                        language_name = prop
                            .value
                            .as_ref()
                            .map(|v| SharedString::from(v.to_string()));
                    }
                    "injection.combined" => combined = true,
                    _ => {}
                }
            }

            let mut ranges = query_match
                .captures
                .iter()
                .filter(|cap| Some(cap.index) == self.language.injection_content_capture_index)
                .map(|capture| capture.node.range())
                .collect::<Vec<_>>();
            // A match that ends by `from` was handled before: its layer is
            // kept, or it was skipped the same way.
            if ranges.is_empty() || ranges.iter().all(|range| range.end_byte <= from) {
                continue;
            }
            let first = ranges
                .iter()
                .map(|range| range.start_byte)
                .min()
                .unwrap_or_default();
            if !combined {
                // A parse attempt of this node's earlier extent is replaced.
                self.attempts.retain(|(start, _)| *start != first);
                if self.attempts.len() >= MAX_NON_COMBINED_INJECTION_PARSES {
                    continue;
                }
            }

            if language_name.is_none() {
                language_name = query_match
                    .captures
                    .iter()
                    .find(|cap| Some(cap.index) == self.language.injection_language_capture_index)
                    .and_then(|capture| {
                        captured_injection_language(text, capture.node.byte_range())
                    });
            }
            let Some(raw_language_name) = language_name else {
                continue;
            };
            let Some((language_name, highlight_query)) = resolved_languages
                .entry(raw_language_name.clone())
                .or_insert_with(|| match &waiter {
                    None => layer_query(&raw_language_name),
                    Some(waiter) => match layer_query_now(&raw_language_name, waiter) {
                        Readiness::Ready(query) => query,
                        // Highlighted without the layer until it is ready.
                        Readiness::Pending => {
                            layers_pending = true;
                            None
                        }
                    },
                })
                .clone()
            else {
                continue;
            };

            ranges.retain(|range| should_include_injection_range(&language_name, range, text));
            if ranges.is_empty() {
                continue;
            }
            ranges.sort_unstable_by_key(|range| (range.start_byte, range.end_byte));

            if combined {
                let collected = match self
                    .combined
                    .iter_mut()
                    .position(|(name, _, _)| *name == language_name)
                {
                    Some(ix) => &mut self.combined[ix].2,
                    None => {
                        self.combined
                            .push((language_name, highlight_query, Vec::new()));
                        &mut self.combined.last_mut().unwrap().2
                    }
                };
                for range in ranges.into_iter().filter(|range| range.end_byte > from) {
                    // The node's earlier extent, kept, is replaced.
                    collected.retain(|kept| kept.start_byte != range.start_byte);
                    collected.push(range);
                }
                continue;
            }

            if !injection_ranges_within_limits(&ranges) {
                continue;
            }
            self.attempts.push((
                first,
                ranges.iter().map(|range| range.end_byte).max().unwrap_or(0),
            ));
            let same_node = |layer: &InjectionLayer| {
                !layer.combined
                    && layer.language_name == language_name
                    && layer.ranges.first().map(|range| range.start_byte) == Some(first)
            };
            if let Some(ix) = kept.iter().position(same_node) {
                let mut superseded = kept.remove(ix);
                if let Some(edit) = edit {
                    edit_layer(&mut superseded, edit);
                }
                replaced.push(superseded);
            }
            let old_tree = replaced
                .iter()
                .find(|layer| same_node(layer))
                .map(|layer| &layer.tree);
            if let Some(layer) = SyntaxHighlighter::parse_injection_layer(
                &language_name,
                highlight_query,
                ranges,
                old_tree,
                false,
                text,
            ) {
                new_layers.push(layer);
            }
        }

        for (language_name, highlight_query, collected) in &mut self.combined {
            collected.sort_unstable_by_key(|range| (range.start_byte, range.end_byte));
            limit_combined_ranges(collected);
            let ranges = normalize_combined_injection_ranges(language_name, collected.clone());
            if ranges.is_empty() {
                continue;
            }
            // Parsed from scratch: reparsing a combined layer from its old
            // tree, over included ranges that changed, can recover from the
            // errors of unfinished code differently than a parse from
            // scratch, and the highlights then depend on how the code
            // arrived. Its old tree still bounds what changed, below.
            if let Some(layer) = SyntaxHighlighter::parse_injection_layer(
                language_name,
                highlight_query.clone(),
                ranges,
                None,
                true,
                text,
            ) {
                new_layers.push(layer);
            }
        }
        self.combined.retain(|(_, _, ranges)| !ranges.is_empty());

        // Where the new layers can highlight differently from the old ones.
        let mut start = dirty;
        for layer in &new_layers {
            let old = replaced.iter().position(|old| {
                old.combined == layer.combined
                    && old.language_name == layer.language_name
                    && (layer.combined
                        || old.ranges.first().map(|range| range.start_byte)
                            == layer.ranges.first().map(|range| range.start_byte))
            });
            let Some(old) = old.map(|ix| replaced.swap_remove(ix)) else {
                start = start.min(layer.byte_range.start);
                continue;
            };
            let common = old.ranges.len().min(layer.ranges.len());
            if let Some(ix) = (0..common).find(|ix| old.ranges[*ix] != layer.ranges[*ix]) {
                start = start.min(old.ranges[ix].start_byte.min(layer.ranges[ix].start_byte));
            } else if let Some(range) = old.ranges.get(common).or(layer.ranges.get(common)) {
                start = start.min(range.start_byte);
            }
            for range in old.tree.changed_ranges(&layer.tree) {
                start = start.min(range.start_byte);
            }
        }
        // A layer that is gone leaves its highlights behind otherwise.
        for old in &replaced {
            start = start.min(old.byte_range.start);
        }

        self.layers = kept;
        self.layers.extend(new_layers);
        self.layers.sort_by_key(|layer| layer.byte_range.start);
        if layers_pending {
            self.layers_pending.get_or_insert(ready_at);
        }
        start
    }

    /// Highlights the code from `start` to its end again, keeping the
    /// highlights before it.
    fn highlight_from(&mut self, start: usize) {
        let range = start..self.len();
        let mut captures = Vec::new();
        self.host_captures(range.clone(), &mut captures);
        for layer in &self.layers {
            if layer.byte_range.end <= range.start {
                continue;
            }
            if layer.byte_range.start >= range.end {
                break;
            }
            layer_captures(layer, range.clone(), &self.text, &mut captures);
        }
        let segments = self.segments(range, captures);

        // Keep the segments before `start`, cutting the one it falls in.
        let keep = self
            .segments
            .partition_point(|segment| segment.range.end <= start);
        if let Some(segment) = self.segments.get_mut(keep)
            && segment.range.start < start
        {
            segment.range.end = start;
            self.segments.truncate(keep + 1);
        } else {
            self.segments.truncate(keep);
        }
        // The cut segment is resolved again for its new range. The styles
        // handed out before are usually dropped with their block by now, so
        // they are changed in place. When they are not, copy what is kept.
        if let Some(resolved) = &mut self.resolved {
            match Arc::get_mut(&mut resolved.styles) {
                Some(styles) => styles.truncate(keep),
                None => {
                    let kept = resolved.styles[..keep.min(resolved.styles.len())].to_vec();
                    resolved.styles = Arc::new(kept);
                }
            }
        }
        self.segments.extend(segments);
    }

    /// The captures of the language's own highlights query over `range`, in
    /// the order [`SyntaxHighlighter`] collects them.
    fn host_captures(&self, range: Range<usize>, out: &mut Vec<(Range<usize>, HighlightName)>) {
        let Some(tree) = &self.tree else {
            return;
        };
        let query = &self.language.highlights;
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range);
        let mut matches = cursor.matches(&query.query, tree.root_node(), TextProvider(&self.text));
        let mut last: Option<(Range<usize>, HighlightName)> = None;
        while let Some(query_match) = StreamingIterator::next(&mut matches) {
            for capture in query_match.captures.iter() {
                let Some(name) = query.name(capture.index) else {
                    continue;
                };
                let node_range = capture.node.start_byte()..capture.node.end_byte();
                // Two captures of the same node keep the first one's name.
                let name = match &last {
                    Some((last_range, last_name)) if *last_range == node_range => *last_name,
                    _ => name,
                };
                last = Some((node_range.clone(), name));
                out.push((node_range, name));
            }
        }
    }

    /// Layers `captures` (in the order they were collected) into segments
    /// covering `range`, the way `unique_styles` layers styles.
    fn segments(
        &mut self,
        range: Range<usize>,
        captures: Vec<(Range<usize>, HighlightName)>,
    ) -> Vec<Segment> {
        let mut events = Vec::with_capacity(captures.len() * 2);
        let mut names = Vec::with_capacity(captures.len());
        for (capture, name) in captures {
            let start = capture.start.max(range.start);
            let end = capture.end.min(range.end);
            // A capture of a stale or odd tree may end inside a character, and
            // text shaping panics on a style boundary there.
            let start = self.text.clip_offset(start, Bias::Left);
            let end = self.text.clip_offset(end.max(start), Bias::Right);
            if start >= end {
                continue;
            }
            let id = names.len();
            names.push(name);
            // Captures cut at `range.start` were layered in the order they
            // started, as a highlight from scratch layers them.
            events.push((start, true, capture.start, id));
            events.push((end, false, capture.start, id));
        }
        // Ends before starts at one position, then by where each started.
        events.sort_unstable_by_key(|&(position, is_start, start, id)| {
            (position, is_start, start, id)
        });

        let mut segments = Vec::new();
        let mut active: SmallVec<[usize; 8]> = SmallVec::new();
        let mut last = range.start;
        for (position, is_start, _, id) in events {
            if position > last {
                let stack = self.stack_id(active.iter().map(|id| names[*id]).collect());
                segments.push(Segment {
                    range: last..position,
                    stack,
                });
                last = position;
            }
            if is_start {
                active.push(id);
            } else {
                active.retain(|active| *active != id);
            }
        }
        if last < range.end {
            segments.push(Segment {
                range: last..range.end,
                stack: 0,
            });
        }
        segments
    }

    fn stack_id(&mut self, stack: NameStack) -> u32 {
        if let Some(id) = self.stack_ids.get(&stack) {
            return *id;
        }
        let id = self.stacks.len() as u32;
        self.stacks.push(stack.clone());
        self.stack_ids.insert(stack, id);
        id
    }

    /// The highlight styles of the code in `theme`.
    fn styles(&mut self, theme: &Arc<super::HighlightTheme>) -> CodeBlockHighlights {
        let resolved = match &mut self.resolved {
            Some(resolved) if Arc::ptr_eq(&resolved.theme, theme) => resolved,
            resolved => resolved.insert(Resolved {
                theme: theme.clone(),
                stack_styles: Vec::new(),
                styles: Arc::new(Vec::with_capacity(self.segments.len())),
            }),
        };
        if resolved.styles.len() == self.segments.len() {
            return CodeBlockHighlights::shared(resolved.styles.clone());
        }
        resolved.stack_styles.resize(self.stacks.len(), None);
        let styles = Arc::make_mut(&mut resolved.styles);
        for segment in &self.segments[styles.len()..] {
            let style = *resolved.stack_styles[segment.stack as usize].get_or_insert_with(|| {
                let mut style = HighlightStyle::default();
                for name in &self.stacks[segment.stack as usize] {
                    if let Some(name_style) = theme.style(name.name().as_ref()) {
                        merge_highlight_style(&mut style, &name_style);
                    }
                }
                style
            });
            styles.push((segment.range.clone(), style));
        }
        CodeBlockHighlights::shared(resolved.styles.clone())
    }
}

/// The captures of an injected layer's highlights query over `range`, in
/// the order [`SyntaxHighlighter`] collects them.
fn layer_captures(
    layer: &InjectionLayer,
    range: Range<usize>,
    text: &Rope,
    out: &mut Vec<(Range<usize>, HighlightName)>,
) {
    let query = &layer.highlight_query;
    let mut cursor = QueryCursor::new();
    // Overlapping captures are dropped in order from the layer's start, so
    // a layer that started before `range` is matched from its start.
    cursor.set_byte_range(layer.byte_range.start.min(range.start)..range.end);
    let mut matches = cursor.matches(&query.query, layer.tree.root_node(), TextProvider(text));
    let mut last_end = 0usize;
    while let Some(query_match) = StreamingIterator::next(&mut matches) {
        let allow_overlapping_captures = query
            .query
            .property_settings(query_match.pattern_index)
            .iter()
            .any(|prop| prop.key.as_ref() == "highlight.allow-overlap");
        for capture in query_match.captures.iter() {
            let node_range = capture.node.start_byte()..capture.node.end_byte();
            if !allow_overlapping_captures && node_range.start < last_end {
                continue;
            }
            if let Some(name) = query.name(capture.index) {
                if !allow_overlapping_captures {
                    last_end = node_range.end;
                }
                out.push((node_range, name));
            }
        }
    }
}

/// How many siblings before a changed node are highlighted again with it: a
/// query pattern can match a node together with the siblings after it.
const SIBLING_MARGIN: usize = 2;

/// The first byte where `new`, a reparse of `old` after an edit `old` took,
/// can be highlighted differently, found by comparing the trees from their
/// roots down.
///
/// [`Tree::changed_ranges`] reports an ERROR node as changed as a whole, and
/// error recovery often wraps the whole of unfinished code in one ERROR root,
/// so every chunk of a long streamed block was highlighted again from the
/// start. Here nodes match when they have the same kind, range and number of
/// children and the edit left the old one alone. A node holding an error is
/// compared all the way down, because error recovery parses it again and can
/// build it differently. The first node that does not match, and the
/// [`SIBLING_MARGIN`] siblings before it, are highlighted again. A node of
/// the same kind and start is entered instead, to look for the first
/// difference among its children.
///
/// Error recovery also turns the root into an ERROR node and back as the
/// code goes in and out of parsing. Children of the root have no fields to
/// tell them by, so a root of another kind is entered too, unless the query
/// names one of the two kinds (`named`).
fn first_change(old: &Tree, new: &Tree, named: impl Fn(&str) -> bool) -> usize {
    let mut old = old.root_node();
    let mut new = new.root_node();
    if old.kind_id() != new.kind_id() && (named(old.kind()) || named(new.kind())) {
        return 0;
    }
    let mut is_root = true;
    loop {
        if (!is_root && old.kind_id() != new.kind_id()) || old.start_byte() != new.start_byte() {
            return old.start_byte().min(new.start_byte());
        }
        is_root = false;
        let count = old.child_count().min(new.child_count());
        let ix = (0..count)
            .find(|ix| match (old.child(*ix), new.child(*ix)) {
                (Some(old), Some(new)) => !same_node(old, new),
                _ => true,
            })
            .unwrap_or(count);
        let margin = ix
            .checked_sub(SIBLING_MARGIN)
            .and_then(|ix| old.child(ix))
            .map_or(old.start_byte(), |sibling| sibling.start_byte());
        if ix == count {
            return if old.child_count() == new.child_count() {
                // The children match, so whatever differs comes after them.
                old.end_byte().min(new.end_byte())
            } else {
                margin
            };
        }
        match (old.child(ix), new.child(ix)) {
            (Some(old_child), Some(new_child))
                if old_child.kind_id() == new_child.kind_id()
                    && old_child.start_byte() == new_child.start_byte()
                    && old_child.child_count() > 0
                    && new_child.child_count() > 0 =>
            {
                old = old_child;
                new = new_child;
            }
            _ => return margin,
        }
    }
}

/// Whether `new` is `old` again (see [`first_change`]).
fn same_node(old: tree_sitter::Node, new: tree_sitter::Node) -> bool {
    let mut pending = vec![(old, new)];
    while let Some((old, new)) = pending.pop() {
        if old.kind_id() != new.kind_id()
            || old.byte_range() != new.byte_range()
            || old.child_count() != new.child_count()
            || old.is_missing() != new.is_missing()
            || old.has_changes()
        {
            return false;
        }
        if old.has_error() || new.has_error() {
            for ix in 0..old.child_count() {
                match (old.child(ix), new.child(ix)) {
                    (Some(old), Some(new)) => pending.push((old, new)),
                    _ => return false,
                }
            }
        }
    }
    true
}

/// Moves `layer` to the text after `edit`.
fn edit_layer(layer: &mut InjectionLayer, edit: &InputEdit) {
    layer.tree.edit(edit);
    for range in &mut layer.ranges {
        edit_range(edit, range);
    }
    if let Some(byte_range) = bounding_byte_range(&layer.ranges) {
        layer.byte_range = byte_range;
    }
}

/// Drops the ranges past the combined-layer limits, as
/// `SyntaxHighlighter::compute_injection_layers` collects them.
fn limit_combined_ranges(ranges: &mut Vec<tree_sitter::Range>) {
    let mut bytes = 0usize;
    let mut keep = 0;
    for range in ranges.iter() {
        if keep >= MAX_INJECTION_RANGES {
            break;
        }
        let len = injection_range_len(range);
        if bytes.saturating_add(len) > MAX_INJECTION_BYTES {
            break;
        }
        bytes += len;
        keep += 1;
    }
    ranges.truncate(keep);
}

fn ts_point(point: gpui_base::input::Point) -> Point {
    Point::new(point.row, point.column)
}

/// The kept highlighters of this thread, the most recently used last.
struct KeptBlocks {
    blocks: Vec<CodeBlockHighlighter>,
    max_blocks: usize,
    max_bytes: usize,
}

impl Default for KeptBlocks {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            max_blocks: MAX_KEPT_BLOCKS,
            max_bytes: MAX_KEPT_BYTES,
        }
    }
}

impl KeptBlocks {
    /// Takes the highlighter of `code`, or of the longest code it extends.
    fn take(
        &mut self,
        language: &Arc<CompiledLanguage>,
        code: &SharedString,
    ) -> Option<CodeBlockHighlighter> {
        let mut best: Option<usize> = None;
        for (ix, block) in self.blocks.iter().enumerate().rev() {
            if !Arc::ptr_eq(&block.language, language) || block.source.len() > code.len() {
                continue;
            }
            let same = block.source.len() == code.len()
                && (block.source.as_ptr() == code.as_ptr() || block.source == *code);
            if same {
                best = Some(ix);
                break;
            }
            if block.tree.is_some()
                && best.is_none_or(|best| self.blocks[best].source.len() < block.source.len())
                && code.starts_with(block.source.as_ref())
            {
                best = Some(ix);
            }
        }
        best.map(|ix| self.blocks.remove(ix))
    }

    fn put(&mut self, block: CodeBlockHighlighter) {
        self.blocks.push(block);
        let mut bytes: usize = self.blocks.iter().map(CodeBlockHighlighter::len).sum();
        while self.blocks.len() > 1
            && (self.blocks.len() > self.max_blocks || bytes > self.max_bytes)
        {
            bytes -= self.blocks.remove(0).len();
        }
    }
}

thread_local! {
    static KEPT_BLOCKS: RefCell<KeptBlocks> = RefCell::default();
}

/// The highlight styles of a code block in `lang`, in `theme`.
///
/// Empty for a language without a grammar. With a `waiter`, a language that
/// is not compiled yet is compiled in the background, the code is plain until
/// then, and `waiter` tells the application when it is ready. Without one,
/// it is compiled here.
pub(crate) fn highlight_code_block(
    lang: &str,
    code: &SharedString,
    theme: &Arc<super::HighlightTheme>,
    waiter: Option<&Waiter>,
) -> CodeBlockHighlights {
    let language = match waiter {
        None => compiled_language(lang),
        Some(waiter) => match compiled_language_now(lang, waiter) {
            Readiness::Ready(language) => language,
            Readiness::Pending => None,
        },
    };
    let Some(language) = language else {
        return CodeBlockHighlights::default();
    };
    KEPT_BLOCKS.with(|kept| {
        let mut kept = kept.borrow_mut();
        let mut block = match kept.take(&language, code) {
            Some(mut block) => {
                if block.waiter.is_none() {
                    block.waiter = waiter.cloned();
                }
                block.update(code);
                block
            }
            None => match CodeBlockHighlighter::new(language, code, waiter) {
                Some(block) => block,
                None => return CodeBlockHighlights::default(),
            },
        };
        let styles = block.styles(theme);
        kept.put(block);
        styles
    })
}

/// Compiles the languages an application's code blocks wait for in the
/// background, and repaints its windows when one is ready.
struct LanguageReady {
    waiter: Waiter,
    _repaint: gpui::Task<()>,
}

impl gpui::Global for LanguageReady {}

/// Has code blocks in `cx` wait for their languages off the UI thread, and
/// repaints the windows when one is ready.
pub(crate) fn init(cx: &mut gpui::App) {
    let (sender, ready) = gpui_base::async_util::unbounded::<()>();
    let repaint = cx.spawn(async move |cx| {
        while ready.recv().await.is_ok() {
            // One repaint for every language that is ready by now.
            while ready.try_recv().is_ok() {}
            _ = cx.update(|cx| {
                // A new highlighter, so code blocks highlighted while their
                // language was not ready are highlighted again.
                if let Some(theme) = cx.try_global::<crate::Theme>().cloned() {
                    crate::text::install_text_view_defaults(&theme, cx);
                }
                cx.refresh_windows();
            });
        }
    });
    let waiter = Waiter::new(cx.background_executor().clone(), Some(sender));
    cx.set_global(LanguageReady {
        waiter,
        _repaint: repaint,
    });
}

/// The waiter to hand [`highlight_code_block`] in `cx`, when [`init`] ran.
pub(crate) fn language_ready_waiter(cx: &gpui::App) -> Option<Waiter> {
    cx.try_global::<LanguageReady>()
        .map(|ready| ready.waiter.clone())
}

#[cfg(test)]
mod tests;
