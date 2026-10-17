use futures::Stream as _;
#[cfg(not(target_family = "wasm"))]
use std::time::Instant;
use std::{
    ops::{Bound, Range, RangeBounds, RangeInclusive},
    pin::Pin,
    sync::{Arc, Mutex, OnceLock},
    task::Poll,
    time::Duration,
};
#[cfg(target_family = "wasm")]
use web_time::Instant;

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, EntityId, FocusHandle, IntoElement, KeyBinding,
    ListState, ParentElement as _, Pixels, Point, Render, SharedString, Styled as _, Subscription,
    Task, WeakEntity, Window, prelude::FluentBuilder as _, px,
};

use crate::{
    AutoScroll, ElementExt, TextSelection,
    async_util::{Receiver, Sender, unbounded},
    input::{self, SelectAll},
    text::{
        CodeBlockActionsFn, CodeBlockHighlighterFn, LinkClickHandlerFn, MarkdownExtensions,
        RangeHighlight, RangeHighlightError, RenderedText, TableActionsFn, TextViewStyle,
        document::ParsedDocument,
        format,
        node::{self, BlockNode, NodeContext},
        range_highlight::{
            LeafRemap, PendingReveal, RangeHighlightFrame, RenderedIndex, RevealRequest,
        },
        selection_adapter::TextViewSelectionAdapter,
        stream_fade::{StreamFadeTracker, TextViewMotion},
    },
    v_flex,
};

const CONTEXT: &'static str = "TextView";
// Keep coalescing bounded so sustained streams still render intermediate updates.
const MAX_COALESCED_UPDATES_PER_PARSE: usize = 64;
// Preserve exact first-layout height for small documents while bounding the
// amount of source parsed synchronously on the UI thread.
const MAX_SYNC_FULL_REPLACE_BYTES: usize = 4 * 1024;
// Repaint a streamed fade at about 30 fps, not at the display refresh rate.
const STREAM_FADE_TICK: Duration = Duration::from_millis(33);

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys(vec![
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-c", input::Copy, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-c", input::Copy, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-a", input::SelectAll, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-a", input::SelectAll, Some(CONTEXT)),
    ]);
}

/// The content format of the text view.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TextViewFormat {
    /// Markdown view
    Markdown,
    /// HTML view
    Html,
}

/// The format of the text returned by
/// [`TextViewState::selected_text`], which is also what copy writes to the
/// clipboard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SelectionFormat {
    /// The rendered text, without any markup.
    #[default]
    Plain,
    /// The source of the selection.
    ///
    /// Select-all returns the original source verbatim, a partial selection is
    /// reconstructed as Markdown from the parsed nodes (e.g. selecting inside
    /// a `**bold**` run yields `**bold**`).
    Source,
}

/// One text element's laid-out vertical extent, reported by `Inline` during
/// prepaint so `TextView` can snap its `max_lines` clip to a whole-line
/// boundary.
#[derive(Clone, Copy)]
pub(super) struct LineSpan {
    pub(super) top: Pixels,
    pub(super) bottom: Pixels,
    pub(super) line_height: Pixels,
}

/// The state of a TextView.
pub struct TextViewState {
    entity_id: EntityId,
    pub(super) focus_handle: FocusHandle,
    pub(super) list_state: ListState,

    /// The bounds of the text view
    bounds: Bounds<Pixels>,

    pub(super) selectable: bool,
    pub(super) selection_format: SelectionFormat,
    pub(super) scrollable: bool,
    pub(super) max_lines: Option<usize>,
    /// See [`crate::text::TextView::code_block_max_lines`].
    pub(super) code_block_max_lines: Option<usize>,
    /// Line spans reported by `Inline` during prepaint (collected only while
    /// [`Self::max_lines`] is set); cleared by `TextView` at each frame start.
    pub(super) line_spans: Arc<Mutex<Vec<LineSpan>>>,
    /// Whether the last painted frame clipped content due to `max_lines`.
    pub(super) clamped: bool,
    pub(super) text_view_style: Arc<TextViewStyle>,
    pub(super) code_block_actions: Option<std::sync::Arc<CodeBlockActionsFn>>,
    pub(super) code_block_highlighter: Option<std::sync::Arc<CodeBlockHighlighterFn>>,
    pub(super) table_actions: Option<std::sync::Arc<TableActionsFn>>,
    pub(super) image_source: Option<std::sync::Arc<super::text_view::ImageSourceFn>>,
    pub(super) link_click_handler: Option<std::sync::Arc<LinkClickHandlerFn>>,
    pub(super) markdown_extensions: Arc<MarkdownExtensions>,

    pub(super) is_selecting: bool,
    /// Logical ranges retained across an explicitly requested resource reflow.
    pub(super) preserve_inline_selection: bool,
    multi_click_selection: Option<TextViewMultiClickSelection>,
    selected_text_override: Option<String>,
    select_all: bool,
    pub(super) auto_scroll: AutoScroll,
    pub(super) selection_adapter: TextViewSelectionAdapter,

    pub(super) parsed_content: ParsedContent,
    pub(super) stream_fade: StreamFadeTracker,
    fade_tick: Option<Task<()>>,
    /// Content format (markdown / html), used for bounded synchronous parsing
    /// of small full-replace updates.
    format: TextViewFormat,
    text: String,
    /// The text a `TextView` element last handed over, and the length of
    /// `text` then, to recognize the same string next frame without
    /// comparing its bytes.
    element_text: Option<(SharedString, usize)>,
    revision: usize,
    /// The revision of the update `parsed_content` was committed from.
    committed_revision: usize,
    /// The revision of the last update whose parse landed, whether it was
    /// committed or failed.
    landed_revision: usize,
    /// The revision of the last update that replaced the text rather than
    /// appending to it.
    full_update_revision: usize,
    /// The rendered text of `parsed_content`, built when first read.
    rendered_index: Arc<OnceLock<RenderedIndex>>,
    range_highlights: Option<Arc<RangeHighlightFrame>>,
    pub(super) pending_reveal: Option<PendingReveal>,
    pub(super) selection_revision: usize,
    compatible_layout_update: bool,
    layout_text_style: Option<(gpui::TextStyle, Pixels)>,
    parsed_error: Option<SharedString>,
    /// Parses text in the background. A [`Self::block_range`] view parses
    /// nothing and has none until it is given text of its own.
    parser: Option<BackgroundParser>,
    /// The document this view shows some blocks of, for a view made by
    /// [`Self::block_range`].
    block_range: Option<BlockRange>,
    /// The revision [`Self::finish`] marked the text finished at.
    finished: Option<usize>,
}

/// The background parse of a [`TextViewState`]'s text, and the task that
/// commits its results.
struct BackgroundParser {
    tx: Sender<UpdateOptions>,
    _parse_task: Task<()>,
    _receive_task: Task<()>,
}

impl BackgroundParser {
    fn new(format: TextViewFormat, cx: &mut Context<TextViewState>) -> Self {
        let (tx, rx) = unbounded::<UpdateOptions>();
        let (tx_result, rx_result) = unbounded::<ParsedUpdate>();
        let _receive_task = cx.spawn({
            async move |weak_self, cx| {
                while let Ok(parsed_update) = rx_result.recv().await {
                    _ = weak_self.update(cx, |state, cx| {
                        state.commit_parsed_update(parsed_update, cx);
                    });
                }
            }
        });
        let _parse_task = cx.background_spawn(UpdateFuture::new(format, rx, tx_result));
        Self {
            tx,
            _parse_task,
            _receive_task,
        }
    }
}

/// The document a [`TextViewState::block_range`] view shows some blocks of.
struct BlockRange {
    document: WeakEntity<TextViewState>,
    /// The document's top-level blocks the view shows, by index.
    blocks: (Bound<usize>, Bound<usize>),
    /// The document's `full_update_revision` and `committed_revision` the
    /// view last took its blocks at, and whether those blocks were complete.
    synced: Option<(usize, usize, bool)>,
    /// The document's source length then: where text it appended starts.
    synced_len: usize,
    _observe: Subscription,
}

/// Identifies a top-level block of a document while text is only appended to
/// it, and tells a block from the blocks before a replacement of the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextBlockId {
    replacement: usize,
    start: usize,
}

/// What a top-level block of a document is.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextBlockContent {
    /// A paragraph, which may hold only an image.
    Paragraph,
    /// A heading of the given level, 1 to 6.
    Heading(u8),
    /// A bulleted, numbered or task list.
    List,
    /// A fenced or indented code block, with the language its fence names.
    CodeBlock(Option<SharedString>),
    /// A table.
    Table,
    /// A block quote.
    Quote,
    /// A thematic break.
    Rule,
    /// A link reference definition, which shows nothing.
    Definition,
    /// A block of a [`MarkdownExtensions`] plugin, by its node name.
    Custom(SharedString),
    /// Anything else, such as an HTML container.
    Other,
}

/// A top-level block of a document: what a long document is split into to
/// show it as rows of a list, each a [`TextViewState::block_range`] view.
#[derive(Clone, Debug, PartialEq)]
pub struct TextBlock {
    index: usize,
    id: TextBlockId,
    source_range: Option<Range<usize>>,
    content: TextBlockContent,
    is_complete: bool,
}

impl TextBlock {
    /// The block's place among the document's top-level blocks.
    pub fn index(&self) -> usize {
        self.index
    }

    /// The block's identity, which stays the same while text is appended to
    /// the document.
    pub fn id(&self) -> TextBlockId {
        self.id
    }

    /// The block's range of the Markdown source, when the parser recorded it
    /// (the HTML parser records none).
    pub fn source_range(&self) -> Option<Range<usize>> {
        self.source_range.clone()
    }

    /// What the block is.
    pub fn content(&self) -> &TextBlockContent {
        &self.content
    }

    /// Whether appending text to the document can no longer change the
    /// block: true of every block but the last.
    pub fn is_complete(&self) -> bool {
        self.is_complete
    }
}

impl TextBlockContent {
    fn of(block: &BlockNode) -> Self {
        match block {
            BlockNode::Paragraph(_) => Self::Paragraph,
            BlockNode::Heading { level, .. } => Self::Heading(*level),
            BlockNode::List { .. } | BlockNode::ListItem { .. } => Self::List,
            BlockNode::CodeBlock(code_block) => Self::CodeBlock(code_block.lang()),
            BlockNode::Table(_) => Self::Table,
            BlockNode::Blockquote { .. } => Self::Quote,
            BlockNode::HorizontalRule { .. } => Self::Rule,
            BlockNode::Definition { .. } => Self::Definition,
            BlockNode::Custom(node) => Self::Custom(node.name().to_string().into()),
            BlockNode::Root { .. } | BlockNode::Break { .. } | BlockNode::Unknown => Self::Other,
        }
    }
}

impl TextViewState {
    /// Create a Markdown TextViewState.
    pub fn markdown(text: &str, cx: &mut Context<Self>) -> Self {
        Self::new(TextViewFormat::Markdown, text, cx)
    }

    /// Create a HTML TextViewState.
    pub fn html(text: &str, cx: &mut Context<Self>) -> Self {
        Self::new(TextViewFormat::Html, text, cx)
    }

    /// Create a new TextViewState.
    fn new(format: TextViewFormat, text: &str, cx: &mut Context<Self>) -> Self {
        let mut this = Self::without_text(format, cx);
        this.parser = Some(BackgroundParser::new(format, cx));
        this.text = text.to_string();
        this.increment_update(text, false, cx);
        this
    }

    /// A state that has parsed no text, and has no background parser yet.
    fn without_text(format: TextViewFormat, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let selection_adapter = TextViewSelectionAdapter::new(cx.entity().downgrade(), cx);

        Self {
            entity_id: cx.entity_id(),
            focus_handle,
            bounds: Bounds::default(),
            multi_click_selection: None,
            selected_text_override: None,
            select_all: false,
            selectable: false,
            selection_format: SelectionFormat::default(),
            scrollable: false,
            max_lines: None,
            code_block_max_lines: None,
            line_spans: Arc::default(),
            clamped: false,
            // Measure all blocks (not just visible ones) so the scrollbar
            // thumb size stays stable. Without this, off-screen blocks count
            // as zero height until scrolled into view, which makes the
            // scrollbar jitter as more blocks get measured during scrolling.
            list_state: ListState::new(0, gpui::ListAlignment::Top, px(1000.)).measure_all(),
            text_view_style: Arc::default(),
            code_block_actions: None,
            code_block_highlighter: None,
            table_actions: None,
            link_click_handler: None,
            image_source: None,
            markdown_extensions: Arc::default(),
            is_selecting: false,
            preserve_inline_selection: false,
            auto_scroll: AutoScroll::default(),
            selection_adapter,
            parsed_content: Default::default(),
            stream_fade: StreamFadeTracker::default(),
            fade_tick: None,
            format,
            parsed_error: None,
            text: String::new(),
            element_text: None,
            revision: 0,
            committed_revision: 0,
            landed_revision: 0,
            full_update_revision: 0,
            rendered_index: Arc::default(),
            range_highlights: None,
            pending_reveal: None,
            selection_revision: 0,
            compatible_layout_update: false,
            layout_text_style: None,
            parser: None,
            block_range: None,
            finished: None,
        }
    }

    /// A view of the top-level blocks `blocks` of `document` (by index, see
    /// [`Self::blocks`]), which keeps parsing the text: one row of a long
    /// document shown as a list of rows.
    ///
    /// Only the rows a list lays out are built, so a long streamed answer
    /// costs what its visible rows cost. Each view selects on its own, and a
    /// selection that starts in one row and ends in another spans the rows
    /// between, as it spans separate text views. Text pushed to `document`
    /// reaches the view once it is parsed, and a view whose blocks are all
    /// complete is left alone from then on. Show either a document or views
    /// of its blocks, as they share the parsed blocks and their selection.
    ///
    /// A range past the document's end shows the blocks that exist. A range
    /// open at the end, such as `4..`, takes in the blocks appended later.
    pub fn block_range(
        document: &Entity<TextViewState>,
        blocks: impl RangeBounds<usize>,
        cx: &mut Context<Self>,
    ) -> Self {
        let format = document.read(cx).format;
        let mut this = Self::without_text(format, cx);
        let observe = cx.observe(document, |this, document, cx| {
            this.sync_block_range(&document, cx);
        });
        this.block_range = Some(BlockRange {
            document: document.downgrade(),
            blocks: (blocks.start_bound().cloned(), blocks.end_bound().cloned()),
            synced: None,
            synced_len: 0,
            _observe: observe,
        });
        this.sync_block_range(document, cx);
        this
    }

    /// Changes the blocks a [`Self::block_range`] view shows. Does nothing to
    /// another view.
    pub fn set_block_range(&mut self, blocks: impl RangeBounds<usize>, cx: &mut Context<Self>) {
        let Some(block_range) = &mut self.block_range else {
            return;
        };
        let blocks = (blocks.start_bound().cloned(), blocks.end_bound().cloned());
        if block_range.blocks == blocks {
            return;
        }
        block_range.blocks = blocks;
        block_range.synced = None;
        if let Some(document) = block_range.document.upgrade() {
            self.sync_block_range(&document, cx);
        }
    }

    /// The top-level blocks the view shows, as of the last parse that landed,
    /// with their index among the document's blocks.
    pub fn blocks(&self) -> Vec<TextBlock> {
        self.blocks_from(0)
    }

    /// The top-level blocks the view shows from the document's block
    /// `index` on: [`Self::blocks`] without the ones before it.
    pub fn blocks_from(&self, index: usize) -> Vec<TextBlock> {
        let blocks = &self.parsed_content.document.blocks;
        let first = self.block_range_start();
        let complete = self.block_range_is_complete() || self.is_finished_and_parsed();
        let document_len = first + blocks.len() + usize::from(complete);
        let replacement = self.block_range_replacement();
        blocks
            .iter()
            .enumerate()
            .skip(index.saturating_sub(first))
            .map(|(ix, block)| {
                let index = first + ix;
                let span = block.span();
                TextBlock {
                    index,
                    id: TextBlockId {
                        replacement,
                        start: span.map_or(index, |span| span.start),
                    },
                    source_range: span.map(|span| span.start..span.end),
                    content: TextBlockContent::of(block),
                    is_complete: index + 1 < document_len,
                }
            })
            .collect()
    }

    /// How many top-level blocks the view shows, as of the last parse that
    /// landed. Cheaper than [`Self::blocks`] to tell whether blocks were
    /// added.
    pub fn block_count(&self) -> usize {
        self.parsed_content.document.blocks.len()
    }

    /// Marks the text finished: nothing more will be appended, so the last
    /// block is complete too, and a [`Self::block_range`] view of it is left
    /// alone from then on like the others. Appending text, or setting text
    /// that replaces it, opens it again.
    ///
    /// Text given just before, in the same frame or the same update, is
    /// parsed first: the last block counts as complete once that parse lands.
    pub fn finish(&mut self, cx: &mut Context<Self>) {
        if self.finished.is_some() {
            return;
        }
        self.finished = Some(self.revision);
        self.mark_last_block_complete();
        cx.notify();
    }

    /// Whether the text is finished and the parse of all of it has landed,
    /// so that no block can change any more.
    fn is_finished_and_parsed(&self) -> bool {
        self.finished
            .is_some_and(|revision| self.landed_revision >= revision)
    }

    /// Marks the code blocks of the last block complete, once the text is
    /// finished and parsed: a code block left open can't grow either.
    fn mark_last_block_complete(&mut self) {
        if self.is_finished_and_parsed()
            && self.block_range.is_none()
            && let Some(last) = Arc::make_mut(&mut self.parsed_content.document.blocks).last_mut()
        {
            last.mark_code_complete();
        }
    }

    /// Whether [`Self::finish`] marked the text finished, and no text was
    /// appended or set since.
    pub fn is_finished(&self) -> bool {
        self.finished.is_some()
    }

    /// The index of the document's block the view starts with.
    fn block_range_start(&self) -> usize {
        match self.block_range.as_ref().map(|range| range.blocks.0) {
            Some(Bound::Included(start)) => start,
            Some(Bound::Excluded(start)) => start + 1,
            _ => 0,
        }
    }

    /// Whether the view shows blocks that are all complete.
    fn block_range_is_complete(&self) -> bool {
        self.block_range
            .as_ref()
            .and_then(|range| range.synced)
            .is_some_and(|(_, _, complete)| complete)
    }

    /// Which replacement of its text the document is at, as far as the view
    /// knows.
    fn block_range_replacement(&self) -> usize {
        match &self.block_range {
            Some(range) => range.synced.map_or(0, |(replacement, _, _)| replacement),
            None => self.full_update_revision,
        }
    }

    /// Takes the view's blocks from `document`'s latest parse.
    fn sync_block_range(&mut self, document: &Entity<TextViewState>, cx: &mut Context<Self>) {
        let Some(block_range) = &self.block_range else {
            return;
        };
        let document = document.read(cx);
        let replacement = document.full_update_revision;
        let revision = document.committed_revision;
        let synced = block_range.synced;
        let extensions = (!Arc::ptr_eq(&self.markdown_extensions, &document.markdown_extensions))
            .then(|| document.markdown_extensions.clone());
        let all = &document.parsed_content.document.blocks;
        let start = match block_range.blocks.0 {
            Bound::Included(start) => start,
            Bound::Excluded(start) => start + 1,
            Bound::Unbounded => 0,
        }
        .min(all.len());
        let end = match block_range.blocks.1 {
            Bound::Included(end) => end + 1,
            Bound::Excluded(end) => end,
            Bound::Unbounded => all.len(),
        }
        .clamp(start, all.len());
        let complete = end < all.len() || document.is_finished_and_parsed();
        // A view of complete blocks has nothing to take while text is only
        // appended.
        if synced.is_some_and(|(synced_replacement, synced_revision, synced_complete)| {
            synced_replacement == replacement
                && synced_complete == complete
                && (synced_revision == revision || complete)
        }) {
            if let Some(extensions) = extensions {
                self.markdown_extensions = extensions;
                cx.notify();
            }
            return;
        }
        let appended =
            synced.is_some_and(|(synced_replacement, _, _)| synced_replacement == replacement);
        let source = document.parsed_content.document.source.clone();
        let source_len = source.len();
        let new = ParsedContent {
            document: ParsedDocument {
                source,
                blocks: Arc::new(all[start..end].to_vec()),
            },
            node_cx: document.parsed_content.node_cx.clone(),
        };
        let synced_len = block_range.synced_len;
        let unchanged = appended
            && new.document.blocks.len() == self.parsed_content.document.blocks.len()
            && new
                .document
                .blocks
                .iter()
                .zip(self.parsed_content.document.blocks.iter())
                .all(|(new, old)| new.span() == old.span() && new == old);
        if let Some(block_range) = &mut self.block_range {
            block_range.synced = Some((replacement, revision, complete));
            block_range.synced_len = source_len;
        }
        if let Some(extensions) = extensions {
            self.markdown_extensions = extensions;
            if unchanged {
                cx.notify();
            }
        }
        if unchanged {
            return;
        }

        if appended {
            self.stream_fade.note_extend(synced_len);
        } else {
            self.stream_fade.note_replace();
            self.full_update_revision = self.revision + 1;
        }
        self.revision += 1;
        self.reconcile_range_highlights(&new.document, self.revision, appended);
        self.stream_fade
            .record(&self.parsed_content.document, &new.document, Instant::now());
        self.parsed_content = new;
        self.parsed_error = None;
        if appended {
            self.compatible_layout_update = true;
        } else {
            self.invalidate_measured_heights();
            if !self.is_selecting {
                self.reset_selection_and_adapter(cx);
            }
        }
        cx.notify();
    }

    /// Get the text content.
    pub(crate) fn source(&self) -> SharedString {
        self.parsed_content.document.source.clone()
    }

    /// Set whether the text is selectable, default false.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    /// Set whether the text is selectable, default false.
    pub fn set_selectable(&mut self, selectable: bool, cx: &mut Context<Self>) {
        self.selectable = selectable;
        cx.notify();
    }

    /// Set the [`SelectionFormat`], default is [`SelectionFormat::Plain`].
    pub fn selection_format(mut self, selection_format: SelectionFormat) -> Self {
        self.selection_format = selection_format;
        self
    }

    /// Set the [`SelectionFormat`], default is [`SelectionFormat::Plain`].
    pub fn set_selection_format(
        &mut self,
        selection_format: SelectionFormat,
        cx: &mut Context<Self>,
    ) {
        self.selection_format = selection_format;
        cx.notify();
    }

    /// Set whether the text view scrolls internally, default false.
    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    /// Set whether the text view scrolls internally, default false.
    pub fn set_scrollable(&mut self, scrollable: bool, cx: &mut Context<Self>) {
        if !scrollable {
            self.reset_selection_and_adapter(cx);
        }
        self.scrollable = scrollable;
        cx.notify();
    }

    /// Whether the last painted frame clipped content because of
    /// [`TextView::max_lines`](crate::text::TextView::max_lines).
    pub fn is_clamped(&self) -> bool {
        self.clamped
    }

    /// Set the text content.
    ///
    /// Markdown that extends the current non-empty text is appended like
    /// [`Self::push_str`], keeping the selection; any other text replaces it.
    /// With a streamed fade-in enabled, extended text fades in; a replacement
    /// shows at once.
    ///
    /// Text that differs from the current text only in the spaces, tabs and
    /// line breaks before its first line or after its last, such as the
    /// current text trimmed, shows the same and leaves the view as it is: the
    /// blocks keep their [`TextBlockId`]s, views of them aren't touched, and
    /// the selection stays. The view keeps the current text, so a code block
    /// left open at its end keeps the blank lines it ends with.
    ///
    /// A [`Self::block_range`] view stops following its document and shows
    /// the text from then on.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.block_range.take().is_some() {
            self.text.clear();
        }
        if self.text.as_str() == text || differs_only_around(&self.text, text) {
            return;
        }
        // HTML blocks carry no spans, so an append would parse the delta as a
        // standalone document. After a failed parse the background parser's
        // document lacks that text, so only a full parse resynchronizes it.
        if self.format == TextViewFormat::Markdown
            && self.parsed_error.is_none()
            && !self.text.is_empty()
            && let Some(delta) = text.strip_prefix(self.text.as_str())
        {
            self.push_str(delta, cx);
            return;
        }
        self.stream_fade.note_replace();
        self.finished = None;

        self.text.clear();
        self.text.push_str(text);
        self.parsed_error = None;
        self.increment_update(text, false, cx);
    }

    /// [`Self::set_text`] for the text a `TextView` element hands over every
    /// frame: the string it handed over last time (the same allocation, not
    /// merely equal bytes) is recognized without a comparison, as long as the
    /// state still holds a text of its length.
    pub(super) fn set_element_text(&mut self, text: &SharedString, cx: &mut Context<Self>) {
        let unchanged = self
            .element_text
            .as_ref()
            .is_some_and(|(current, text_len)| {
                current.as_ptr() == text.as_ptr()
                    && current.len() == text.len()
                    && self.text.len() == *text_len
            });
        if unchanged {
            return;
        }
        self.set_text(text, cx);
        self.element_text = Some((text.clone(), self.text.len()));
    }

    /// Append partial text content to the existing text.
    ///
    /// Push text to the document of a [`Self::block_range`] view, not to the
    /// view: the view stops following its document and shows the text.
    pub fn push_str(&mut self, new_text: &str, cx: &mut Context<Self>) {
        if new_text.is_empty() {
            return;
        }
        if self.block_range.take().is_some() {
            self.text.clear();
            self.set_text(new_text, cx);
            return;
        }
        self.finished = None;
        self.stream_fade.note_extend(self.text.len());
        self.text.push_str(new_text);
        self.increment_update(new_text, true, cx);
    }

    /// Set the motion policy; see [`TextViewMotion`].
    pub fn motion(mut self, motion: TextViewMotion) -> Self {
        self.set_motion(motion);
        self
    }

    /// Set the motion policy; see [`TextViewMotion`].
    pub fn set_motion(&mut self, motion: TextViewMotion) {
        self.stream_fade.set_motion(motion);
    }

    /// Set the Markdown extensions the text parses and renders with,
    /// [`MarkdownPlugin`](crate::text::MarkdownPlugin)s included.
    ///
    /// A [`TextView`](crate::text::TextView) given extensions or plugins sets
    /// its state's every frame. For a document shown as
    /// [`Self::block_range`] views, set them on the document: the views parse
    /// nothing and render with their document's.
    pub fn markdown_extensions(mut self, extensions: impl Into<Arc<MarkdownExtensions>>) -> Self {
        let extensions = extensions.into();
        if self.block_range.is_some()
            || self.markdown_extensions.revision() == extensions.revision()
        {
            return self;
        }
        let reparse = !self
            .markdown_extensions
            .has_same_parser_configuration(&extensions);
        self.markdown_extensions = extensions;
        if reparse && self.format == TextViewFormat::Markdown && !self.text.is_empty() {
            self.revision += 1;
            self.full_update_revision = self.revision;
            self.selection_revision = self.selection_revision.wrapping_add(1);
            if let Some(parser) = &self.parser {
                _ = parser.tx.try_send(UpdateOptions {
                    revision: self.revision,
                    append: false,
                    mode: ParseMode::Replace,
                    pending_text: self.text.clone(),
                    markdown_extensions: self.markdown_extensions.clone(),
                });
            }
        }
        self
    }

    /// Set the Markdown extensions. See [`Self::markdown_extensions`]. Does
    /// nothing to a [`Self::block_range`] view.
    pub fn set_markdown_extensions(
        &mut self,
        extensions: impl Into<Arc<MarkdownExtensions>>,
        cx: &mut Context<Self>,
    ) {
        if self.apply_markdown_extensions(extensions.into(), cx) {
            cx.notify();
        }
    }

    /// Takes the extensions a `TextView` element hands over, and tells
    /// whether they differ from the current ones.
    pub(crate) fn apply_markdown_extensions(
        &mut self,
        markdown_extensions: Arc<MarkdownExtensions>,
        cx: &mut Context<Self>,
    ) -> bool {
        // A block range view shows what its document parsed, with the
        // document's extensions.
        if self.block_range.is_some()
            || self.markdown_extensions.revision() == markdown_extensions.revision()
        {
            return false;
        }

        let parser_configuration_changed = !self
            .markdown_extensions
            .has_same_parser_configuration(&markdown_extensions);
        self.markdown_extensions = markdown_extensions;
        if parser_configuration_changed && self.format == TextViewFormat::Markdown {
            let text = self.text.clone();
            self.increment_update(&text, false, cx);
        }
        true
    }

    /// Return the selected text, in the view's [`SelectionFormat`].
    pub fn selected_text(&self) -> String {
        self.selected_text_in(None)
    }

    /// Return the original Markdown source byte range corresponding to the
    /// rendered selection.
    ///
    /// The range addresses the source passed to this Markdown TextView. It is
    /// derived from parser positions retained by the rendered inline nodes, so
    /// identical rendered text maps to the occurrence that was actually
    /// selected. The result is one contiguous source range, so it includes any
    /// Markdown delimiters between the selected rendered endpoints. HTML views
    /// and selections without an exact source mapping return `None`. Select-all
    /// in a Markdown view returns the full source range.
    pub fn selected_source_range(&self) -> Option<Range<usize>> {
        if self.format != TextViewFormat::Markdown {
            return None;
        }
        if self.select_all {
            return Some(self.shown_source_range());
        }
        self.parsed_content.document.selected_source_range()
    }

    /// The format to copy in, which is [`SelectionFormat::Plain`] whenever the
    /// requested one cannot be produced.
    ///
    /// Only a Markdown view can return source. Reconstructing HTML would mean
    /// spelling every attribute back out — a mark's color, an image's
    /// dimensions, a cell's alignment — with a new way to lose one at each
    /// step, and html5ever records no source offsets to fall back on (it
    /// reports only line numbers), so there is no original text to copy from
    /// either.
    /// The copy group of a [`Self::block_range`] view: the views of one
    /// document copy as one view of it would, their blocks apart as blocks
    /// are within a view.
    pub(super) fn copy_group(&self) -> Option<crate::TextSelectionCopyGroup> {
        let document = self.block_range.as_ref()?.document.entity_id();
        let separator = match self.effective_format() {
            SelectionFormat::Plain => "\n",
            SelectionFormat::Source => "\n\n",
        };
        Some(crate::TextSelectionCopyGroup::new(
            document.as_u64(),
            separator,
        ))
    }

    fn effective_format(&self) -> SelectionFormat {
        match self.format {
            TextViewFormat::Markdown => self.selection_format,
            TextViewFormat::Html => SelectionFormat::Plain,
        }
    }

    /// The range of the source the view shows: all of it, or the blocks of a
    /// [`Self::block_range`] view.
    fn shown_source_range(&self) -> Range<usize> {
        let source = self.source();
        if self.block_range.is_none() {
            return 0..source.len();
        }
        let blocks = &self.parsed_content.document.blocks;
        let start = blocks
            .first()
            .and_then(|block| block.span())
            .map(|span| span.start);
        let end = blocks
            .last()
            .and_then(|block| block.span())
            .map(|span| span.end);
        match (start, end) {
            (Some(start), Some(end)) if start <= end && end <= source.len() => start..end,
            _ => 0..0,
        }
    }

    /// Return the selected text, with `blocks` bounding which top-level blocks
    /// the selection covers.
    ///
    /// The range comes from the selection endpoints, which know their block
    /// even after it scrolls out of view; see
    /// [`ParsedDocument::selected_text`](crate::text::document::ParsedDocument).
    pub(super) fn selected_text_in(&self, blocks: Option<RangeInclusive<usize>>) -> String {
        let format = self.effective_format();

        if self.select_all {
            if format == SelectionFormat::Source {
                return self.source()[self.shown_source_range()].to_string();
            }

            return self.parsed_content.document.text();
        }

        // A multi-click stores the plain text it selected, which is a shortcut
        // past the block walk. Source mode cannot take it: the word it stored
        // has lost its markup. The click also set the inline selection it came
        // from, so the walk reconstructs the same range with the markup intact.
        if format != SelectionFormat::Source
            && let Some(text) = &self.selected_text_override
        {
            return text.clone();
        }

        self.parsed_content.document.selected_text(format, blocks)
    }

    /// Force a full re-measure of the block list after the document has been
    /// replaced.
    ///
    /// `Document::render_root` only calls `ListState::reset` when the block
    /// *count* changes, and the full-measure pass enabled by `measure_all` is
    /// a one-shot latch that only `reset`, `remeasure_items`, or a width
    /// change re-arms. Replacing a document with one that happens to have the
    /// same number of blocks therefore leaves every cached height belonging to
    /// the *previous* document. The list summary height stays wrong, and since
    /// the wheel clamps against that summary, the blocks past the false bottom
    /// can never scroll into view to be re-measured -- the clamp seals itself.
    ///
    /// `remeasure_items` re-arms the latch and marks the items unmeasured
    /// while keeping their old sizes as hints, so the scrollbar does not
    /// collapse in the frame before the next layout measures the real heights.
    fn invalidate_measured_heights(&self) {
        let count = self.list_state.item_count();
        if count > 0 {
            self.list_state.remeasure_items(0..count);
        }
    }

    /// Remeasure inline resources after their prepared content or metrics change.
    ///
    /// Call from the resource owner's completion handler, through a weak entity
    /// when the task can outlive this view. This does not reparse the document.
    /// Existing logical selection is retained until the next selection gesture.
    pub fn invalidate_inline_layout(&mut self, cx: &mut Context<Self>) {
        self.preserve_inline_selection = true;
        self.compatible_layout_update = true;
        self.invalidate_measured_heights();
        cx.notify();
    }

    fn increment_update(&mut self, text: &str, append: bool, cx: &mut Context<Self>) {
        self.revision += 1;
        if !append {
            self.full_update_revision = self.revision;
            self.selection_revision = self.selection_revision.wrapping_add(1);
        }
        let parse_synchronously = !append && text.len() <= MAX_SYNC_FULL_REPLACE_BYTES;
        let update_options = UpdateOptions {
            revision: self.revision,
            append,
            mode: if append {
                ParseMode::Compatible
            } else if parse_synchronously {
                ParseMode::BaselineAck
            } else {
                ParseMode::Replace
            },
            pending_text: text.to_string(),
            markdown_extensions: self.markdown_extensions.clone(),
        };

        // Keep small full replacements synchronous so their first layout has
        // the exact content height. Larger replacements use the existing
        // background parser, bounding synchronous parser input on the UI thread.
        if parse_synchronously {
            match parse_content(self.format, ParsedContent::default(), &update_options) {
                Ok(content) => {
                    self.reconcile_range_highlights(&content.document, self.revision, false);
                    self.stream_fade.record(
                        &self.parsed_content.document,
                        &content.document,
                        Instant::now(),
                    );
                    self.parsed_content = content;
                    self.parsed_error = None;
                    self.invalidate_measured_heights();
                    if !self.is_selecting {
                        self.reset_selection_and_adapter(cx);
                    }
                }
                Err(err) => {
                    self.stream_fade.discard_pending();
                    self.parsed_error = Some(err);
                    self.landed_revision = self.revision;
                }
            }
            // Keep the background parser's accumulated document in sync so a
            // later append extends this baseline instead of parsing the delta
            // as a standalone document.
            _ = self.background_parser(cx).tx.try_send(update_options);
            cx.notify();
            return;
        }

        _ = self.background_parser(cx).tx.try_send(update_options);
    }

    /// The background parser, started now if the state had none.
    fn background_parser(&mut self, cx: &mut Context<Self>) -> &BackgroundParser {
        let format = self.format;
        self.parser
            .get_or_insert_with(|| BackgroundParser::new(format, cx))
    }

    /// Commit a result of the background parser.
    ///
    /// A stream that appends faster than a parse completes has always moved
    /// past the revision the result was parsed from, so only discarding
    /// results of an older revision would show nothing until the stream
    /// stops. A result parsed since the text was last replaced is a prefix of
    /// the current text and is committed; one from before that replacement,
    /// or older than what is already committed, is discarded.
    fn commit_parsed_update(&mut self, parsed_update: ParsedUpdate, cx: &mut Context<Self>) {
        if parsed_update.revision < self.full_update_revision
            || parsed_update.revision <= self.committed_revision
        {
            return;
        }
        if parsed_update.baseline_ack {
            debug_assert!(parsed_update.full_parse);
            return;
        }

        match parsed_update.result {
            Ok(content) => {
                let append = parsed_update.selection_compatible && !parsed_update.full_parse;
                if append && self.full_update_revision <= self.committed_revision {
                    self.splice_appended_blocks(&content.document);
                }
                self.reconcile_range_highlights(&content.document, parsed_update.revision, append);
                self.stream_fade.record(
                    &self.parsed_content.document,
                    &content.document,
                    Instant::now(),
                );
                // This result may cover only part of the queued appends.
                // Keep the uncommitted tail pending from this document's end,
                // rather than consuming its fade with the earlier chunk.
                if parsed_update.revision < self.revision {
                    self.stream_fade.note_extend(content.document.source.len());
                }
                self.parsed_content = content;
                self.mark_last_block_complete();
                self.parsed_error = None;
                self.compatible_layout_update = parsed_update.selection_compatible;
                if parsed_update.full_parse {
                    self.invalidate_measured_heights();
                }
            }
            Err(err) => {
                self.stream_fade.discard_pending();
                self.parsed_error = Some(err);
                self.landed_revision = parsed_update.revision;
            }
        }
        // Don't interrupt an active drag-selection; the stored
        // positions remain valid for append-only updates and will
        // self-correct on the next mouse-move event.
        if !parsed_update.selection_compatible && !self.is_selecting {
            self.reset_selection_and_adapter(cx);
        }
        cx.notify();
    }

    /// The text this view renders, which [`RangeHighlight`] ranges index.
    ///
    /// This is the string plain copy produces, as of the last parse that
    /// landed; text set since then is not in it until its parse lands.
    pub fn rendered_text(&self) -> RenderedText {
        RenderedText::new(
            self.entity_id,
            self.committed_revision,
            self.parsed_content.document.clone(),
            self.rendered_index.clone(),
        )
    }

    /// Replace the range highlights, whose ranges index the current rendered text.
    ///
    /// Compute ranges from the current [`Self::rendered_text`] and set them in
    /// the same update. Search the new text again after its content changes.
    /// Ranges of the Markdown source, such as [`Self::selected_source_range`]
    /// returns, convert with [`RenderedText::range_for_source`].
    /// A range crossing blocks paints in both, skipping their separator.
    /// Text outside every block (separators, custom blocks, HTML blocks, and
    /// inline objects) is left unpainted. Any invalid range rejects the set.
    ///
    /// Highlights follow unchanged text through updates. Text appended while
    /// streaming keeps earlier highlights; after a table edit, cells in and
    /// after the edited row lose theirs because cells are known by position.
    /// Text backgrounds such as `<mark>` paint over a highlight; inline code
    /// backgrounds paint under it.
    pub fn set_range_highlights(
        &mut self,
        highlights: impl IntoIterator<Item = RangeHighlight>,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeHighlightError> {
        if self.format != TextViewFormat::Markdown {
            return Err(RangeHighlightError::Unsupported);
        }
        let text = self.rendered_text();
        self.range_highlights = RangeHighlightFrame::new(&text, highlights)?.map(Arc::new);
        cx.notify();
        Ok(())
    }

    /// Scroll the line `range` starts on into view, `range` indexing the
    /// current rendered text, as with [`Self::set_range_highlights`].
    ///
    /// A [`Self::scrollable`] view scrolls itself. Otherwise the nearest
    /// enclosing `gpui::list` scrolls, as long as the row holding the view is
    /// laid out, so scroll to that row first when it may be off screen. Any
    /// other scroll container scrolls through
    /// [`TextView::on_reveal`](crate::text::TextView::on_reveal). An empty
    /// range reveals the line of its position; a range in text that belongs
    /// to no block's text, such as a custom block's, scrolls its whole block
    /// into a scrollable view.
    ///
    /// Only the latest reveal is carried out. It follows the content as
    /// [range highlights](Self::set_range_highlights) do, and it is dropped
    /// when its text changes, when the view clamps its lines, or when it
    /// cannot be shown within a second, so it never scrolls long after it
    /// was asked for. A whole block taller than the view, scrolled to from
    /// below, shows its end.
    ///
    /// Revealing is best effort: `Ok(())` means the range is valid for the
    /// current text and the request was taken, not that the view has
    /// scrolled. A dropped request is not reported.
    pub fn reveal_range(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeHighlightError> {
        if self.format != TextViewFormat::Markdown {
            return Err(RangeHighlightError::Unsupported);
        }
        let text = self.rendered_text();
        if text.is_empty() && range == (0..0) {
            // Nothing to reveal in an empty view.
            self.pending_reveal = None;
            return Ok(());
        }
        let now = cx.background_executor().now();
        self.pending_reveal = Some(
            PendingReveal::new(&text, &range, now).ok_or(RangeHighlightError::InvalidRange(0))?,
        );
        cx.notify();
        Ok(())
    }

    /// Remove all range highlights.
    pub fn clear_range_highlights(&mut self, cx: &mut Context<Self>) {
        if self.range_highlights.take().is_some() {
            cx.notify();
        }
    }

    /// Carry the range highlights over to `new`, the document the update of
    /// `revision` parsed, before it replaces the current one.
    ///
    /// `append` is whether that update parsed only appended text onto the
    /// document before it. Only when no update has replaced the text since
    /// the current document was committed was that document the current one,
    /// so only then do the blocks before its last stay unchanged.
    fn reconcile_range_highlights(&mut self, new: &ParsedDocument, revision: usize, append: bool) {
        let tail_only = append && self.full_update_revision <= self.committed_revision;
        self.committed_revision = revision;
        self.landed_revision = self.landed_revision.max(revision);
        self.rendered_index = Arc::default();
        if self.range_highlights.is_some() || self.pending_reveal.is_some() {
            let remap = LeafRemap::new(&self.parsed_content.document, new, tail_only);
            self.range_highlights = self
                .range_highlights
                .take()
                .and_then(|highlights| highlights.remap(&remap))
                .map(Arc::new);
            self.pending_reveal = self
                .pending_reveal
                .take()
                .and_then(|reveal| reveal.remap(&remap));
        }
    }

    /// Tell the scrollable list which blocks an append replaced, before `new`,
    /// the appended document, replaces the current one.
    ///
    /// An append parses only the current document's last block again, so the
    /// blocks before it keep their spans and their measured heights. Without
    /// this, `render_root` resets the list whenever the block count changes,
    /// which scrolled a streaming view back to the top and measured every
    /// block again on each new block. Only a grown block count is handled
    /// here; anything else is left to that reset.
    fn splice_appended_blocks(&self, new: &ParsedDocument) {
        let old = &self.parsed_content.document.blocks;
        let (old_count, new_count) = (old.len(), new.blocks.len());
        if old_count == 0 || new_count <= old_count || self.list_state.item_count() != old_count {
            return;
        }

        // The last block is always measured again: it was parsed again, and
        // it is no longer the last one.
        let unchanged = old
            .iter()
            .zip(new.blocks.iter())
            .take(old_count - 1)
            .take_while(|(old_block, new_block)| {
                old_block
                    .span()
                    .is_some_and(|span| new_block.span() == Some(span))
            })
            .count();
        // Keeps the replaced blocks' heights as hints and the scroll offset
        // inside them, and has the next layout measure only the blocks that
        // are not measured.
        self.list_state.remeasure_items(unchanged..old_count);
        self.list_state
            .splice(old_count..old_count, new_count - old_count);
    }

    /// Save bounds and unselect if bounds changed.
    pub(super) fn update_bounds(&mut self, bounds: Bounds<Pixels>, _cx: &mut App) {
        self.bounds = bounds;
    }

    /// The index of the top-level block at `content_y`, in this view's content
    /// coordinates (the same space the base selection endpoint stores its point in).
    ///
    /// Only laid-out blocks can be located, which is enough for a selection
    /// endpoint: the user can only put one where they can see it. Returns
    /// `None` for a view that is not virtualized, where every block paints and
    /// the range is not needed.
    pub(super) fn block_ix_at(&self, content_y: Pixels) -> Option<usize> {
        if !self.scrollable {
            return None;
        }

        let origin = self.bounds.origin.y + self.scroll_offset().y;
        let count = self.list_state.item_count();
        let mut ix = self.list_state.logical_scroll_top().item_ix;
        while ix < count {
            let bounds = self.list_state.bounds_for_item(ix)?;
            if content_y < bounds.bottom() - origin {
                return Some(ix);
            }
            ix += 1;
        }

        count.checked_sub(1)
    }

    #[doc(hidden)]
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    #[doc(hidden)]
    pub fn list_state(&self) -> &ListState {
        &self.list_state
    }

    #[doc(hidden)]
    pub fn is_selecting(&self) -> bool {
        self.is_selecting
    }

    #[doc(hidden)]
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Whether this view has a view-local selection (select-all, multi-click, or override),
    /// independent of the window-level selection.
    pub(super) fn has_view_selection(&self) -> bool {
        self.select_all
            || self.multi_click_selection.is_some()
            || self.selected_text_override.is_some()
    }

    pub(super) fn stop_auto_scroll(&mut self) {
        self.auto_scroll.stop();
    }

    pub(super) fn reset_selection(&mut self) {
        self.preserve_inline_selection = false;
        self.multi_click_selection = None;
        self.selected_text_override = None;
        self.select_all = false;
        self.is_selecting = false;
        self.auto_scroll.stop();
        // Clear the inline selection state synchronously, so offscreen
        // (virtualized) views that won't repaint don't leak stale selection
        // text into a new cross-view copy.
        self.parsed_content.document.clear_selection();
    }

    fn reset_selection_and_adapter(&mut self, cx: &mut App) {
        self.reset_selection();
        self.selection_adapter.set_local_selection(false, cx);
    }

    /// Clear the current text selection.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.reset_selection_and_adapter(cx);
        cx.notify();
    }

    pub(super) fn scroll_offset(&self) -> Point<Pixels> {
        if self.scrollable {
            self.list_state.scroll_px_offset_for_scrollbar()
        } else {
            Point::default()
        }
    }

    /// Select all rendered text in this view.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.multi_click_selection = None;
        self.selected_text_override = None;
        self.select_all = true;
        self.is_selecting = false;
        self.auto_scroll.stop();
        self.selection_adapter.set_local_selection(true, cx);
        cx.notify();
    }

    pub(crate) fn set_multi_click_selection(
        &mut self,
        pos: Point<Pixels>,
        kind: TextViewMultiClickKind,
        selected_text: String,
        cx: &mut App,
    ) {
        self.preserve_inline_selection = false;
        let scroll_offset = self.scroll_offset();
        let pos = pos - self.bounds.origin - scroll_offset;
        self.multi_click_selection = Some(TextViewMultiClickSelection {
            pos,
            kind,
            line_bounds: None,
        });
        self.selected_text_override = Some(selected_text);
        self.select_all = false;
        self.is_selecting = false;
        self.auto_scroll.stop();
        self.selection_adapter.set_local_selection(true, cx);
    }

    pub(crate) fn set_multi_click_line(&mut self, bounds: Bounds<Pixels>, cx: &mut App) {
        self.set_multi_click_selection(
            bounds.center(),
            TextViewMultiClickKind::Line,
            String::new(),
            cx,
        );
        self.selected_text_override = None;
        let offset = self.bounds.origin + self.scroll_offset();
        if let Some(selection) = self.multi_click_selection.as_mut() {
            selection.line_bounds = Some(Bounds::new(bounds.origin - offset, bounds.size));
        }
    }

    pub(super) fn set_auto_scroll(&mut self, delta: Option<Pixels>, cx: &mut Context<Self>) {
        self.auto_scroll.set(delta, cx, |delta, state, cx| {
            state.list_state.scroll_by(delta);
            cx.notify();
        });
    }

    /// Return the window selection (anchor, cursor) in window coordinates if
    /// this view participates in it.
    ///
    /// Single-view fast path: when both endpoints are anchored inside one
    /// TextView, only that view participates (identical to the previous
    /// per-view behavior).
    pub(crate) fn selection_points(&self, cx: &App) -> Option<(Point<Pixels>, Point<Pixels>)> {
        if !self.selectable {
            return None;
        }
        self.selection_adapter.selection_points(cx)
    }

    pub(crate) fn has_selection(&self, cx: &App) -> bool {
        self.has_view_selection() || self.selection_points(cx).is_some()
    }

    pub(super) fn on_action_select_all(
        &mut self,
        _: &SelectAll,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.selectable {
            cx.propagate();
            return;
        }

        self.select_all(cx);
    }

    pub(crate) fn is_selectable(&self) -> bool {
        self.selectable
    }

    pub(crate) fn is_all_selected(&self) -> bool {
        self.select_all
    }

    pub(crate) fn multi_click_selection(&self) -> Option<TextViewMultiClickSelection> {
        let scroll_offset = self.scroll_offset();
        self.multi_click_selection.map(|selection| {
            let pos = selection.pos + scroll_offset + self.bounds.origin;
            let line_bounds = selection.line_bounds.map(|bounds| {
                Bounds::new(
                    bounds.origin + scroll_offset + self.bounds.origin,
                    bounds.size,
                )
            });
            TextViewMultiClickSelection {
                pos,
                line_bounds,
                ..selection
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextViewMultiClickSelection {
    pub(crate) pos: Point<Pixels>,
    pub(crate) kind: TextViewMultiClickKind,
    pub(crate) line_bounds: Option<Bounds<Pixels>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextViewMultiClickKind {
    Word,
    Paragraph,
    Line,
}

impl TextViewState {
    /// Starts a frame of the pending reveal: the line to hand to rendering,
    /// and the block a scrollable view has to scroll to first, because it is
    /// off screen or the reveal is of a whole block.
    fn reveal_frame(
        &mut self,
        now: Instant,
        window: &mut Window,
    ) -> (Option<RevealRequest>, Option<usize>) {
        if self.max_lines.is_some()
            || self
                .pending_reveal
                .as_ref()
                .is_some_and(|reveal| reveal.is_expired(now))
        {
            self.pending_reveal = None;
        }
        let Some(pending) = &self.pending_reveal else {
            return (None, None);
        };

        let block_ix = pending.block_ix(&self.parsed_content.document);
        let block_off_screen = |ix: usize| {
            let viewport = self.list_state.viewport_bounds();
            self.list_state.bounds_for_item(ix).is_none_or(|item| {
                item.bottom() <= viewport.top() || item.top() >= viewport.bottom()
            })
        };
        let reveal_block = block_ix.filter(|ix| {
            self.scrollable
                && (pending.is_block() || !pending.was_laid_out())
                && block_off_screen(*ix)
        });
        let request = pending.request();
        if pending.is_block() {
            // A block has no line to wait for.
            self.pending_reveal = None;
        } else {
            window.request_animation_frame();
        }
        (request, reveal_block)
    }
}

impl Render for TextViewState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typography = (window.text_style(), window.rem_size());
        if self
            .layout_text_style
            .as_ref()
            .is_some_and(|previous| previous != &typography)
        {
            // ListState invalidates its cached rows for width changes, but does
            // not know that an inherited font or rem change affects offscreen
            // inline metrics. Retain logical selections just as for resources.
            self.preserve_inline_selection = true;
            self.compatible_layout_update = true;
            self.invalidate_measured_heights();
        }
        self.layout_text_style = Some(typography);
        let state = cx.entity();
        let stream_fade = self.stream_fade.frame(Instant::now(), cx.reduce_motion());
        if stream_fade.is_some() && self.fade_tick.is_none() {
            self.fade_tick = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(STREAM_FADE_TICK).await;
                _ = this.update(cx, |state, cx| {
                    state.fade_tick = None;
                    cx.notify();
                });
            }));
        }
        let (reveal, reveal_block) = self.reveal_frame(cx.background_executor().now(), window);
        // Built every frame, so everything in it is shared, not copied.
        let node_cx = NodeContext {
            offset: self.parsed_content.node_cx.offset,
            link_refs: self.parsed_content.node_cx.link_refs.clone(),
            style: self.text_view_style.clone(),
            code_block_actions: self.code_block_actions.clone(),
            code_block_highlighter: self.code_block_highlighter.clone(),
            code_block_max_lines: self.code_block_max_lines,
            table_actions: self.table_actions.clone(),
            link_click_handler: self.link_click_handler.clone(),
            image_source: self.image_source.clone(),
            markdown_extensions: self.markdown_extensions.clone(),
            stream_fade,
            range_highlights: self.range_highlights.clone(),
            reveal,
        };

        let content = v_flex()
            .w_full()
            // Clamped content must keep its natural height: stretching it to
            // the capped box would hide the overflow the clamp has to measure.
            .when(self.max_lines.is_none(), |this| this.h_full())
            .map(|this| match &self.parsed_error {
                None => this.child(self.parsed_content.document.render_root(
                    if self.scrollable {
                        Some(self.list_state.clone())
                    } else {
                        None
                    },
                    &node_cx,
                    window,
                    cx,
                )),
                Some(err) => this.child(
                    v_flex()
                        .gap_1()
                        .child("Failed to parse content")
                        .child(err.to_string()),
                ),
            })
            .on_prepaint(move |bounds, window, cx| {
                let (
                    size_changed,
                    selection_involves_view,
                    has_selection_snapshot,
                    is_selecting,
                    compatible_layout_update,
                ) = {
                    let state = state.read(cx);
                    (
                        state.bounds().size != bounds.size,
                        state.selection_adapter.is_part_of_window_selection(cx),
                        state.selection_adapter.has_selection_snapshot(cx),
                        state.is_selecting,
                        state.compatible_layout_update,
                    )
                };
                let mut revision_changed = false;
                state.update(cx, |state, cx| {
                    revision_changed = state
                        .selection_adapter
                        .update_layout_revision(state.selection_revision, state.is_selecting);
                    state.update_bounds(bounds, cx);
                    state.compatible_layout_update = false;
                });
                if !is_selecting
                    && ((size_changed && selection_involves_view && !compatible_layout_update)
                        || (revision_changed && has_selection_snapshot))
                {
                    TextSelection::clear(window, cx);
                }
            });
        // After `render_root`, which resets the list when the block count
        // changed other than by an append, and with it the scroll position.
        if let Some(block_ix) = reveal_block {
            self.list_state.scroll_to_reveal_item(block_ix);
        }
        content
    }
}

#[derive(Clone, PartialEq, Default)]
pub(crate) struct ParsedContent {
    pub(crate) document: ParsedDocument,
    pub(crate) node_cx: node::NodeContext,
}

struct UpdateFuture {
    format: TextViewFormat,
    content: ParsedContent,
    rx: Pin<Box<Receiver<UpdateOptions>>>,
    tx_result: Sender<ParsedUpdate>,
}

impl UpdateFuture {
    fn new(
        format: TextViewFormat,
        rx: Receiver<UpdateOptions>,
        tx_result: Sender<ParsedUpdate>,
    ) -> Self {
        Self {
            format,
            content: Default::default(),
            rx: Box::pin(rx),
            tx_result,
        }
    }
}

impl Future for UpdateFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.rx.as_mut().poll_next(cx) {
                Poll::Ready(Some(mut options)) => {
                    let hit_coalesce_budget =
                        merge_pending_options(&mut options, self.rx.as_ref().get_ref());

                    let res = parse_content(self.format, self.content.clone(), &options);
                    if let Ok(content) = &res {
                        self.content = content.clone();
                    }
                    _ = self.tx_result.try_send(ParsedUpdate {
                        revision: options.revision,
                        full_parse: !options.append,
                        selection_compatible: options.mode == ParseMode::Compatible,
                        baseline_ack: options.mode == ParseMode::BaselineAck,
                        result: res,
                    });
                    if hit_coalesce_budget {
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                    continue;
                }
                Poll::Ready(None) => return Poll::Ready(()),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[derive(Clone)]
struct UpdateOptions {
    revision: usize,
    pending_text: String,
    append: bool,
    mode: ParseMode,
    markdown_extensions: Arc<MarkdownExtensions>,
}

impl UpdateOptions {
    fn merge(&mut self, next: UpdateOptions) {
        if next.append {
            self.pending_text.push_str(&next.pending_text);
            self.revision = next.revision;
            // The text was parsed with these when it was set, unless the
            // extensions were given before any text was.
            self.markdown_extensions = next.markdown_extensions;
            if self.mode != ParseMode::Replace {
                self.mode = ParseMode::Compatible;
            }
        } else {
            *self = next;
        }
    }
}

struct ParsedUpdate {
    revision: usize,
    full_parse: bool,
    selection_compatible: bool,
    baseline_ack: bool,
    result: Result<ParsedContent, SharedString>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParseMode {
    BaselineAck,
    Replace,
    Compatible,
}

/// Whether `new` is `old` with other whitespace before its first line or
/// after its last, which shows the same. The first line's indentation counts:
/// four spaces make it code.
///
/// Only spaces, tabs and line endings count, as in a Markdown blank line.
/// Another space, such as an ideographic or a no-break space, makes a line
/// of a paragraph.
fn differs_only_around(old: &str, new: &str) -> bool {
    const BLANK: [char; 4] = [' ', '\t', '\r', '\n'];
    let content = old.trim_matches(BLANK);
    if content.is_empty() || content != new.trim_matches(BLANK) {
        return false;
    }
    fn indentation(text: &str) -> &str {
        let leading = &text[..text.len() - text.trim_start_matches(BLANK).len()];
        leading.rsplit(['\n', '\r']).next().unwrap_or(leading)
    }
    indentation(old) == indentation(new)
}

fn merge_pending_options(options: &mut UpdateOptions, rx: &Receiver<UpdateOptions>) -> bool {
    let mut update_count = 1;

    while update_count < MAX_COALESCED_UPDATES_PER_PARSE {
        match rx.try_recv() {
            Ok(next_options) => {
                options.merge(next_options);
                update_count += 1;
            }
            Err(_) => return false,
        }
    }

    true
}

fn parse_content(
    format: TextViewFormat,
    mut content: ParsedContent,
    options: &UpdateOptions,
) -> Result<ParsedContent, SharedString> {
    let mut node_cx = NodeContext {
        markdown_extensions: options.markdown_extensions.clone(),
        ..NodeContext::default()
    };

    // Re-parse the last block together with the appended text, so a block the
    // new text continues (an unclosed list, a fenced code block) is not split
    // in two. A block without a span cannot be located in `source` — the HTML
    // parser never records spans — so it is left in place and only the
    // appended text is parsed, positioned at the end of the current source.
    let last_span = options
        .append
        .then(|| {
            content
                .document
                .blocks
                .last()
                .and_then(|block| block.span())
        })
        .flatten();

    let mut source = String::new();
    if let Some(span) = last_span {
        Arc::make_mut(&mut content.document.blocks).pop();
        node_cx.offset = span.start;
        source.push_str(&content.document.source[span.start..]);
        source.push_str(&options.pending_text);
    } else {
        if options.append {
            node_cx.offset = content.document.source.len();
        }
        source.push_str(&options.pending_text);
    }

    let new_document = match format {
        TextViewFormat::Markdown => format::markdown::parse(&source, &mut node_cx),
        TextViewFormat::Html => format::html::parse(&source, &mut node_cx),
    }?;

    if options.append {
        content.document.source =
            format!("{}{}", content.document.source, options.pending_text).into();
        Arc::make_mut(&mut content.document.blocks)
            .extend(Arc::unwrap_or_clone(new_document.blocks));
    } else {
        content.document = new_document;
    }

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{
        MarkdownNode,
        node::{BlockNode, Span},
    };
    use gpui::{Entity, TestAppContext};

    mod stream_fade {
        use std::{cell::Cell, ops::Range, rc::Rc, time::Duration};

        use gpui::{Entity, TestAppContext};

        use super::super::*;
        use crate::{motion::Easing, text::stream_fade::TextLeafKey};

        /// Long enough that a debug test cannot outrun the fade before it
        /// samples the first frame.
        const FADE: Duration = Duration::from_secs(10);

        fn fading_state(markdown: &str, cx: &mut TestAppContext) -> Entity<TextViewState> {
            cx.update(crate::init);
            let state = cx.update(|cx| {
                cx.new(|cx| {
                    TextViewState::markdown(markdown, cx).motion(
                        TextViewMotion::default()
                            .with_stream_fade(FADE)
                            .with_stream_fade_easing(Easing::Linear),
                    )
                })
            });
            cx.run_until_parked();
            state
        }

        /// The fade ranges of `key` sampled right now, or `None` when the
        /// state has nothing fading.
        fn fades(
            state: &Entity<TextViewState>,
            key: TextLeafKey,
            cx: &mut TestAppContext,
        ) -> Option<Vec<Range<usize>>> {
            state.update(cx, |state, _| {
                let frame = state.stream_fade.frame(Instant::now(), false)?;
                let fades = frame.fades(key)?;
                assert!(
                    fades.iter().all(|(_, fade_out)| *fade_out > 0.9),
                    "a fade sampled right after it starts is still transparent: {fades:?}"
                );
                Some(fades.iter().map(|(range, _)| range.clone()).collect())
            })
        }

        #[gpui::test]
        fn push_str_fades_only_the_appended_text(cx: &mut TestAppContext) {
            let state = fading_state("hello", cx);
            state.update(cx, |state, cx| state.push_str(" world", cx));
            cx.run_until_parked();

            assert_eq!(fades(&state, TextLeafKey::block(0), cx), Some(vec![5..11]));

            state.update(cx, |state, _| {
                let later = Instant::now() + FADE + Duration::from_secs(1);
                assert!(state.stream_fade.frame(later, false).is_none());
                assert!(state.stream_fade.frame(Instant::now(), false).is_none());
            });
        }

        #[gpui::test]
        fn overtaken_parse_preserves_the_remaining_chunks_fade(cx: &mut TestAppContext) {
            let state = fading_state("hello", cx);
            let parsed = super::push_and_parse(&state, " one", cx);
            state.update(cx, |state, cx| {
                state.push_str(" two", cx);
                state.commit_parsed_update(parsed, cx);
            });
            assert_eq!(fades(&state, TextLeafKey::block(0), cx), Some(vec![5..9]));

            cx.run_until_parked();
            assert_eq!(
                fades(&state, TextLeafKey::block(0), cx),
                Some(vec![5..9, 9..13])
            );
        }

        #[gpui::test]
        fn overtaken_parse_preserves_a_remaining_blocks_fade(cx: &mut TestAppContext) {
            let state = fading_state("hello", cx);
            let parsed = super::push_and_parse(&state, " one", cx);
            state.update(cx, |state, cx| {
                state.push_str("\n\nsecond", cx);
                state.commit_parsed_update(parsed, cx);
            });
            assert_eq!(fades(&state, TextLeafKey::block(0), cx), Some(vec![5..9]));

            // A third chunk arrives while the second is still uncommitted.
            state.update(cx, |state, cx| state.push_str(" third", cx));
            cx.run_until_parked();
            let ranges = fades(&state, TextLeafKey::block(11), cx).expect("new paragraph fades");
            assert_eq!(
                ranges.into_iter().flatten().collect::<Vec<_>>(),
                (0..12).collect::<Vec<_>>()
            );
        }

        #[gpui::test]
        fn set_text_extending_the_text_fades_like_push_str(cx: &mut TestAppContext) {
            let state = fading_state("hello", cx);
            state.update(cx, |state, cx| state.set_text("hello world", cx));
            cx.run_until_parked();

            assert_eq!(fades(&state, TextLeafKey::block(0), cx), Some(vec![5..11]));
        }

        #[gpui::test]
        fn set_text_replacing_the_text_shows_it_at_once(cx: &mut TestAppContext) {
            let state = fading_state("hello", cx);
            state.update(cx, |state, cx| state.push_str(" world", cx));
            cx.run_until_parked();
            assert!(fades(&state, TextLeafKey::block(0), cx).is_some());

            state.update(cx, |state, cx| state.set_text("other", cx));
            cx.run_until_parked();

            assert_eq!(fades(&state, TextLeafKey::block(0), cx), None);
        }

        #[gpui::test]
        fn completed_markup_refades_from_the_divergence(cx: &mut TestAppContext) {
            // The paragraph renders `text` (trailing space trimmed), then
            // `text **bo` literally.
            let state = fading_state("text ", cx);
            state.update(cx, |state, cx| state.push_str("**bo", cx));
            cx.run_until_parked();
            assert_eq!(fades(&state, TextLeafKey::block(0), cx), Some(vec![4..9]));

            // `text **bold**` renders `text bold`: the space keeps its fade,
            // the glyphs from byte 5 on changed and fade again as one run.
            state.update(cx, |state, cx| state.push_str("ld**", cx));
            cx.run_until_parked();
            assert_eq!(
                fades(&state, TextLeafKey::block(0), cx),
                Some(vec![4..5, 5..9])
            );
        }

        #[gpui::test]
        fn without_stagger_an_update_fades_as_one_chunk(cx: &mut TestAppContext) {
            let state = fading_state("hello", cx);
            state.update(cx, |state, cx| state.push_str(" one two three", cx));
            cx.run_until_parked();

            assert_eq!(fades(&state, TextLeafKey::block(0), cx), Some(vec![5..19]));
        }

        #[gpui::test]
        fn words_of_one_update_start_one_after_another(cx: &mut TestAppContext) {
            let stagger = Duration::from_millis(100);
            let state = fading_state("hello", cx);
            state.update(cx, |state, _| {
                state.set_motion(
                    TextViewMotion::default()
                        .with_stream_fade(FADE)
                        .with_stream_fade_stagger(stagger)
                        .with_stream_fade_easing(Easing::Linear),
                )
            });
            state.update(cx, |state, cx| state.push_str(" one two three", cx));
            cx.run_until_parked();

            state.update(cx, |state, _| {
                let frame = state
                    .stream_fade
                    .frame(Instant::now() + stagger * 3, false)
                    .expect("words still fading");
                let fades = frame.fades(TextLeafKey::block(0)).expect("paragraph fades");
                let ranges: Vec<_> = fades.iter().map(|(range, _)| range.clone()).collect();
                assert_eq!(ranges, vec![5..10, 10..14, 14..19]);
                // A later word has faded less, so it is still more transparent.
                assert!(
                    fades[0].1 < fades[1].1 && fades[1].1 < fades[2].1,
                    "{fades:?}"
                );
            });
        }

        #[gpui::test]
        fn a_new_paragraph_fades_as_a_whole(cx: &mut TestAppContext) {
            let state = fading_state("first", cx);
            state.update(cx, |state, cx| state.push_str("\n\nsecond", cx));
            cx.run_until_parked();

            assert_eq!(fades(&state, TextLeafKey::block(0), cx), None);
            assert_eq!(fades(&state, TextLeafKey::block(7), cx), Some(vec![0..6]));
        }

        #[gpui::test]
        fn code_block_text_fades_by_block(cx: &mut TestAppContext) {
            let state = fading_state("```rs\nlet", cx);
            state.update(cx, |state, cx| state.push_str(" x", cx));
            cx.run_until_parked();

            assert_eq!(fades(&state, TextLeafKey::block(0), cx), Some(vec![3..5]));
        }

        #[gpui::test]
        fn table_cells_fade_by_ordinal(cx: &mut TestAppContext) {
            let state = fading_state("| a | b |\n|---|---|\n| c | d", cx);
            state.update(cx, |state, cx| state.push_str("e |", cx));
            cx.run_until_parked();

            assert_eq!(fades(&state, TextLeafKey::table_cell(0, 2), cx), None);
            assert_eq!(
                fades(&state, TextLeafKey::table_cell(0, 3), cx),
                Some(vec![1..2])
            );
        }

        #[gpui::test]
        fn zero_duration_records_nothing(cx: &mut TestAppContext) {
            cx.update(crate::init);
            let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("hello", cx)));
            cx.run_until_parked();
            state.update(cx, |state, cx| state.push_str(" world", cx));
            cx.run_until_parked();

            state.update(cx, |state, _| {
                assert!(state.stream_fade.frame(Instant::now(), false).is_none());
            });
        }

        #[gpui::test]
        fn reduced_motion_drops_the_fade(cx: &mut TestAppContext) {
            let state = fading_state("hello", cx);
            state.update(cx, |state, cx| state.push_str(" world", cx));
            cx.run_until_parked();

            state.update(cx, |state, _| {
                assert!(state.stream_fade.frame(Instant::now(), true).is_none());
                assert!(state.stream_fade.frame(Instant::now(), false).is_none());
            });
        }

        #[gpui::test]
        fn a_fade_repaints_on_a_timer_until_nothing_fades(cx: &mut TestAppContext) {
            struct FadeRoot(Entity<TextViewState>);
            impl Render for FadeRoot {
                fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                    crate::text::TextView::new(&self.0)
                }
            }

            let state = fading_state("hello", cx);
            let (_, cx) = cx.add_window_view(|_, _| FadeRoot(state.clone()));
            let notifies = Rc::new(Cell::new(0));
            let _subscription = cx.update(|_, cx| {
                let notifies = notifies.clone();
                cx.observe(&state, move |_, _| notifies.set(notifies.get() + 1))
            });

            state.update(cx, |state, cx| state.push_str(" world", cx));
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let before = notifies.get();

            cx.executor().advance_clock(STREAM_FADE_TICK / 2);
            cx.run_until_parked();
            assert_eq!(notifies.get(), before);

            // Each tick repaints once, and that frame schedules the next tick.
            for tick in 1..=3 {
                cx.executor().advance_clock(STREAM_FADE_TICK);
                cx.run_until_parked();
                assert_eq!(notifies.get(), before + tick);
            }

            // Replacing the text drops the fade, so the ticks stop.
            state.update(cx, |state, cx| state.set_text("other", cx));
            cx.run_until_parked();
            cx.executor().advance_clock(STREAM_FADE_TICK);
            cx.run_until_parked();
            let after = notifies.get();
            cx.executor().advance_clock(STREAM_FADE_TICK * 10);
            cx.run_until_parked();
            assert_eq!(notifies.get(), after);
            assert!(state.read_with(cx, |state, _| state.fade_tick.is_none()));
        }
    }

    #[gpui::test]
    fn small_full_replace_parses_before_background_executor_runs(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let markdown = "# ready";
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown(markdown, cx)));

        state.read_with(cx, |state, _| {
            assert_eq!(state.source().as_str(), markdown);
            assert_eq!(state.parsed_content.document.blocks.len(), 1);
        });
    }

    #[gpui::test]
    fn large_markdown_and_html_full_replacements_wait_for_background_executor(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::init);
        let markdown = "# x\n\n".repeat(MAX_SYNC_FULL_REPLACE_BYTES / 5 + 1);
        let html = format!("<p>{}</p>", "x".repeat(MAX_SYNC_FULL_REPLACE_BYTES + 1));
        assert!(markdown.len() > MAX_SYNC_FULL_REPLACE_BYTES);
        assert!(html.len() > MAX_SYNC_FULL_REPLACE_BYTES);

        let (markdown_state, html_state) = cx.update(|cx| {
            (
                cx.new(|cx| TextViewState::markdown(&markdown, cx)),
                cx.new(|cx| TextViewState::html(&html, cx)),
            )
        });

        markdown_state.read_with(cx, |state, _| {
            assert_eq!(state.text.as_str(), markdown.as_str());
            assert!(state.source().as_str().is_empty());
            assert!(state.parsed_content.document.blocks.is_empty());
        });
        html_state.read_with(cx, |state, _| {
            assert_eq!(state.text.as_str(), html.as_str());
            assert!(state.source().as_str().is_empty());
            assert!(state.parsed_content.document.blocks.is_empty());
        });

        cx.run_until_parked();

        markdown_state.read_with(cx, |state, _| {
            assert_eq!(state.source().as_str(), markdown.as_str());
            assert!(!state.parsed_content.document.blocks.is_empty());
        });
        html_state.read_with(cx, |state, _| {
            assert_eq!(state.source().as_str(), html.as_str());
            assert!(!state.parsed_content.document.blocks.is_empty());
        });
    }

    #[gpui::test]
    fn inline_source_ranges_follow_streamed_tail_reparsing(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("中文 $a$\n\n尾 $x", cx)));
        state.update(cx, |state, cx| {
            let extensions = MarkdownExtensions::default().plugin(
                crate::text::markdown_ext::TestInlinePlugin::new("test").parse_with(|node, _| {
                    let markdown::mdast::Node::InlineMath(math) = node else {
                        return None;
                    };
                    Some(super::super::MarkdownNode::new("formula", ()).text(math.value.clone()))
                }),
            );
            state.set_markdown_extensions(Arc::new(extensions), cx);
        });
        cx.run_until_parked();
        state.update(cx, |state, cx| state.push_str("^2$ 后", cx));
        cx.run_until_parked();
        state.read_with(cx, |state, _| {
            let source = "中文 $a$\n\n尾 $x^2$ 后";
            assert_eq!(state.source().as_str(), source);
            let objects: Vec<_> = state
                .parsed_content
                .document
                .blocks
                .iter()
                .flat_map(|block| {
                    let super::super::node::BlockNode::Paragraph(paragraph) = block else {
                        panic!()
                    };
                    paragraph
                        .children
                        .iter()
                        .filter_map(|node| node.custom.as_ref())
                })
                .collect();
            assert_eq!(objects.len(), 2);
            for (object, markdown) in objects.iter().zip(["$a$", "$x^2$"]) {
                let start = source.find(markdown).unwrap();
                assert_eq!(object.source_range(), Some(start..start + markdown.len()));
                assert_eq!(object.as_markdown(), markdown);
            }
        });
    }

    #[gpui::test]
    fn async_full_replace_then_push_str_preserves_complete_source(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("old", cx)));
        cx.run_until_parked();

        let replacement = "x".repeat(MAX_SYNC_FULL_REPLACE_BYTES + 1);
        let expected = format!("{replacement} tail");
        state.update(cx, |state, cx| {
            state.set_text(&replacement, cx);
            state.push_str(" tail", cx);
        });
        cx.run_until_parked();

        state.read_with(cx, |state, _| {
            assert_eq!(state.text.as_str(), expected.as_str());
            assert_eq!(state.source().as_str(), expected.as_str());
        });
    }

    /// Push `chunk` and parse it the way the background parser would, without
    /// running the parser, returning the update it would send.
    fn push_and_parse(
        state: &Entity<TextViewState>,
        chunk: &str,
        cx: &mut TestAppContext,
    ) -> ParsedUpdate {
        let (revision, baseline) = state.update(cx, |state, cx| {
            state.push_str(chunk, cx);
            (state.revision, state.parsed_content.clone())
        });
        let options = UpdateOptions {
            revision,
            pending_text: chunk.to_string(),
            append: true,
            mode: ParseMode::Compatible,
            markdown_extensions: Arc::default(),
        };
        ParsedUpdate {
            revision,
            full_parse: false,
            selection_compatible: true,
            baseline_ack: false,
            result: parse_content(TextViewFormat::Markdown, baseline, &options),
        }
    }

    #[gpui::test]
    fn stream_commits_a_parse_that_a_newer_chunk_overtook(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("# Answer\n\n", cx)));
        cx.run_until_parked();

        // Chunks arriving faster than they parse always push the next chunk
        // before the previous parse lands.
        let parsed = push_and_parse(&state, "Streaming", cx);
        state.update(cx, |state, cx| {
            state.push_str(" tokens", cx);
            state.commit_parsed_update(parsed, cx);
            assert_eq!(state.source().as_str(), "# Answer\n\nStreaming");
        });

        cx.run_until_parked();
        state.read_with(cx, |state, _| {
            assert_eq!(state.source().as_str(), "# Answer\n\nStreaming tokens");
        });
    }

    #[gpui::test]
    fn a_parse_from_before_a_replacement_is_discarded(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("old", cx)));
        cx.run_until_parked();

        let parsed = push_and_parse(&state, " text", cx);
        // Large enough to parse in the background, so the replacement is not
        // committed yet when the older parse lands.
        let replacement = "x".repeat(MAX_SYNC_FULL_REPLACE_BYTES + 1);
        state.update(cx, |state, cx| {
            state.set_text(&replacement, cx);
            state.commit_parsed_update(parsed, cx);
            assert_eq!(state.source().as_str(), "old");
        });

        cx.run_until_parked();
        state.read_with(cx, |state, _| {
            assert_eq!(state.source().as_str(), replacement.as_str());
        });
    }

    #[gpui::test]
    fn html_push_str_keeps_earlier_blocks(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::html("<p>first</p>", cx)));
        cx.run_until_parked();

        state.update(cx, |state, cx| {
            state.push_str("<p>second</p>", cx);
        });
        cx.run_until_parked();

        state.read_with(cx, |state, _| {
            assert_eq!(state.source().as_str(), "<p>first</p><p>second</p>");
            let text = state
                .parsed_content
                .document
                .blocks
                .iter()
                .map(|block| block.text())
                .collect::<String>();
            assert!(text.contains("first"), "lost the first block: {text:?}");
            assert!(text.contains("second"), "lost the appended block: {text:?}");
        });
    }

    #[gpui::test]
    fn element_text_of_the_same_string_is_not_compared_again(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("", cx)));
        let revision = |cx: &mut TestAppContext| state.read_with(cx, |state, _| state.revision);
        let text = SharedString::from("hello");

        state.update(cx, |state, cx| state.set_element_text(&text, cx));
        let parsed = revision(cx);
        assert!(parsed > 0);

        // The same allocation again, and equal bytes in another allocation,
        // both leave the parsed text alone.
        state.update(cx, |state, cx| state.set_element_text(&text, cx));
        assert_eq!(revision(cx), parsed);
        let equal = SharedString::from("hello".to_string());
        state.update(cx, |state, cx| state.set_element_text(&equal, cx));
        assert_eq!(revision(cx), parsed);

        // Once the state's text moved on, the element's string is set again.
        state.update(cx, |state, cx| {
            state.push_str(" world", cx);
            state.set_element_text(&equal, cx);
        });
        state.read_with(cx, |state, _| assert_eq!(state.text.as_str(), "hello"));
    }

    #[gpui::test]
    fn set_text_then_push_str_appends_to_replaced_content(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("old", cx)));
        cx.run_until_parked();

        state.update(cx, |state, cx| {
            state.set_text("", cx);
            state.push_str("new", cx);
            state.push_str(" text", cx);
        });
        cx.run_until_parked();

        state.read_with(cx, |state, _| {
            assert_eq!(state.text.as_str(), "new text");
            assert_eq!(state.source().as_str(), "new text");
        });

        state.update(cx, |state, cx| {
            state.set_text("", cx);
        });
        cx.run_until_parked();

        state.read_with(cx, |state, _| {
            assert_eq!(state.text.as_str(), "");
            assert_eq!(state.source().as_str(), "");
        });
    }

    #[gpui::test]
    fn full_parse_coalesced_with_append_preserves_new_select_all(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("old", cx)));
        cx.run_until_parked();

        state.update(cx, |state, cx| {
            state.set_text("new", cx);
            state.push_str(" text", cx);
            state.select_all(cx);
        });
        cx.run_until_parked();

        state.read_with(cx, |state, _| {
            assert!(state.select_all);
            assert_eq!(state.selected_text().trim(), "new text");
        });
    }

    fn parsed_blocks(
        state: &Entity<TextViewState>,
        cx: &mut TestAppContext,
    ) -> Vec<(String, Option<Span>)> {
        state.read_with(cx, |state, _| {
            state
                .parsed_content
                .document
                .blocks
                .iter()
                .map(|block| (block.text(), block.span()))
                .collect()
        })
    }

    #[gpui::test]
    fn set_text_extending_markdown_appends_and_keeps_selection(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("hello", cx)));
        cx.run_until_parked();
        let revisions =
            |state: &TextViewState| (state.selection_revision, state.full_update_revision);
        let initial = state.read_with(cx, |state, _| revisions(state));

        state.update(cx, |state, cx| state.set_text("hello world", cx));
        cx.run_until_parked();
        state.read_with(cx, |state, _| {
            assert_eq!(state.source().as_str(), "hello world");
            assert_eq!(revisions(state), initial);
        });

        state.update(cx, |state, cx| state.set_text("hello", cx));
        cx.run_until_parked();
        state.read_with(cx, |state, _| {
            assert_eq!(state.source().as_str(), "hello");
            assert_ne!(state.selection_revision, initial.0);
            assert_ne!(state.full_update_revision, initial.1);
        });
    }

    #[gpui::test]
    fn set_text_streaming_markdown_matches_a_full_parse(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let streamed = cx.update(|cx| cx.new(|cx| TextViewState::markdown("# Title", cx)));
        cx.run_until_parked();

        // Chunks continue a heading, a paragraph, an inline code span, a list
        // and a fenced code block, and the text grows past the synchronous
        // parse limit.
        let filler = "word ".repeat(MAX_SYNC_FULL_REPLACE_BYTES / 5 + 1);
        let chunks = [
            "\n\nfirst para",
            "graph with `co",
            "de`\n\n- one\n",
            "- two\n\n```rust\nfn main() {",
            "}\n```\n\n",
            filler.as_str(),
            "\n\nlast",
        ];
        let full_update_revision = streamed.read_with(cx, |state, _| state.full_update_revision);
        let mut text = "# Title".to_string();
        for chunk in chunks {
            text.push_str(chunk);
            streamed.update(cx, |state, cx| state.set_text(&text, cx));
            cx.run_until_parked();
        }
        assert_eq!(
            streamed.read_with(cx, |state, _| state.full_update_revision),
            full_update_revision
        );

        let parsed = cx.update(|cx| cx.new(|cx| TextViewState::markdown(&text, cx)));
        cx.run_until_parked();

        assert_eq!(
            streamed.read_with(cx, |state, _| state.source()),
            parsed.read_with(cx, |state, _| state.source())
        );
        assert_eq!(parsed_blocks(&streamed, cx), parsed_blocks(&parsed, cx));
    }

    #[gpui::test]
    fn set_text_extending_html_parses_it_again(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let streamed = cx.update(|cx| cx.new(|cx| TextViewState::html("<ul><li>a</li>", cx)));
        cx.run_until_parked();

        let text = "<ul><li>a</li><li>b</li></ul>";
        streamed.update(cx, |state, cx| state.set_text(text, cx));
        cx.run_until_parked();
        assert_ne!(
            streamed.read_with(cx, |state, _| state.full_update_revision),
            0
        );

        let parsed = cx.update(|cx| cx.new(|cx| TextViewState::html(text, cx)));
        cx.run_until_parked();
        assert_eq!(parsed_blocks(&streamed, cx), parsed_blocks(&parsed, cx));
    }

    #[gpui::test]
    fn set_text_extending_after_a_parse_error_parses_it_again(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("hello", cx)));
        cx.run_until_parked();

        let full_update_revision = state.read_with(cx, |state, _| state.full_update_revision);

        state.update(cx, |state, cx| {
            state.parsed_error = Some("failed".into());
            state.set_text("hello world", cx);
        });
        cx.run_until_parked();

        state.read_with(cx, |state, _| {
            assert!(state.parsed_error.is_none());
            assert_ne!(state.full_update_revision, full_update_revision);
            assert_eq!(state.source().as_str(), "hello world");
        });
    }

    #[test]
    fn update_options_merge_keeps_latest_full_text() {
        let mut options = UpdateOptions {
            revision: 1,
            pending_text: "old".to_string(),
            append: true,
            mode: ParseMode::Compatible,
            markdown_extensions: Arc::default(),
        };

        options.merge(UpdateOptions {
            revision: 2,
            pending_text: "new".to_string(),
            append: false,
            mode: ParseMode::BaselineAck,
            markdown_extensions: Arc::default(),
        });
        options.merge(UpdateOptions {
            revision: 3,
            pending_text: " text".to_string(),
            append: true,
            mode: ParseMode::Compatible,
            markdown_extensions: Arc::default(),
        });

        assert_eq!(options.revision, 3);
        assert_eq!(options.pending_text, "new text");
        assert!(!options.append);
    }

    #[test]
    fn append_merged_into_async_replace_remains_a_replacement() {
        let mut options = UpdateOptions {
            revision: 1,
            pending_text: "new".to_string(),
            append: false,
            mode: ParseMode::Replace,
            markdown_extensions: Arc::default(),
        };

        options.merge(UpdateOptions {
            revision: 2,
            pending_text: " text".to_string(),
            append: true,
            mode: ParseMode::Compatible,
            markdown_extensions: Arc::default(),
        });

        assert_eq!(options.revision, 2);
        assert_eq!(options.pending_text, "new text");
        assert!(!options.append);
        assert_eq!(options.mode, ParseMode::Replace);
    }

    #[test]
    fn update_future_yields_before_coalescing_all_queued_updates() {
        let (tx, rx) = unbounded::<UpdateOptions>();
        let (tx_result, rx_result) = unbounded::<ParsedUpdate>();
        let total_updates = 128;

        for revision in 1..=total_updates {
            tx.try_send(UpdateOptions {
                revision,
                pending_text: format!("{revision}\n"),
                append: revision != 1,
                mode: if revision == 1 {
                    ParseMode::BaselineAck
                } else {
                    ParseMode::Compatible
                },
                markdown_extensions: Arc::default(),
            })
            .unwrap();
        }

        let mut future = Box::pin(UpdateFuture::new(TextViewFormat::Markdown, rx, tx_result));
        let waker = futures::task::noop_waker();
        let mut task_cx = std::task::Context::from_waker(&waker);

        assert!(matches!(
            std::future::Future::poll(future.as_mut(), &mut task_cx),
            Poll::Pending
        ));
        let parsed_update = rx_result.try_recv().expect("parse result");

        assert!(
            parsed_update.revision < total_updates,
            "single poll coalesced every queued update through revision {}",
            parsed_update.revision
        );

        assert!(matches!(
            std::future::Future::poll(future.as_mut(), &mut task_cx),
            Poll::Pending
        ));
        let parsed_update = rx_result.try_recv().expect("next parse result");
        assert_eq!(parsed_update.revision, total_updates);
    }

    #[gpui::test]
    fn select_all_returns_rendered_text(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("**quick** value", cx)));
        cx.run_until_parked();

        state.update(cx, |state, cx| {
            state.select_all(cx);
        });

        state.read_with(cx, |state, _| {
            assert!(state.has_view_selection());
            assert_eq!(state.selected_text().trim(), "quick value");
        });

        state.update(cx, |state, cx| {
            state.clear_selection(cx);
        });

        state.read_with(cx, |state, _| {
            assert!(!state.has_view_selection());
            assert_eq!(state.selected_text(), "");
        });
    }

    #[gpui::test]
    fn select_all_in_source_format_returns_source(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let markdown = "**quick** value";
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown(markdown, cx)));
        cx.run_until_parked();

        state.update(cx, |state, cx| state.select_all(cx));

        // The default (plain) mode strips the markup.
        state.read_with(cx, |state, _| {
            assert_eq!(state.selected_text().trim(), "quick value");
        });

        state.update(cx, |state, cx| {
            state.set_selection_format(SelectionFormat::Source, cx)
        });

        // Source mode yields the whole source verbatim.
        state.read_with(cx, |state, _| {
            assert_eq!(state.selected_text().trim(), markdown);
        });
    }

    #[gpui::test]
    fn selected_source_range_returns_full_markdown_source_for_select_all(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let markdown = "**quick** value";
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown(markdown, cx)));
        cx.run_until_parked();

        state.update(cx, |state, cx| state.select_all(cx));

        state.read_with(cx, |state, _| {
            assert_eq!(state.selected_source_range(), Some(0..markdown.len()));
        });
    }

    #[gpui::test]
    fn selected_source_range_returns_none_for_html(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::html("<b>quick</b>", cx)));
        cx.run_until_parked();

        state.update(cx, |state, cx| state.select_all(cx));

        state.read_with(cx, |state, _| {
            assert_eq!(state.selected_source_range(), None);
        });
    }

    #[test]
    fn selected_source_range_keeps_global_offsets_after_incremental_tail_parse() {
        let options = UpdateOptions {
            revision: 1,
            pending_text: "first\n\nsecond".to_string(),
            append: false,
            mode: ParseMode::Replace,
            markdown_extensions: Arc::default(),
        };
        let content = parse_content(TextViewFormat::Markdown, ParsedContent::default(), &options)
            .expect("initial parse");
        let content = parse_content(
            TextViewFormat::Markdown,
            content,
            &UpdateOptions {
                revision: 2,
                pending_text: "\n\n**écho**".to_string(),
                append: true,
                mode: ParseMode::Compatible,
                markdown_extensions: Arc::default(),
            },
        )
        .expect("incremental parse");
        let BlockNode::Paragraph(paragraph) = &content.document.blocks[2] else {
            panic!("expected appended paragraph");
        };
        let mut state = paragraph.state.lock().unwrap();
        state.set_text(paragraph.text().into());
        state.selection = Some((0.."écho".len()).into());
        drop(state);

        assert_eq!(content.document.selected_source_range(), Some(17..22));
    }

    #[test]
    fn streamed_ordered_list_continuation_preserves_start() {
        let initial = parse_content(
            TextViewFormat::Markdown,
            ParsedContent::default(),
            &UpdateOptions {
                revision: 1,
                pending_text: "3. three".to_string(),
                append: false,
                mode: ParseMode::Replace,
                markdown_extensions: Arc::default(),
            },
        )
        .expect("initial list parse");
        let continued = parse_content(
            TextViewFormat::Markdown,
            initial,
            &UpdateOptions {
                revision: 2,
                pending_text: "\n4. four".to_string(),
                append: true,
                mode: ParseMode::Compatible,
                markdown_extensions: Arc::default(),
            },
        )
        .expect("streamed list continuation parse");

        let BlockNode::List {
            start, children, ..
        } = &continued.document.blocks[0]
        else {
            panic!("expected streamed ordered list");
        };
        assert_eq!(*start, Some(3));
        assert_eq!(children.len(), 2);
    }

    #[gpui::test]
    fn parser_revision_reparses_same_name_inline_configuration(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("@member", cx)));
        for (revision, label) in [(1, "Alice"), (2, "Bob")] {
            let extensions = MarkdownExtensions::default()
                .parser_revision(revision)
                .plugin(
                    crate::text::markdown_ext::TestInlinePlugin::new("test").parse_with(
                        move |node, _| {
                            matches!(node, markdown::mdast::Node::Text(_))
                                .then(|| MarkdownNode::new("mention", label).text(label))
                        },
                    ),
                );
            state.update(cx, |state, cx| {
                state.set_markdown_extensions(Arc::new(extensions), cx)
            });
            cx.run_until_parked();
            state.read_with(cx, |state, _| {
                let node::BlockNode::Paragraph(paragraph) =
                    &state.parsed_content.document.blocks[0]
                else {
                    panic!()
                };
                assert_eq!(
                    paragraph.children[0].custom.as_ref().unwrap().as_text(),
                    label
                );
            });
        }
    }

    #[gpui::test]
    fn set_markdown_extensions_reparses_existing_text(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("$TSLA.US", cx)));
        cx.run_until_parked();

        let extensions = MarkdownExtensions::default().block_parser(|node, cx| {
            let markdown::mdast::Node::Paragraph(paragraph) = node else {
                return None;
            };
            let [markdown::mdast::Node::Text(text)] = paragraph.children.as_slice() else {
                return None;
            };
            let symbol = text.value.strip_prefix('$')?.to_string();
            let node_text = format!("${symbol}");

            Some(
                MarkdownNode::new("ticker", symbol)
                    .text(node_text)
                    .markdown(cx.node_source(node).unwrap_or_default()),
            )
        });

        state.update(cx, |state, cx| {
            state.set_markdown_extensions(Arc::new(extensions), cx);
        });
        cx.run_until_parked();

        state.read_with(cx, |state, _| {
            let node::BlockNode::Custom(node) = &state.parsed_content.document.blocks[0] else {
                panic!("expected custom markdown node");
            };
            assert_eq!(node.name(), "ticker");
            assert_eq!(node.data::<String>().map(String::as_str), Some("TSLA.US"));
        });
    }

    mod block_range {
        use std::{cell::Cell, rc::Rc};

        use gpui::{AppContext as _, Entity, TestAppContext};

        use crate::text::{SelectionFormat, TextBlock, TextBlockContent, TextViewState};

        fn document(markdown: &str, cx: &mut TestAppContext) -> Entity<TextViewState> {
            cx.update(crate::init);
            let document = cx.new(|cx| TextViewState::markdown(markdown, cx));
            cx.run_until_parked();
            document
        }

        fn rendered(view: &Entity<TextViewState>, cx: &mut TestAppContext) -> String {
            view.read_with(cx, |view, _| view.rendered_text().as_str().to_string())
        }

        #[gpui::test]
        fn a_view_shows_its_blocks_of_the_document(cx: &mut TestAppContext) {
            let document = document(
                "# Title\n\nfirst\n\nsecond\n\n```rust\nfn a() {}\n```\n",
                cx,
            );
            let middle = cx.new(|cx| TextViewState::block_range(&document, 1..3, cx));
            let tail = cx.new(|cx| TextViewState::block_range(&document, 3.., cx));

            assert_eq!(rendered(&middle, cx), "first\nsecond\n");
            assert!(rendered(&tail, cx).starts_with("fn a() {}"));

            let blocks = document.read_with(cx, |document, _| document.blocks());
            let contents = blocks
                .iter()
                .map(|block| block.content().clone())
                .collect::<Vec<_>>();
            assert_eq!(
                contents,
                [
                    TextBlockContent::Heading(1),
                    TextBlockContent::Paragraph,
                    TextBlockContent::Paragraph,
                    TextBlockContent::CodeBlock(Some("rust".into())),
                ]
            );
            assert_eq!(
                blocks
                    .iter()
                    .map(|block| block.is_complete())
                    .collect::<Vec<_>>(),
                [true, true, true, false],
                "the last block can still grow"
            );
            let middle_blocks = middle.read_with(cx, |middle, _| middle.blocks());
            assert_eq!(middle_blocks.len(), 2);
            assert_eq!(middle_blocks[0].id(), blocks[1].id());
            assert!(middle_blocks.iter().all(|block| block.is_complete()));
        }

        #[gpui::test]
        fn views_follow_a_streamed_document(cx: &mut TestAppContext) {
            let document = document("first paragraph", cx);
            let head = cx.new(|cx| TextViewState::block_range(&document, 0..1, cx));
            let rest = cx.new(|cx| TextViewState::block_range(&document, 1.., cx));
            let first_id = document.read_with(cx, |document, _| document.blocks()[0].id());

            document.update(cx, |document, cx| document.push_str(" grows", cx));
            cx.run_until_parked();
            assert_eq!(rendered(&head, cx), "first paragraph grows\n");
            assert_eq!(rendered(&rest, cx), "");

            document.update(cx, |document, cx| document.push_str("\n\nsecond", cx));
            cx.run_until_parked();
            assert_eq!(rendered(&head, cx), "first paragraph grows\n");
            assert_eq!(rendered(&rest, cx), "second\n");
            assert_eq!(
                document.read_with(cx, |document, _| document.blocks()[0].id()),
                first_id,
                "a block keeps its identity while text is appended"
            );

            // The head is complete now: more text leaves it alone.
            let notified = Rc::new(Cell::new(0));
            let _subscription = cx.update(|cx| {
                let notified = notified.clone();
                cx.observe(&head, move |_, _| notified.set(notified.get() + 1))
            });
            document.update(cx, |document, cx| document.push_str(" and third", cx));
            cx.run_until_parked();
            assert_eq!(rendered(&rest, cx), "second and third\n");
            assert_eq!(
                notified.get(),
                0,
                "a view of complete blocks is not touched"
            );

            // A replacement shows at once, with new identities.
            document.update(cx, |document, cx| document.set_text("replaced", cx));
            cx.run_until_parked();
            assert_eq!(rendered(&head, cx), "replaced\n");
            assert_ne!(
                document.read_with(cx, |document, _| document.blocks()[0].id()),
                first_id
            );
        }

        /// A chat feed ends a streamed reply with the reply's text trimmed.
        #[gpui::test]
        fn the_text_trimmed_keeps_the_blocks_and_the_selection(cx: &mut TestAppContext) {
            for streamed in [
                "Here is the plan.\n\n- one\n- two\n\n```rust\nfn a() {}\n```\n\nDone.\n",
                "\n\nHere is the plan.\n\n- one\n- two\n\n```rust\nfn a() {}\n```\n\nDone.",
            ] {
                let document = document("", cx);
                for chunk in streamed.as_bytes().chunks(7) {
                    let chunk = std::str::from_utf8(chunk).unwrap();
                    document.update(cx, |document, cx| document.push_str(chunk, cx));
                    cx.run_until_parked();
                }
                let blocks = document.read_with(cx, |document, _| document.blocks());
                assert_eq!(blocks.len(), 4);
                let rows = (0..blocks.len())
                    .map(|ix| cx.new(|cx| TextViewState::block_range(&document, ix..=ix, cx)))
                    .collect::<Vec<_>>();
                rows[0].update(cx, |row, cx| row.select_all(cx));
                let notified = Rc::new(Cell::new(0));
                let _subscriptions = rows
                    .iter()
                    .map(|row| {
                        let notified = notified.clone();
                        cx.update(|cx| {
                            cx.observe(row, move |_, _| notified.set(notified.get() + 1))
                        })
                    })
                    .collect::<Vec<_>>();

                document.update(cx, |document, cx| document.set_text(streamed.trim(), cx));
                cx.run_until_parked();

                let ids =
                    |blocks: Vec<TextBlock>| blocks.iter().map(TextBlock::id).collect::<Vec<_>>();
                assert_eq!(
                    ids(document.read_with(cx, |document, _| document.blocks())),
                    ids(blocks),
                    "{streamed:?}"
                );
                assert_eq!(notified.get(), 0, "{streamed:?}");
                assert_eq!(
                    rows[0].read_with(cx, |row, _| row.selected_text()),
                    "Here is the plan.\n"
                );
            }

            // Four spaces before the first line make it code.
            let document = document("    code", cx);
            let id = document.read_with(cx, |document, _| document.blocks()[0].id());
            document.update(cx, |document, cx| document.set_text("code", cx));
            cx.run_until_parked();
            let block = document.read_with(cx, |document, _| document.blocks()[0].clone());
            assert_ne!(block.id(), id);
            assert_eq!(block.content(), &TextBlockContent::Paragraph);
        }

        /// Only spaces, tabs and line breaks are blank in Markdown. A line of
        /// an ideographic or a no-break space belongs to the paragraph.
        #[gpui::test]
        fn text_set_with_other_spaces_around_it_replaces_the_text(cx: &mut TestAppContext) {
            for (old, new) in [("text", "\u{3000}\ntext"), ("text", "text\n\u{a0}")] {
                let state = document(old, cx);
                state.update(cx, |state, cx| state.set_text(new, cx));
                cx.run_until_parked();
                let fresh = document(new, cx);
                assert_eq!(rendered(&state, cx), rendered(&fresh, cx), "{new:?}");
            }

            let state = document("text\t\r\n", cx);
            let id = state.read_with(cx, |state, _| state.blocks()[0].id());
            state.update(cx, |state, cx| state.set_text("text", cx));
            cx.run_until_parked();
            assert_eq!(state.read_with(cx, |state, _| state.blocks()[0].id()), id);
        }

        /// `$SYMBOL` paragraphs parse as a `ticker` block and render as a
        /// "ticker" element.
        fn tickers() -> crate::text::MarkdownExtensions {
            crate::text::MarkdownExtensions::default()
                .block_parser(|node, cx| {
                    let markdown::mdast::Node::Paragraph(paragraph) = node else {
                        return None;
                    };
                    let [markdown::mdast::Node::Text(text)] = paragraph.children.as_slice() else {
                        return None;
                    };
                    let symbol = text.value.strip_prefix('$')?.to_string();
                    Some(
                        crate::text::MarkdownNode::new("ticker", symbol.clone())
                            .text(symbol)
                            .markdown(cx.node_source(node).unwrap_or_default()),
                    )
                })
                .block_renderer("ticker", |_, _, _| gpui::div())
        }

        struct Shown(Entity<TextViewState>);

        impl gpui::Render for Shown {
            fn render(
                &mut self,
                _: &mut gpui::Window,
                _: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                crate::text::TextView::new(&self.0)
            }
        }

        #[gpui::test]
        fn rows_use_the_markdown_extensions_of_their_document(cx: &mut TestAppContext) {
            cx.update(crate::init);
            let document =
                cx.new(|cx| TextViewState::markdown("", cx).markdown_extensions(tickers()));
            document.update(cx, |document, cx| {
                document.push_str("Prices:\n\n$TSLA.US\n\n$AAPL.US", cx)
            });
            cx.run_until_parked();
            let row = cx.new(|cx| TextViewState::block_range(&document, 1.., cx));
            let contents = |state: &Entity<TextViewState>, cx: &mut TestAppContext| {
                state.read_with(cx, |state, _| {
                    state
                        .blocks()
                        .iter()
                        .map(|block| block.content().clone())
                        .collect::<Vec<_>>()
                })
            };
            let ticker = TextBlockContent::Custom("ticker".into());
            assert_eq!(contents(&row, cx), [ticker.clone(), ticker.clone()]);

            // The row renders with the document's renderers, whatever its
            // element has: here none.
            let (_, window) = cx.add_window_view(|_, _| Shown(row.clone()));
            window.run_until_parked();
            window.update(|window, cx| {
                window.draw(cx).clear(cx);
                assert!(std::sync::Arc::ptr_eq(
                    &row.read(cx).markdown_extensions,
                    &document.read(cx).markdown_extensions
                ));
            });

            // An element without extensions leaves the document's alone.
            let (_, window) = cx.add_window_view(|_, _| Shown(document.clone()));
            window.update(|window, cx| window.draw(cx).clear(cx));
            document.update(cx, |document, cx| document.set_text("$MSFT.US", cx));
            cx.run_until_parked();
            assert_eq!(contents(&document, cx), [ticker]);
        }

        #[gpui::test]
        fn a_finished_document_has_only_complete_blocks(cx: &mut TestAppContext) {
            let document = document("one\n\ntwo\n\nthree", cx);
            let last = cx.new(|cx| TextViewState::block_range(&document, 2..3, cx));
            let complete = |state: &Entity<TextViewState>, cx: &mut TestAppContext| {
                state.read_with(cx, |state, _| {
                    state
                        .blocks()
                        .iter()
                        .map(TextBlock::is_complete)
                        .collect::<Vec<_>>()
                })
            };
            assert_eq!(complete(&document, cx), [true, true, false]);
            assert_eq!(complete(&last, cx), [false]);

            document.update(cx, |document, cx| document.finish(cx));
            cx.run_until_parked();
            assert!(document.read_with(cx, |document, _| document.is_finished()));
            assert_eq!(complete(&document, cx), [true, true, true]);
            assert_eq!(complete(&last, cx), [true]);

            // Text appended opens it again, and the view takes it.
            document.update(cx, |document, cx| document.push_str(" and four", cx));
            cx.run_until_parked();
            assert!(!document.read_with(cx, |document, _| document.is_finished()));
            assert_eq!(complete(&document, cx), [true, true, false]);
            assert_eq!(rendered(&last, cx), "three and four\n");
            assert_eq!(complete(&last, cx), [false]);
        }

        #[gpui::test]
        fn finishing_marks_an_open_code_block_complete(cx: &mut TestAppContext) {
            use crate::text::node::BlockNode;

            let document = document("Intro\n\n```rust\nfn a() {}\n", cx);
            let row = cx.new(|cx| TextViewState::block_range(&document, 1.., cx));
            let code_complete = |state: &Entity<TextViewState>, cx: &mut TestAppContext| {
                state.read_with(cx, |state, _| {
                    match state.parsed_content.document.blocks.last() {
                        Some(BlockNode::CodeBlock(code)) => code.is_complete(),
                        _ => panic!("a code block last"),
                    }
                })
            };
            assert!(!code_complete(&document, cx));
            document.update(cx, |document, cx| document.finish(cx));
            cx.run_until_parked();
            assert!(code_complete(&document, cx));
            assert!(code_complete(&row, cx), "the row takes the complete block");
        }

        /// A chat feed often ends a reply in the frame its last chunk arrives,
        /// before that chunk is parsed: the last chunk, the reply's text
        /// trimmed, and the end, in one update or in three.
        #[gpui::test]
        fn finishing_before_the_last_chunk_is_parsed_keeps_the_chunk(cx: &mut TestAppContext) {
            let answer = "Here is the plan.\n\n- one\n- two\n\nDone, and the last words.\n";
            let (head, tail) = answer.split_at(answer.len() - 11);
            let complete = |state: &Entity<TextViewState>, cx: &mut TestAppContext| {
                state.read_with(cx, |state, _| {
                    state
                        .blocks()
                        .iter()
                        .map(TextBlock::is_complete)
                        .collect::<Vec<_>>()
                })
            };
            for in_one_update in [true, false] {
                let document = document("", cx);
                for chunk in head.as_bytes().chunks(7) {
                    let chunk = std::str::from_utf8(chunk).unwrap();
                    document.update(cx, |document, cx| document.push_str(chunk, cx));
                    cx.run_until_parked();
                }
                let last = cx.new(|cx| TextViewState::block_range(&document, 2..3, cx));
                assert_eq!(rendered(&last, cx), "Done, and the l\n");

                if in_one_update {
                    document.update(cx, |document, cx| {
                        document.push_str(tail, cx);
                        document.set_text(answer.trim(), cx);
                        document.finish(cx);
                    });
                } else {
                    document.update(cx, |document, cx| document.push_str(tail, cx));
                    document.update(cx, |document, cx| document.set_text(answer.trim(), cx));
                    document.update(cx, |document, cx| document.finish(cx));
                }
                assert!(document.read_with(cx, |document, _| document.is_finished()));
                assert_eq!(
                    complete(&document, cx),
                    [true, true, false],
                    "the last block can still change until the last chunk is parsed"
                );

                cx.run_until_parked();
                assert_eq!(rendered(&last, cx), "Done, and the last words.\n");
                assert_eq!(complete(&document, cx), [true, true, true]);
                assert_eq!(complete(&last, cx), [true]);
            }
        }

        #[gpui::test]
        fn blocks_are_counted_and_listed_from_an_index(cx: &mut TestAppContext) {
            let document = document("one\n\ntwo\n\nthree\n\nfour", cx);
            let middle = cx.new(|cx| TextViewState::block_range(&document, 1..3, cx));
            let indices =
                |blocks: Vec<TextBlock>| blocks.iter().map(TextBlock::index).collect::<Vec<_>>();
            document.read_with(cx, |document, _| {
                assert_eq!(document.block_count(), 4);
                assert_eq!(indices(document.blocks_from(2)), [2, 3]);
                assert_eq!(indices(document.blocks_from(9)), [0usize; 0]);
                assert_eq!(document.blocks_from(1), document.blocks()[1..]);
            });
            middle.read_with(cx, |middle, _| {
                assert_eq!(middle.block_count(), 2);
                assert_eq!(indices(middle.blocks_from(0)), [1, 2]);
                assert_eq!(indices(middle.blocks_from(2)), [2]);
            });
        }

        /// A view parses nothing, so it has no parse tasks until it is given
        /// text of its own.
        #[gpui::test]
        fn a_view_starts_a_parser_only_for_text_of_its_own(cx: &mut TestAppContext) {
            let document = document("one\n\ntwo", cx);
            let view = cx.new(|cx| TextViewState::block_range(&document, 1.., cx));
            document.update(cx, |document, cx| document.push_str(" more", cx));
            cx.run_until_parked();
            assert_eq!(rendered(&view, cx), "two more\n");
            assert!(view.read_with(cx, |view, _| view.parser.is_none()));

            view.update(cx, |view, cx| view.push_str("own **text**", cx));
            cx.run_until_parked();
            assert!(view.read_with(cx, |view, _| view.parser.is_some()));
            assert_eq!(rendered(&view, cx), "own text\n");
            view.update(cx, |view, cx| view.push_str(" grows", cx));
            cx.run_until_parked();
            assert_eq!(rendered(&view, cx), "own text grows\n");
        }

        struct Column {
            views: Vec<Entity<TextViewState>>,
            format: SelectionFormat,
        }

        impl gpui::Render for Column {
            fn render(
                &mut self,
                _: &mut gpui::Window,
                _: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                use gpui::{ParentElement as _, Styled as _};
                // The layer takes a drag from one view to the next, as `Root` does.
                gpui::div()
                    .w(gpui::px(400.))
                    .flex()
                    .flex_col()
                    .children(
                        self.views.iter().map(|view| {
                            crate::text::TextView::new(view).selection_format(self.format)
                        }),
                    )
                    .child(crate::TextSelectionLayer)
            }
        }

        /// Drags from the start of the first view to the end of the last and
        /// copies.
        fn drag_and_copy(
            views: Vec<Entity<TextViewState>>,
            format: SelectionFormat,
            cx: &mut TestAppContext,
        ) -> String {
            use gpui::{Modifiers, MouseButton, point, px};

            let (_, cx) = cx.add_window_view(|_, _| Column {
                views: views.clone(),
                format,
            });
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let first = views[0].read_with(cx, |view, _| view.bounds());
            let last = views[views.len() - 1].read_with(cx, |view, _| view.bounds());
            let start = point(first.left() + px(1.), first.top() + px(4.));
            let end = point(last.right() - px(1.), last.bottom() - px(4.));
            cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let copied = cx.update(|window, cx| crate::TextSelection::selected_text(window, cx));
            cx.update(|window, _| window.remove_window());
            cx.run_until_parked();
            copied
        }

        #[gpui::test]
        fn rows_copy_as_one_view_of_their_document_copies(cx: &mut TestAppContext) {
            let answer =
                "Para one.\n\nPara two.\n\n- item\n\n```rust\nfn a() {}\n```\n\nPara three.";
            for format in [SelectionFormat::Source, SelectionFormat::Plain] {
                let parsed = document(answer, cx);
                let rows = (0..5)
                    .map(|ix| cx.new(|cx| TextViewState::block_range(&parsed, ix..=ix, cx)))
                    .collect::<Vec<_>>();
                let rows_copy = drag_and_copy(rows, format, cx);
                let whole = document(answer, cx);
                let one_view = drag_and_copy(vec![whole], format, cx);

                assert_eq!(rows_copy, one_view, "{format:?}");
                if format == SelectionFormat::Source {
                    assert_eq!(rows_copy, answer);
                }
            }
        }

        #[gpui::test]
        fn select_all_in_a_view_copies_its_own_blocks(cx: &mut TestAppContext) {
            let document = document("**first**\n\nsecond\n\nthird", cx);
            let middle = cx.new(|cx| {
                TextViewState::block_range(&document, 1..2, cx)
                    .selection_format(SelectionFormat::Source)
            });
            middle.update(cx, |middle, cx| middle.select_all(cx));
            middle.read_with(cx, |middle, _| {
                assert_eq!(middle.selected_text(), "second");
                assert_eq!(middle.selected_source_range(), Some(11..17));
            });
        }

        #[gpui::test]
        fn set_block_range_moves_a_view_and_set_text_detaches_it(cx: &mut TestAppContext) {
            let document = document("one\n\ntwo\n\nthree", cx);
            let view = cx.new(|cx| TextViewState::block_range(&document, 0..1, cx));
            view.update(cx, |view, cx| view.set_block_range(1..=2, cx));
            assert_eq!(rendered(&view, cx), "two\nthree\n");

            view.update(cx, |view, cx| view.set_text("own text", cx));
            cx.run_until_parked();
            document.update(cx, |document, cx| document.push_str(" more", cx));
            cx.run_until_parked();
            assert_eq!(rendered(&view, cx), "own text\n");
        }
    }

    mod range_highlights {
        use std::ops::Range;

        use gpui::{Entity, TestAppContext};

        use super::super::*;
        use crate::text::stream_fade::TextLeafKey;

        fn state(markdown: &str, cx: &mut TestAppContext) -> Entity<TextViewState> {
            cx.update(crate::init);
            let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown(markdown, cx)));
            cx.run_until_parked();
            state
        }

        fn highlight(range: Range<usize>) -> RangeHighlight {
            RangeHighlight::new(range, gpui::hsla(0.15, 1., 0.5, 0.4))
        }

        /// Highlights `ranges` of the current rendered text.
        fn set(
            state: &Entity<TextViewState>,
            ranges: impl IntoIterator<Item = Range<usize>>,
            cx: &mut TestAppContext,
        ) -> Result<(), RangeHighlightError> {
            state.update(cx, |state, cx| {
                state.set_range_highlights(ranges.into_iter().map(highlight), cx)
            })
        }

        /// The ranges leaf `key` paints, in its own byte space.
        fn painted(
            state: &Entity<TextViewState>,
            key: TextLeafKey,
            cx: &mut TestAppContext,
        ) -> Vec<Range<usize>> {
            state.read_with(cx, |state, _| {
                state.range_highlights.as_ref().map_or(Vec::new(), |frame| {
                    frame
                        .backgrounds(key)
                        .iter()
                        .map(|(range, _)| range.clone())
                        .collect()
                })
            })
        }

        fn has_highlights(state: &Entity<TextViewState>, cx: &mut TestAppContext) -> bool {
            state.read_with(cx, |state, _| state.range_highlights.is_some())
        }

        #[gpui::test]
        fn rendered_text_is_the_plain_copy_text(cx: &mut TestAppContext) {
            let state = state(
                "# Title\n\nhello **world** \\*\n\n- item\n\n| a | b |\n|---|---|\n| c | d |\n\n```\nlet x\n```",
                cx,
            );
            state.read_with(cx, |state, _| {
                assert_eq!(
                    state.rendered_text().as_str(),
                    "Title\nhello world *\nitem\na b\nc d\n\nlet x\n"
                );
            });
        }

        #[gpui::test]
        fn a_match_across_marks_paints_in_its_paragraph(cx: &mut TestAppContext) {
            let state = state("hello **world**", cx);
            set(&state, [0.."hello world".len()], cx).unwrap();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..11]);
        }

        #[gpui::test]
        fn repeated_text_maps_to_the_occurrence_addressed(cx: &mut TestAppContext) {
            let state = state("foo\n\nfoo", cx);
            let second = state.read_with(cx, |state, _| {
                state.rendered_text().as_str().rfind("foo").unwrap()
            });
            set(&state, [second..second + 3], cx).unwrap();
            assert!(painted(&state, TextLeafKey::block(0), cx).is_empty());
            assert_eq!(painted(&state, TextLeafKey::block(5), cx), [0..3]);
        }

        #[gpui::test]
        fn a_range_across_blocks_skips_the_separator(cx: &mut TestAppContext) {
            let state = state("ab\n\ncd", cx);
            // "ab\ncd\n": 1..4 is "b\nc".
            set(&state, [1..4], cx).unwrap();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [1..2]);
            assert_eq!(painted(&state, TextLeafKey::block(4), cx), [0..1]);
        }

        #[gpui::test]
        fn table_cells_and_code_blocks_are_leaves(cx: &mut TestAppContext) {
            let state = state("| a | b |\n|---|---|\n| c | d |\n\n```\nlet x\n```", cx);
            // "a b\nc d\n\nlet x\n"
            set(&state, [0..3, 6..7, 9..12], cx).unwrap();
            assert_eq!(painted(&state, TextLeafKey::table_cell(0, 0), cx), [0..1]);
            assert_eq!(painted(&state, TextLeafKey::table_cell(0, 1), cx), [0..1]);
            assert_eq!(painted(&state, TextLeafKey::table_cell(0, 3), cx), [0..1]);
            let code_start = "| a | b |\n|---|---|\n| c | d |\n\n".len();
            assert_eq!(painted(&state, TextLeafKey::block(code_start), cx), [0..3]);
        }

        #[gpui::test]
        fn invalid_ranges_reject_the_whole_set(cx: &mut TestAppContext) {
            let state = state("中文\n\nab", cx);
            // "中文\nab\n"
            set(&state, [0.."中".len()], cx).unwrap();
            let invalid = [Range { start: 6, end: 3 }, 0..1, 0..100];
            for range in invalid {
                assert_eq!(
                    set(&state, [0..3, range.clone()], cx),
                    Err(RangeHighlightError::InvalidRange(1)),
                    "{range:?}"
                );
            }
            // The rejected sets left the earlier highlight in place.
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..3]);
        }

        #[gpui::test]
        fn text_outside_every_block_is_left_unpainted(cx: &mut TestAppContext) {
            let source = "foo one\n\n<div>foo two</div>\n\n| foo | x |\n|---|---|\n\nfoo three";
            let state = state(source, cx);
            let text = state.read_with(cx, |state, _| state.rendered_text().as_str().to_string());
            // Every "foo" and " ", an empty range, and the separator after the
            // first block: the HTML block's text and the separators are not
            // any block's text, so they paint nothing, but nothing fails.
            let ranges = text
                .match_indices("foo")
                .chain(text.match_indices(' '))
                .map(|(start, found)| start..start + found.len())
                .chain([3..3, 7..8])
                .collect::<Vec<_>>();
            set(&state, ranges, cx).unwrap();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..3, 3..4]);
            let table = source.find("| foo").unwrap();
            assert_eq!(
                painted(&state, TextLeafKey::table_cell(table, 0), cx),
                [0..3]
            );
            let last = source.find("foo three").unwrap();
            assert_eq!(painted(&state, TextLeafKey::block(last), cx), [0..3, 3..4]);
        }

        #[gpui::test]
        fn a_later_highlight_paints_over_an_earlier_one(cx: &mut TestAppContext) {
            let state = state("abcdef", cx);
            set(&state, [0..6, 2..3], cx).unwrap();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..6, 2..3]);
        }

        #[gpui::test]
        fn html_text_is_unsupported(cx: &mut TestAppContext) {
            let html = cx.update(|cx| cx.new(|cx| TextViewState::html("<p>one</p>", cx)));
            cx.run_until_parked();
            html.update(cx, |html, cx| {
                assert_eq!(
                    html.set_range_highlights([highlight(0..3)], cx),
                    Err(RangeHighlightError::Unsupported)
                );
            });
        }

        #[gpui::test]
        fn inline_objects_are_skipped(cx: &mut TestAppContext) {
            let state = state("x $a$ y", cx);
            state.update(cx, |state, cx| {
                let extensions = MarkdownExtensions::default().plugin(
                    crate::text::markdown_ext::TestInlinePlugin::new("test").parse_with(
                        |node, _| {
                            let markdown::mdast::Node::InlineMath(math) = node else {
                                return None;
                            };
                            Some(
                                crate::text::MarkdownNode::new("formula", ())
                                    .text(math.value.clone()),
                            )
                        },
                    ),
                );
                state.set_markdown_extensions(Arc::new(extensions), cx);
            });
            cx.run_until_parked();
            // "x a y\n", where "a" is the formula.
            set(&state, [2..3], cx).unwrap();
            assert!(painted(&state, TextLeafKey::block(0), cx).is_empty());
            set(&state, [0..5], cx).unwrap();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..2, 3..5]);
        }

        #[gpui::test]
        fn push_str_keeps_earlier_blocks_and_clips_the_changed_tail(cx: &mut TestAppContext) {
            let state = state("first\n\na **b", cx);
            // "first\na **b\n"
            set(&state, [0..5, 6..11], cx).unwrap();

            // Closing the emphasis renders the tail as "a bc".
            state.update(cx, |state, cx| state.push_str("c**", cx));
            cx.run_until_parked();

            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..5]);
            assert_eq!(painted(&state, TextLeafKey::block(7), cx), [0..2]);
        }

        #[gpui::test]
        fn push_str_keeps_a_block_whose_text_is_unchanged(cx: &mut TestAppContext) {
            let state = state("first\n\nsecond", cx);
            set(&state, [6..12], cx).unwrap();
            // A setext underline turns the paragraph into a heading at the
            // same source start, rendering the same text.
            state.update(cx, |state, cx| state.push_str("\n===", cx));
            cx.run_until_parked();
            assert_eq!(painted(&state, TextLeafKey::block(7), cx), [0..6]);
        }

        #[gpui::test]
        fn push_str_drops_highlights_of_a_leaf_that_is_gone(cx: &mut TestAppContext) {
            let state = state("first\n\n| a |", cx);
            set(&state, [0..5, 6..11], cx).unwrap();
            // A delimiter row turns the paragraph into a table, whose text
            // lives in cells instead.
            state.update(cx, |state, cx| state.push_str("\n|---|", cx));
            cx.run_until_parked();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..5]);
            assert!(painted(&state, TextLeafKey::block(7), cx).is_empty());
            assert!(painted(&state, TextLeafKey::table_cell(7, 0), cx).is_empty());
        }

        #[gpui::test]
        fn replacing_text_drops_highlights_only_where_it_changed(cx: &mut TestAppContext) {
            let state = state("first\n\nsecond", cx);
            set(&state, [0..5, 6..12], cx).unwrap();
            state.update(cx, |state, cx| state.set_text("first\n\nchanged", cx));
            cx.run_until_parked();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..5]);
            assert!(painted(&state, TextLeafKey::block(7), cx).is_empty());

            let large = "x".repeat(MAX_SYNC_FULL_REPLACE_BYTES + 1);
            state.update(cx, |state, cx| state.set_text(&large, cx));
            cx.run_until_parked();
            assert!(!has_highlights(&state, cx));
        }

        #[gpui::test]
        fn a_highlight_follows_its_block_past_an_earlier_edit(cx: &mut TestAppContext) {
            let state = state("foo\n\nbar\n\nbar", cx);
            // "foo\nbar\nbar\n": the first "bar", in the block at source 5.
            set(&state, [4..7], cx).unwrap();
            // Deleting "foo" moves that "bar" to 0, and the second one to 5.
            state.update(cx, |state, cx| state.set_text("bar\n\nbar", cx));
            cx.run_until_parked();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..3]);
            assert!(painted(&state, TextLeafKey::block(5), cx).is_empty());

            let state = self::state("ERROR a\n\nERROR b\n\nERROR c", cx);
            set(&state, [8..15], cx).unwrap();
            state.update(cx, |state, cx| state.set_text("ERROR b\n\nERROR c", cx));
            cx.run_until_parked();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..7]);
            assert!(painted(&state, TextLeafKey::block(9), cx).is_empty());
        }

        #[gpui::test]
        fn appending_a_copy_of_the_last_block_keeps_the_highlight_on_it(cx: &mut TestAppContext) {
            let state = state("a\n\nfoo", cx);
            // "a\nfoo\n"
            set(&state, [2..5], cx).unwrap();
            state.update(cx, |state, cx| state.set_text("a\n\nfoo\n\nfoo", cx));
            cx.run_until_parked();
            assert_eq!(painted(&state, TextLeafKey::block(3), cx), [0..3]);
            assert!(painted(&state, TextLeafKey::block(8), cx).is_empty());
        }

        #[gpui::test]
        fn deleting_a_table_row_drops_highlights_in_the_rows_it_moves(cx: &mut TestAppContext) {
            let table = "| a | b |\n|---|---|\n| x | 1 |\n| x | 2 |";
            let without_middle_row = "| a | b |\n|---|---|\n| x | 2 |";
            // "a b\nx 1\nx 2\n\n": the last row's cells, then the middle
            // row's "x".
            for ranges in [vec![8..9, 10..11], vec![4..5]] {
                let state = state(table, cx);
                set(&state, [0..1].into_iter().chain(ranges), cx).unwrap();
                state.update(cx, |state, cx| state.set_text(without_middle_row, cx));
                cx.run_until_parked();
                assert_eq!(painted(&state, TextLeafKey::table_cell(0, 0), cx), [0..1]);
                for cell in 1..4 {
                    assert!(
                        painted(&state, TextLeafKey::table_cell(0, cell), cx).is_empty(),
                        "cell {cell}"
                    );
                }
            }
        }

        #[gpui::test]
        fn streaming_table_rows_keeps_the_highlights_of_earlier_rows(cx: &mut TestAppContext) {
            let state = state("| a | b |\n|---|---|\n| x | 1 |", cx);
            // "a b\nx 1\n\n"
            set(&state, [0..1, 4..5], cx).unwrap();
            state.update(cx, |state, cx| {
                state.set_text("| a | b |\n|---|---|\n| x | 1 |\n| y | 2 |", cx)
            });
            cx.run_until_parked();
            assert_eq!(painted(&state, TextLeafKey::table_cell(0, 0), cx), [0..1]);
            assert_eq!(painted(&state, TextLeafKey::table_cell(0, 2), cx), [0..1]);
        }

        #[gpui::test]
        fn an_edit_in_an_earlier_block_keeps_later_highlights(cx: &mut TestAppContext) {
            let state = state("one\n\ntwo", cx);
            set(&state, [0..3, 4..7], cx).unwrap();
            state.update(cx, |state, cx| state.set_text("one!\n\ntwo", cx));
            cx.run_until_parked();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..3]);
            assert_eq!(painted(&state, TextLeafKey::block(6), cx), [0..3]);
        }

        #[gpui::test]
        fn an_append_after_a_full_update_compares_every_block(cx: &mut TestAppContext) {
            let state = state("first\n\nsecond", cx);
            set(&state, [0..5], cx).unwrap();
            // An append result whose baseline was a full update that has not
            // been committed: its earlier blocks are not the current ones.
            let options = UpdateOptions {
                revision: 0,
                pending_text: "FIRST\n\nsecond".to_string(),
                append: false,
                mode: ParseMode::Replace,
                markdown_extensions: Arc::default(),
            };
            let new = parse_content(TextViewFormat::Markdown, ParsedContent::default(), &options)
                .unwrap()
                .document;
            state.update(cx, |state, _| {
                state.full_update_revision = state.committed_revision + 1;
                let revision = state.committed_revision + 2;
                state.reconcile_range_highlights(&new, revision, true);
            });
            assert!(!has_highlights(&state, cx));
        }

        #[gpui::test]
        fn streaming_through_set_text_keeps_highlights(cx: &mut TestAppContext) {
            let state = state("first\n\nsec", cx);
            // "first\nsec\n"
            set(&state, [0..5, 6..9], cx).unwrap();
            state.update(cx, |state, cx| state.set_text("first\n\nsecond", cx));
            cx.run_until_parked();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..5]);
            assert_eq!(painted(&state, TextLeafKey::block(7), cx), [0..3]);
        }

        #[gpui::test]
        fn a_full_parse_merged_with_an_append_compares_every_block(cx: &mut TestAppContext) {
            let state = state("", cx);
            // The small text is committed at once, and its background parse
            // merges with the append into one full parse, in which the
            // definition turns the earlier `[foo]` into the link text `foo`.
            state.update(cx, |state, cx| {
                state.set_text("[foo] and some text\n\nmore", cx);
                let text = state.rendered_text();
                let some = text.as_str().find("some").unwrap();
                state
                    .set_range_highlights([highlight(some..some + 4)], cx)
                    .unwrap();
                state.push_str("\n\n[foo]: https://example.com", cx);
            });
            cx.run_until_parked();
            state.read_with(cx, |state, _| {
                assert!(state.rendered_text().as_str().starts_with("foo and some"));
            });
            assert!(painted(&state, TextLeafKey::block(0), cx).is_empty());
        }

        #[gpui::test]
        fn a_full_update_before_an_append_drops_replaced_highlights(cx: &mut TestAppContext) {
            let state = state("first", cx);
            set(&state, [0..5], cx).unwrap();
            let large = "x".repeat(MAX_SYNC_FULL_REPLACE_BYTES + 1);
            state.update(cx, |state, cx| {
                state.set_text(&large, cx);
                state.push_str(" tail", cx);
            });
            cx.run_until_parked();
            assert!(!has_highlights(&state, cx));
        }

        #[gpui::test]
        fn reparsing_unchanged_text_keeps_highlights(cx: &mut TestAppContext) {
            let state = state("first", cx);
            set(&state, [0..5], cx).unwrap();
            state.update(cx, |state, cx| {
                state.set_markdown_extensions(
                    Arc::new(MarkdownExtensions::default().parser_revision(1)),
                    cx,
                )
            });
            cx.run_until_parked();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..5]);
        }

        pub(super) struct Root {
            pub(super) state: Entity<TextViewState>,
        }

        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                gpui::div()
                    .w(px(120.))
                    .child(crate::text::TextView::new(&self.state).selectable(true))
            }
        }

        #[gpui::test]
        fn highlights_paint_across_inline_code_tables_and_code_blocks(cx: &mut TestAppContext) {
            cx.update(crate::init);
            let markdown = "wrapping text with `inline code` and a [link](https://x.y) \
                            that wraps\n\n| a | b |\n|---|---|\n| c | d |\n\n```\nlet x = 1;\n```";
            let (root, cx) = cx.add_window_view(|_, cx| Root {
                state: cx.new(|cx| TextViewState::markdown(markdown, cx)),
            });
            cx.run_until_parked();
            let state = root.read_with(cx, |root, _| root.state.clone());
            let text = state.read_with(cx, |state, _| state.rendered_text().as_str().to_string());
            let code = text.find("code").unwrap();
            let len = text.len();
            // Part of the inline code, every character, and the whole text.
            let ranges = [code..code + 2]
                .into_iter()
                .chain(text.char_indices().map(|(ix, c)| ix..ix + c.len_utf8()))
                .chain([0..len])
                .filter(|range| !text[range.clone()].trim().is_empty());
            state.update(cx, |state, cx| {
                state
                    .set_range_highlights(ranges.map(highlight), cx)
                    .unwrap();
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(
                painted(&state, TextLeafKey::block(0), cx)[0],
                code..code + 2
            );
        }

        /// The ranges of the current rendered text converted from `sources`.
        fn converted(
            state: &Entity<TextViewState>,
            sources: impl IntoIterator<Item = Range<usize>>,
            cx: &mut TestAppContext,
        ) -> Vec<Option<Range<usize>>> {
            state.read_with(cx, |state, _| {
                let text = state.rendered_text();
                sources
                    .into_iter()
                    .map(|source| text.range_for_source(source))
                    .collect()
            })
        }

        fn find(haystack: &str, needle: &str) -> Range<usize> {
            let start = haystack.find(needle).unwrap();
            start..start + needle.len()
        }

        #[gpui::test]
        fn source_ranges_highlight_the_text_rendered_from_them(cx: &mut TestAppContext) {
            let markdown =
                "In Rust, we use `u128` to handle **larger** numbers\n\n| a | **b** |\n|---|---|";
            let state = state(markdown, cx);
            state.update(cx, |state, cx| {
                let text = state.rendered_text();
                assert_eq!(text.source(), markdown);
                let highlights = ["`u128` to handle **larger**", "**b**"]
                    .into_iter()
                    .map(|needle| text.range_for_source(find(markdown, needle)).unwrap())
                    .map(highlight)
                    .collect::<Vec<_>>();
                state.set_range_highlights(highlights, cx).unwrap();
            });
            let rendered = "In Rust, we use u128 to handle larger numbers";
            assert_eq!(
                painted(&state, TextLeafKey::block(0), cx),
                [find(rendered, "u128 to handle larger")]
            );
            let table = markdown.find("| a").unwrap();
            assert_eq!(
                painted(&state, TextLeafKey::table_cell(table, 1), cx),
                [0..1]
            );
        }

        #[gpui::test]
        fn select_all_converts_to_every_rendered_character(cx: &mut TestAppContext) {
            let markdown = "# Title\n\nhello **world**\n\n- item";
            let state = state(markdown, cx);
            state.update(cx, |state, cx| state.select_all(cx));
            state.read_with(cx, |state, _| {
                let text = state.rendered_text();
                assert_eq!(text.as_str(), "Title\nhello world\nitem\n");
                let source = state.selected_source_range().unwrap();
                // All but the separator after the last block.
                assert_eq!(text.range_for_source(source), Some(0..text.len() - 1));
            });
        }

        #[gpui::test]
        fn a_snapshot_converts_against_its_own_source(cx: &mut TestAppContext) {
            let state = state("first", cx);
            let before = state.read_with(cx, |state, _| state.rendered_text());
            state.update(cx, |state, cx| state.push_str("\n\nsecond **part**", cx));
            let source = "first\n\nsecond **part**";
            let second = find(source, "second **part**");

            // The append is parsed in the background, so until it lands the
            // view renders the source it had.
            state.read_with(cx, |state, _| {
                let text = state.rendered_text();
                assert_eq!(text, before);
                assert_eq!(text.source(), "first");
                assert_eq!(text.range_for_source(second.clone()), None);
            });
            cx.run_until_parked();

            assert_eq!(
                converted(&state, [second.clone(), 0..5], cx),
                [Some(6..17), Some(0..5)]
            );
            state.read_with(cx, |state, _| {
                let text = state.rendered_text();
                assert_ne!(text, before);
                assert_eq!(text.source(), source);
                assert_eq!(&text.as_str()[6..17], "second part");
            });
            // An earlier snapshot keeps converting against its own source.
            assert_eq!(before.source(), "first");
            assert_eq!(before.range_for_source(0..5), Some(0..5));
            assert_eq!(before.range_for_source(second), None);
        }

        #[gpui::test]
        fn appended_blocks_convert_at_their_place_in_the_whole_source(cx: &mut TestAppContext) {
            let state = state("first\n\nsecond", cx);
            // The tail parse reparses from the last block, whose positions
            // must still count from the start of the whole source.
            state.update(cx, |state, cx| state.push_str(" more\n\n**écho** end", cx));
            cx.run_until_parked();
            let source = "first\n\nsecond more\n\n**écho** end";
            let converted = converted(
                &state,
                [
                    find(source, "second more"),
                    find(source, "**écho**"),
                    find(source, "end"),
                ],
                cx,
            );
            state.read_with(cx, |state, _| {
                let text = state.rendered_text();
                assert_eq!(text.source(), source);
                let texts = converted
                    .into_iter()
                    .map(|range| &text.as_str()[range.unwrap()])
                    .collect::<Vec<_>>();
                assert_eq!(texts, ["second more", "écho", "end"]);
            });
        }

        #[gpui::test]
        fn a_background_parse_converts_once_it_lands(cx: &mut TestAppContext) {
            cx.update(crate::init);
            let markdown = format!(
                "{}\n\n**tail**",
                "word ".repeat(MAX_SYNC_FULL_REPLACE_BYTES / 4)
            );
            let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown(&markdown, cx)));
            let tail = find(&markdown, "**tail**");
            state.read_with(cx, |state, _| {
                let text = state.rendered_text();
                assert!(text.source().is_empty());
                assert!(text.is_empty());
                assert_eq!(text.range_for_source(tail.clone()), None);
            });
            cx.run_until_parked();
            state.read_with(cx, |state, _| {
                let text = state.rendered_text();
                assert_eq!(text.source(), markdown);
                let range = text.range_for_source(tail).unwrap();
                assert_eq!(&text.as_str()[range], "tail");
            });
        }

        #[gpui::test]
        fn inline_objects_convert_whole_and_stay_unpainted(cx: &mut TestAppContext) {
            let markdown = "x $ab$ y";
            let state = state(markdown, cx);
            state.update(cx, |state, cx| {
                let extensions = MarkdownExtensions::default().plugin(
                    crate::text::markdown_ext::TestInlinePlugin::new("test").parse_with(
                        |node, _| {
                            let markdown::mdast::Node::InlineMath(math) = node else {
                                return None;
                            };
                            Some(
                                crate::text::MarkdownNode::new("formula", ())
                                    .text(math.value.clone()),
                            )
                        },
                    ),
                );
                state.set_markdown_extensions(Arc::new(extensions), cx);
            });
            cx.run_until_parked();
            // "x ab y\n", where "ab" is the formula.
            let formula = find(markdown, "$ab$");
            assert_eq!(
                converted(
                    &state,
                    [
                        formula.clone(),
                        formula.start + 1..formula.start + 2,
                        0..markdown.len(),
                        find(markdown, "ab$ y"),
                    ],
                    cx
                ),
                [Some(2..4), Some(2..4), Some(0..6), Some(2..6)]
            );
            let ranges = converted(&state, [formula, 0..markdown.len()], cx);
            set(&state, [ranges[0].clone().unwrap()], cx).unwrap();
            assert!(painted(&state, TextLeafKey::block(0), cx).is_empty());
            set(&state, [ranges[1].clone().unwrap()], cx).unwrap();
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [0..2, 4..6]);
        }

        #[gpui::test]
        fn a_dragged_selection_highlights_through_its_source_range(cx: &mut TestAppContext) {
            struct SelectionRoot {
                state: Entity<TextViewState>,
            }

            impl Render for SelectionRoot {
                fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                    gpui::div()
                        .w(px(160.))
                        .child(crate::TextSelectionLayer)
                        .child(crate::text::TextView::new(&self.state))
                }
            }

            cx.update(crate::init);
            let markdown = "hello **world** and `code` &amp; more";
            let (root, cx) = cx.add_window_view(|_, cx| SelectionRoot {
                state: cx.new(|cx| TextViewState::markdown(markdown, cx)),
            });
            cx.run_until_parked();
            let state = root.read_with(cx, |root, _| root.state.clone());
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let bounds = state.read_with(cx, |state, _| state.bounds());
            assert!(bounds.size.height > px(30.), "the text wraps, {bounds:?}");
            let start = bounds.origin + gpui::point(px(2.), px(4.));
            let end = bounds.bottom_right() - gpui::point(px(2.), px(4.));
            cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.simulate_mouse_move(end, gpui::MouseButton::Left, gpui::Modifiers::default());
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
            cx.update(|window, cx| window.draw(cx).clear(cx));

            let range = state.update(cx, |state, cx| {
                let selected = state.selected_text();
                let source = state
                    .selected_source_range()
                    .expect("selection maps to source");
                let text = state.rendered_text();
                let range = text.range_for_source(source).expect("source renders text");
                assert!(selected.contains("code &"), "selected {selected:?}");
                assert_eq!(
                    &text.as_str()[range.clone()],
                    selected.trim_end_matches('\n')
                );
                state.clear_selection(cx);
                state
                    .set_range_highlights([highlight(range.clone())], cx)
                    .unwrap();
                range
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(painted(&state, TextLeafKey::block(0), cx), [range]);
        }

        #[gpui::test]
        fn html_views_convert_no_source(cx: &mut TestAppContext) {
            let html = cx.update(|cx| cx.new(|cx| TextViewState::html("<p>one</p>", cx)));
            cx.run_until_parked();
            html.read_with(cx, |html, _| {
                let text = html.rendered_text();
                assert_eq!(text.source(), "<p>one</p>");
                assert_eq!(text.range_for_source(0..10), None);
                assert_eq!(text.range_for_source(3..6), None);
            });
        }

        #[gpui::test]
        fn clear_range_highlights_removes_them(cx: &mut TestAppContext) {
            let state = state("first", cx);
            set(&state, [0..5], cx).unwrap();
            state.update(cx, |state, cx| state.clear_range_highlights(cx));
            assert!(!has_highlights(&state, cx));
        }
    }

    mod reveal_range {
        use gpui::{
            Entity, InteractiveElement as _, ListAlignment, ListState, ScrollHandle,
            StatefulInteractiveElement as _, TestAppContext, VisualTestContext, div, list,
        };

        use super::super::*;
        use crate::text::TextView;

        /// Where the view sits in a 200 × 100 window.
        #[derive(Clone)]
        enum Container {
            /// A scrollable view.
            Scrollable,
            /// A fit-content view, second row of an application list.
            List(ListState),
            /// A fit-content view in a scrolling `div`, which follows reveals
            /// through `on_reveal`.
            Div(ScrollHandle),
            /// A fit-content view clamped to two lines, second row of a list.
            Clamped(ListState),
            /// A fit-content view in nothing that scrolls.
            Fixed,
        }

        struct Root {
            state: Entity<TextViewState>,
            container: Container,
        }

        impl Render for Root {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let state = self.state.clone();
                let frame = div().w(px(200.)).h(px(100.));
                let row = |list_state: &ListState, clamp: bool| {
                    let state = state.clone();
                    list(list_state.clone(), move |ix, _, _| match ix {
                        0 => div().h(px(40.)).into_any_element(),
                        _ => TextView::new(&state)
                            .when(clamp, |view| view.max_lines(2))
                            .into_any_element(),
                    })
                    .size_full()
                };
                match &self.container {
                    Container::Scrollable => frame
                        .child(TextView::new(&state).scrollable(true))
                        .into_any_element(),
                    Container::List(list_state) => {
                        frame.child(row(list_state, false)).into_any_element()
                    }
                    Container::Clamped(list_state) => {
                        frame.child(row(list_state, true)).into_any_element()
                    }
                    Container::Div(handle) => {
                        let scroll = handle.clone();
                        frame
                            .child(
                                div()
                                    .id("scroll")
                                    .size_full()
                                    .overflow_y_scroll()
                                    .track_scroll(handle)
                                    .child(TextView::new(&state).on_reveal(move |line, _, _| {
                                        let viewport = scroll.bounds();
                                        let mut offset = scroll.offset();
                                        if line.bottom() > viewport.bottom() {
                                            offset.y -= line.bottom() - viewport.bottom();
                                        } else if line.top() < viewport.top() {
                                            offset.y += viewport.top() - line.top();
                                        }
                                        scroll.set_offset(offset);
                                    })),
                            )
                            .into_any_element()
                    }
                    Container::Fixed => frame.child(TextView::new(&state)).into_any_element(),
                }
            }
        }

        fn window<'a>(
            markdown: &str,
            container: Container,
            cx: &'a mut TestAppContext,
        ) -> (Entity<TextViewState>, &'a mut VisualTestContext) {
            cx.update(crate::init);
            let (root, cx) = cx.add_window_view(|_, cx| Root {
                state: cx.new(|cx| TextViewState::markdown(markdown, cx)),
                container,
            });
            cx.run_until_parked();
            draw(cx);
            let state = root.read_with(cx, |root, _| root.state.clone());
            (state, cx)
        }

        fn draw(cx: &mut VisualTestContext) {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }

        /// Asks to reveal the first occurrence of `needle`, then runs `then`
        /// before anything is drawn.
        fn request(
            state: &Entity<TextViewState>,
            needle: &str,
            cx: &mut VisualTestContext,
            then: impl FnOnce(&mut TextViewState, &mut Context<TextViewState>),
        ) {
            state.update(cx, |state, cx| {
                let text = state.rendered_text();
                let start = text.as_str().find(needle).unwrap();
                state.reveal_range(start..start + needle.len(), cx).unwrap();
                then(state, cx);
            });
        }

        /// Reveals the first occurrence of `needle` and draws a few frames.
        fn reveal(state: &Entity<TextViewState>, needle: &str, cx: &mut VisualTestContext) {
            request(state, needle, cx, |_, _| {});
            for _ in 0..3 {
                draw(cx);
            }
        }

        fn is_pending(state: &Entity<TextViewState>, cx: &mut VisualTestContext) -> bool {
            state.read_with(cx, |state, _| state.pending_reveal.is_some())
        }

        fn scroll_top(
            state: &Entity<TextViewState>,
            cx: &mut VisualTestContext,
        ) -> gpui::ListOffset {
            state.read_with(cx, |state, _| state.list_state.logical_scroll_top())
        }

        fn paragraphs(count: usize) -> String {
            (0..count)
                .map(|ix| format!("paragraph {ix}"))
                .collect::<Vec<_>>()
                .join("\n\n")
        }

        fn words(count: usize) -> String {
            (0..count)
                .map(|ix| format!("w{ix}"))
                .collect::<Vec<_>>()
                .join(" ")
        }

        #[gpui::test]
        fn a_scrollable_view_scrolls_to_an_offscreen_block(cx: &mut TestAppContext) {
            let (state, cx) = window(&paragraphs(200), Container::Scrollable, cx);
            reveal(&state, "paragraph 150", cx);
            assert!(!is_pending(&state, cx));
            let top = scroll_top(&state, cx);
            assert!((140..=150).contains(&top.item_ix), "{top:?}");

            reveal(&state, "paragraph 3", cx);
            assert!(!is_pending(&state, cx));
            assert!(scroll_top(&state, cx).item_ix <= 3);
        }

        #[gpui::test]
        fn an_append_adding_blocks_keeps_the_scroll_position(cx: &mut TestAppContext) {
            let (state, cx) = window(&paragraphs(200), Container::Scrollable, cx);
            reveal(&state, "paragraph 100", cx);
            let top = scroll_top(&state, cx);
            assert!(top.item_ix > 0, "{top:?}");

            state.update(cx, |state, cx| state.push_str("\n\nparagraph 200", cx));
            cx.run_until_parked();
            draw(cx);

            state.read_with(cx, |state, _| {
                assert_eq!(state.list_state.item_count(), 201);
            });
            assert_unmoved(&state, top, cx);
        }

        #[gpui::test]
        fn a_scrollable_view_scrolls_to_a_line_inside_a_long_paragraph(cx: &mut TestAppContext) {
            let (state, cx) = window(&words(400), Container::Scrollable, cx);
            reveal(&state, "w390", cx);
            assert!(!is_pending(&state, cx));
            let top = scroll_top(&state, cx);
            assert_eq!(top.item_ix, 0);
            assert!(top.offset_in_item > px(100.), "{top:?}");
        }

        #[gpui::test]
        fn revealing_a_visible_line_does_not_scroll(cx: &mut TestAppContext) {
            let (state, cx) = window(&words(400), Container::Scrollable, cx);
            reveal(&state, "w200", cx);
            let top = scroll_top(&state, cx);
            assert!(top.offset_in_item > px(0.), "{top:?}");
            for word in ["w199", "w198", "w197", "w196"] {
                reveal(&state, word, cx);
                assert!(!is_pending(&state, cx));
                let now = scroll_top(&state, cx);
                assert_eq!(
                    (now.item_ix, now.offset_in_item),
                    (top.item_ix, top.offset_in_item),
                    "{word}"
                );
            }
        }

        #[gpui::test]
        fn an_enclosing_list_scrolls_to_a_line_of_a_fit_content_view(cx: &mut TestAppContext) {
            let outer = ListState::new(2, ListAlignment::Top, px(1000.));
            let (state, cx) = window(&words(400), Container::List(outer.clone()), cx);
            reveal(&state, "w390", cx);
            assert!(!is_pending(&state, cx));
            let top = outer.logical_scroll_top();
            assert_eq!(top.item_ix, 1, "{top:?}");
            assert!(top.offset_in_item > px(100.), "{top:?}");
        }

        #[gpui::test]
        fn on_reveal_scrolls_a_container_that_ignores_scroll_requests(cx: &mut TestAppContext) {
            let handle = ScrollHandle::new();
            let (state, cx) = window(&words(400), Container::Div(handle.clone()), cx);
            reveal(&state, "w390", cx);
            assert!(!is_pending(&state, cx));
            assert!(handle.offset().y < px(-100.), "{:?}", handle.offset());
        }

        #[gpui::test]
        fn a_reveal_that_cannot_be_shown_gives_up(cx: &mut TestAppContext) {
            let (state, cx) = window(&words(4000), Container::Fixed, cx);
            reveal(&state, "w3990", cx);
            assert!(is_pending(&state, cx));
            for _ in 0..10 {
                draw(cx);
            }
            assert!(!is_pending(&state, cx));
        }

        #[gpui::test]
        fn a_reveal_not_carried_out_in_time_is_dropped(cx: &mut TestAppContext) {
            let (state, cx) = window(&paragraphs(200), Container::Scrollable, cx);
            request(&state, "paragraph 150", cx, |_, cx| {
                cx.background_executor()
                    .advance_clock(std::time::Duration::from_secs(2))
            });
            draw(cx);
            assert!(!is_pending(&state, cx));
            assert_eq!(scroll_top(&state, cx).item_ix, 0);
        }

        #[gpui::test]
        fn an_empty_range_reveals_its_line(cx: &mut TestAppContext) {
            let (state, cx) = window(&words(400), Container::Scrollable, cx);
            reveal(&state, "w390", cx);
            let top = scroll_top(&state, cx);
            // A position on a visible line leaves the view where it is.
            state.update(cx, |state, cx| {
                let text = state.rendered_text();
                let start = text.as_str().find("w391").unwrap();
                state.reveal_range(start..start, cx).unwrap();
            });
            for _ in 0..3 {
                draw(cx);
            }
            assert!(!is_pending(&state, cx));
            let now = scroll_top(&state, cx);
            assert_eq!(
                (now.item_ix, now.offset_in_item),
                (top.item_ix, top.offset_in_item)
            );
        }

        #[gpui::test]
        fn a_position_after_the_last_character_reveals_its_line(cx: &mut TestAppContext) {
            let (state, cx) = window(&words(400), Container::Scrollable, cx);
            reveal(&state, "w399", cx);
            let top = scroll_top(&state, cx);
            assert!(top.offset_in_item > px(1000.), "{top:?}");
            // The end of the text, on the visible last line.
            state.update(cx, |state, cx| {
                let text = state.rendered_text();
                let end = text.as_str().find("w399").unwrap() + "w399".len();
                state.reveal_range(end..end, cx).unwrap();
            });
            for _ in 0..3 {
                draw(cx);
            }
            assert!(!is_pending(&state, cx));
            let now = scroll_top(&state, cx);
            assert_eq!(
                (now.item_ix, now.offset_in_item),
                (top.item_ix, top.offset_in_item)
            );
        }

        /// Reveals `range` of the current text and draws a few frames.
        fn reveal_at(
            state: &Entity<TextViewState>,
            range: impl FnOnce(&str) -> std::ops::Range<usize>,
            cx: &mut VisualTestContext,
        ) {
            state.update(cx, |state, cx| {
                let text = state.rendered_text();
                let range = range(text.as_str());
                state.reveal_range(range, cx).unwrap();
            });
            for _ in 0..3 {
                draw(cx);
            }
        }

        fn assert_unmoved(
            state: &Entity<TextViewState>,
            top: gpui::ListOffset,
            cx: &mut VisualTestContext,
        ) {
            assert!(!is_pending(state, cx));
            let now = scroll_top(state, cx);
            assert_eq!(
                (now.item_ix, now.offset_in_item),
                (top.item_ix, top.offset_in_item)
            );
        }

        #[gpui::test]
        fn the_end_of_the_text_and_its_separators_reveal_the_last_line(cx: &mut TestAppContext) {
            let (state, cx) = window(&words(400), Container::Scrollable, cx);
            reveal(&state, "w399", cx);
            let top = scroll_top(&state, cx);
            assert!(top.offset_in_item > px(1000.), "{top:?}");
            // The end of the text, after the separator that ends the block.
            reveal_at(&state, |text| text.len()..text.len(), cx);
            assert_unmoved(&state, top, cx);
            // Only that separator.
            reveal_at(&state, |text| text.len() - 1..text.len(), cx);
            assert_unmoved(&state, top, cx);

            let code = (0..200)
                .map(|ix| format!("line {ix}"))
                .collect::<Vec<_>>()
                .join("\n");
            state.update(cx, |state, cx| {
                state.set_text(&format!("```\n{code}\n```"), cx)
            });
            cx.run_until_parked();
            reveal(&state, "line 199", cx);
            let top = scroll_top(&state, cx);
            assert!(top.offset_in_item > px(1000.), "{top:?}");
            reveal_at(&state, |text| text.len()..text.len(), cx);
            assert_unmoved(&state, top, cx);
        }

        #[gpui::test]
        fn the_end_of_a_block_above_reveals_its_last_line(cx: &mut TestAppContext) {
            let second = (0..400)
                .map(|ix| format!("v{ix}"))
                .collect::<Vec<_>>()
                .join(" ");
            let markdown = format!("{}\n\n{second}", words(400));
            let (state, cx) = window(&markdown, Container::Scrollable, cx);
            reveal(&state, "v399", cx);
            assert_eq!(scroll_top(&state, cx).item_ix, 1);
            // The end of the first paragraph, on the separator after it.
            reveal_at(
                &state,
                |text| {
                    let end = text.find("w399").unwrap() + "w399".len();
                    end..end
                },
                cx,
            );
            assert!(!is_pending(&state, cx));
            let top = scroll_top(&state, cx);
            assert_eq!(top.item_ix, 0, "{top:?}");
            assert!(top.offset_in_item > px(1000.), "{top:?}");
        }

        #[gpui::test]
        fn an_empty_view_has_nothing_to_reveal(cx: &mut TestAppContext) {
            cx.update(crate::init);
            let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("", cx)));
            cx.run_until_parked();
            state.update(cx, |state, cx| {
                assert_eq!(state.reveal_range(0..0, cx), Ok(()));
                assert!(state.pending_reveal.is_none());
            });
        }

        #[gpui::test]
        fn revealing_a_visible_block_does_not_scroll(cx: &mut TestAppContext) {
            let markdown = format!("{}\n\n<div>html</div>\n\n{}", words(400), words(400));
            let (state, cx) = window(&markdown, Container::Scrollable, cx);
            reveal(&state, "html", cx);
            // Still in view a little further down.
            state.update(cx, |state, _| state.list_state.scroll_by(px(30.)));
            draw(cx);
            let top = scroll_top(&state, cx);
            reveal(&state, "html", cx);
            assert_unmoved(&state, top, cx);
        }

        #[gpui::test]
        fn a_line_of_an_inline_flow_counts_as_shown_once_scrolled_to(cx: &mut TestAppContext) {
            // Inline code every tenth word, so the rows are taller than the
            // body line and land between pixels.
            let markdown = (0..400)
                .map(|ix| {
                    if ix % 10 == 0 {
                        format!("`c{ix}`")
                    } else {
                        format!("w{ix}")
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            let (state, cx) = window(&markdown, Container::Scrollable, cx);
            reveal(&state, "c390", cx);
            assert!(!is_pending(&state, cx));
            assert!(scroll_top(&state, cx).offset_in_item > px(1000.));
        }

        #[gpui::test]
        fn a_range_starting_on_a_line_break_reveals_the_next_line(cx: &mut TestAppContext) {
            let code = (0..200)
                .map(|ix| format!("line {ix}"))
                .collect::<Vec<_>>()
                .join("\n");
            let (state, cx) = window(&format!("```\n{code}\n```"), Container::Scrollable, cx);
            reveal(&state, "\nline 190", cx);
            assert!(!is_pending(&state, cx));
            let top = scroll_top(&state, cx);
            assert!(top.offset_in_item > px(1000.), "{top:?}");
        }

        #[gpui::test]
        fn a_range_starting_on_a_line_break_in_an_inline_flow_reveals_the_next_line(
            cx: &mut TestAppContext,
        ) {
            // Inline code lays the paragraph out as an inline flow.
            let lines = (0..200)
                .map(|ix| format!("line {ix} `code`"))
                .collect::<Vec<_>>()
                .join("\\\n");
            let (state, cx) = window(&lines, Container::Scrollable, cx);
            reveal(&state, "\nline 190", cx);
            assert!(!is_pending(&state, cx));
            let top = scroll_top(&state, cx);
            assert!(top.offset_in_item > px(1000.), "{top:?}");
        }

        #[gpui::test]
        fn a_reveal_is_dropped_when_its_text_before_it_changes(cx: &mut TestAppContext) {
            let (state, cx) = window(&paragraphs(200), Container::Scrollable, cx);
            let target = "first words then the target";
            let markdown = format!("{target}\n\n{}", paragraphs(200));
            state.update(cx, |state, cx| state.set_text(&markdown, cx));
            cx.run_until_parked();

            // Appending to its paragraph keeps it.
            request(&state, "target", cx, |state, cx| {
                state.set_text(&markdown.replacen("target", "target and more", 1), cx);
                assert!(state.pending_reveal.is_some());
            });
            // An edit before it in its paragraph drops it.
            request(&state, "target", cx, |state, cx| {
                state.set_text(&markdown.replacen("words", "WORDS", 1), cx);
                assert!(state.pending_reveal.is_none());
            });
        }

        #[gpui::test]
        fn a_clamped_view_does_not_reveal(cx: &mut TestAppContext) {
            let outer = ListState::new(2, ListAlignment::Top, px(1000.));
            let (state, cx) = window(&words(400), Container::Clamped(outer.clone()), cx);
            reveal(&state, "w390", cx);
            assert!(!is_pending(&state, cx));
            assert_eq!(outer.logical_scroll_top().item_ix, 0);
        }

        #[gpui::test]
        fn text_outside_every_block_reveals_its_block(cx: &mut TestAppContext) {
            let markdown = format!("{}\n\n<div>html text</div>", paragraphs(100));
            let (state, cx) = window(&markdown, Container::Scrollable, cx);
            reveal(&state, "html text", cx);
            assert!(!is_pending(&state, cx));
            assert!(
                scroll_top(&state, cx).item_ix >= 90,
                "{:?}",
                scroll_top(&state, cx)
            );
        }

        #[gpui::test]
        fn a_reveal_follows_its_text_past_an_edit_before_it(cx: &mut TestAppContext) {
            let (state, cx) = window(&paragraphs(200), Container::Scrollable, cx);
            // An inserted paragraph moves the target down one block.
            request(&state, "paragraph 150", cx, |state, cx| {
                state.set_text(&format!("inserted\n\n{}", paragraphs(200)), cx);
                assert!(state.pending_reveal.is_some());
            });
            for _ in 0..3 {
                draw(cx);
            }
            assert!(!is_pending(&state, cx));
            let top = scroll_top(&state, cx);
            assert!((141..=151).contains(&top.item_ix), "{top:?}");

            // A reveal whose text changed is dropped.
            request(&state, "paragraph 150", cx, |state, cx| {
                state.set_text(&paragraphs(200).replace("paragraph 150", "changed"), cx);
                assert!(state.pending_reveal.is_none());
            });
        }

        #[gpui::test]
        fn malformed_ranges_and_html_views_are_rejected(cx: &mut TestAppContext) {
            cx.update(crate::init);
            let state = cx.update(|cx| cx.new(|cx| TextViewState::markdown("first\n\nsecond", cx)));
            cx.run_until_parked();
            state.update(cx, |state, cx| {
                assert_eq!(
                    state.reveal_range(std::ops::Range { start: 5, end: 3 }, cx),
                    Err(RangeHighlightError::InvalidRange(0))
                );
                assert_eq!(
                    state.reveal_range(0..100, cx),
                    Err(RangeHighlightError::InvalidRange(0))
                );
                state.reveal_range(0..5, cx).unwrap();
            });

            let html = cx.update(|cx| cx.new(|cx| TextViewState::html("<p>one</p>", cx)));
            cx.run_until_parked();
            html.update(cx, |html, cx| {
                assert_eq!(
                    html.reveal_range(0..3, cx),
                    Err(RangeHighlightError::Unsupported)
                );
            });
        }
    }
}
