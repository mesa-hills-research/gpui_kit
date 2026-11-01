//! A visual row whose offsets remain in source UTF-8 bytes.
use gpui::{App, Pixels, Point, ShapedLine, SharedString, TextAlign, Window, point, px};
use std::ops::Range;

/// One piece of a visual row: shaped text, a token's element, or ghost text.
///
/// `range` is in the row's source bytes. Text and tokens cover the bytes they
/// show; ghost text shows bytes that are not in the source, so its range is
/// empty, at the offset it is inserted at. That offset belongs in front of the
/// ghost text, which is where the caret is drawn.
///
/// Text shaped with inline text in it, such as tentative text, covers the
/// bytes of the row it shows and has `inserted`, the bytes of its shaped text
/// that the source doesn't have. Its offsets map around them, and the offset
/// where they are inserted is drawn in front of them.
pub(crate) struct InlineFragment {
    pub(crate) range: Range<usize>,
    pub(crate) x: Pixels,
    pub(crate) width: Pixels,
    pub(crate) text: Option<ShapedLine>,
    pub(crate) inserted: Option<Range<usize>>,
}

impl InlineFragment {
    /// Where byte `local` of the fragment's range is in its shaped text.
    fn display_index(&self, local: usize) -> usize {
        match &self.inserted {
            Some(inserted) if local > inserted.start => local + inserted.len(),
            _ => local,
        }
    }

    /// The byte of the fragment's range at byte `ix` of its shaped text. A
    /// byte of the inserted text is where it is inserted.
    fn source_index(&self, ix: usize) -> usize {
        match &self.inserted {
            Some(inserted) if ix > inserted.start => {
                inserted.start.max(ix.saturating_sub(inserted.len()))
            }
            _ => ix,
        }
    }
}

pub(crate) struct InputLine {
    pub(crate) len: usize,
    pub(crate) width: Pixels,
    pub(crate) text: SharedString,
    /// The byte of the row where inline text starts, if it starts in this
    /// row. The row owns that offset even at its end, since the caret is
    /// drawn in front of the inline text.
    pub(crate) inline_text_at: Option<usize>,
    content: Content,
}
// Keep the ordinary shaped row inline: boxing it would add an allocation to
// every existing plain-text row. Only token rows, and the row ghost text is
// in, allocate fragment storage.
#[allow(clippy::large_enum_variant)]
enum Content {
    Text(ShapedLine),
    Inline(Vec<InlineFragment>),
}
impl From<ShapedLine> for InputLine {
    fn from(line: ShapedLine) -> Self {
        Self {
            len: line.len,
            width: line.width,
            text: line.text.clone(),
            inline_text_at: None,
            content: Content::Text(line),
        }
    }
}
impl InputLine {
    pub(crate) fn inline(text: SharedString, fragments: Vec<InlineFragment>) -> Self {
        let width = fragments.last().map_or(px(0.), |f| f.x + f.width);
        Self {
            len: text.len(),
            text,
            width,
            inline_text_at: None,
            content: Content::Inline(fragments),
        }
    }

    /// Marks this row as the one inline text starts on, at its byte `at`.
    pub(crate) fn starts_inline_text(mut self, at: Option<usize>) -> Self {
        self.inline_text_at = at;
        self
    }

    /// The shaped text that draws byte `ix` of the row, its x in the row,
    /// and the byte `ix` is at in it.
    pub(crate) fn shaped_at(&self, ix: usize) -> Option<(&ShapedLine, Pixels, usize)> {
        match &self.content {
            Content::Text(line) => Some((line, px(0.), ix)),
            Content::Inline(fragments) => fragments.iter().find_map(|f| {
                let line = f.text.as_ref()?;
                (ix >= f.range.start && ix <= f.range.end && !f.range.is_empty())
                    .then(|| (line, f.x, f.display_index(ix - f.range.start)))
            }),
        }
    }

    /// Where the row's inline text is, from its left edge to its right.
    pub(crate) fn inserted_span(&self) -> Option<Range<Pixels>> {
        self.inserted_x(false)
    }

    /// Where the row's inline text has words, without the spaces at either
    /// end, or `None` when it has only spaces.
    pub(crate) fn inserted_words(&self) -> Option<Range<Pixels>> {
        self.inserted_x(true)
    }

    fn inserted_x(&self, words: bool) -> Option<Range<Pixels>> {
        let Content::Inline(fragments) = &self.content else {
            return None;
        };
        fragments.iter().find_map(|f| {
            let (inserted, line) = (f.inserted.as_ref()?, f.text.as_ref()?);
            let mut range = inserted.clone();
            if words {
                let text = line.text.get(range.clone())?;
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return None;
                }
                range.start += text.len() - text.trim_start().len();
                range.end = range.start + trimmed.len();
            }
            Some(f.x + line.x_for_index(range.start)..f.x + line.x_for_index(range.end))
        })
    }

    /// Where the element covering bytes from `start` is, for a token.
    pub(crate) fn element_x(&self, start: usize) -> Option<Pixels> {
        let Content::Inline(fragments) = &self.content else {
            return None;
        };
        fragments
            .iter()
            .find(|f| f.text.is_none() && f.range.start == start && !f.range.is_empty())
            .map(|f| f.x)
    }

    pub(crate) fn x_for_index(&self, ix: usize) -> Pixels {
        match &self.content {
            Content::Text(line) => line.x_for_index(ix),
            Content::Inline(fragments) => {
                for f in fragments {
                    if f.inserted.is_some() {
                        if ix >= f.range.start && ix <= f.range.end {
                            let display = f.display_index(ix - f.range.start);
                            return f.x
                                + f.text
                                    .as_ref()
                                    .map_or(px(0.), |line| line.x_for_index(display));
                        }
                        continue;
                    }
                    if f.range.is_empty() {
                        if ix == f.range.start {
                            return f.x;
                        }
                        continue;
                    }
                    if ix < f.range.end {
                        return f.x
                            + f.text.as_ref().map_or(
                                if ix <= f.range.start { px(0.) } else { f.width },
                                |line| line.x_for_index(ix.saturating_sub(f.range.start)),
                            );
                    }
                }
                self.width
            }
        }
    }
    pub(crate) fn closest_index_for_x(&self, x: Pixels) -> usize {
        match &self.content {
            Content::Text(line) => line.closest_index_for_x(x),
            Content::Inline(fragments) => {
                for f in fragments {
                    if x <= f.x + f.width {
                        if f.inserted.is_some() {
                            return f.range.start
                                + f.text.as_ref().map_or(0, |line| {
                                    f.source_index(line.closest_index_for_x(x - f.x))
                                });
                        }
                        // Ghost text has no offsets of its own: a point on it
                        // lands where it is inserted.
                        if f.range.is_empty() {
                            return f.range.start;
                        }
                        return f.text.as_ref().map_or(
                            if x - f.x < f.width / 2. {
                                f.range.start
                            } else {
                                f.range.end
                            },
                            |line| f.range.start + line.closest_index_for_x(x - f.x),
                        );
                    }
                }
                self.len
            }
        }
    }
    pub(crate) fn index_for_x(&self, x: Pixels) -> Option<usize> {
        match &self.content {
            Content::Text(line) => line.index_for_x(x),
            Content::Inline(_) => {
                (x >= px(0.) && x <= self.width).then(|| self.closest_index_for_x(x))
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(
        &self,
        pos: Point<Pixels>,
        height: Pixels,
        align: TextAlign,
        width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.paint_pass(pos, height, align, width, false, window, cx);
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_background(
        &self,
        pos: Point<Pixels>,
        height: Pixels,
        align: TextAlign,
        width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.paint_pass(pos, height, align, width, true, window, cx);
    }
    #[allow(clippy::too_many_arguments)]
    fn paint_pass(
        &self,
        pos: Point<Pixels>,
        height: Pixels,
        align: TextAlign,
        width: Option<Pixels>,
        background: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let paint = |line: &ShapedLine, pos, align, width, window: &mut Window, cx: &mut App| {
            if background {
                let _ = line.paint_background(pos, height, align, width, window, cx);
            } else {
                let _ = line.paint(pos, height, align, width, window, cx);
            }
        };
        match &self.content {
            Content::Text(line) => paint(line, pos, align, width, window, cx),
            Content::Inline(fragments) => {
                let remaining = (width.unwrap_or(self.width) - self.width).max(px(0.));
                let offset = match align {
                    TextAlign::Right => remaining,
                    TextAlign::Center => remaining / 2.,
                    _ => px(0.),
                };
                for f in fragments {
                    if let Some(line) = &f.text {
                        paint(
                            line,
                            pos + point(offset + f.x, px(0.)),
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                }
            }
        }
    }
}
