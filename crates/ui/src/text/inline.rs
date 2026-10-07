use std::{
    ops::Range,
    rc::Rc,
    sync::{Arc, Mutex},
};

use gpui::{
    App, BorderStyle, Bounds, CursorStyle, Edges, Element, ElementId, GlobalElementId, Half,
    HighlightStyle, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId, MouseUpEvent,
    Pixels, Point, SharedString, StyledText, TextLayout, Window, point, px, quad,
};

use crate::{ActiveTheme, global_state::GlobalState, input::Selection, text::node::LinkMark};

/// An inline element used to render selectable inline text.
pub(super) struct Inline {
    id: ElementId,
    text: SharedString,
    links: Rc<Vec<(Range<usize>, LinkMark)>>,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    styled_text: StyledText,
    state: Arc<Mutex<InlineState>>,
}

/// State associated with rendered inline text.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct InlineState {
    pub(super) text: SharedString,
    pub(super) selection: Option<Selection>,
}

impl InlineState {
    /// Saves the text that was actually rendered.
    pub(crate) fn set_text(&mut self, text: SharedString) {
        self.text = text;
    }
}

impl Inline {
    pub(super) fn new(
        id: impl Into<ElementId>,
        state: Arc<Mutex<InlineState>>,
        links: Vec<(Range<usize>, LinkMark)>,
        highlights: Vec<(Range<usize>, HighlightStyle)>,
    ) -> Self {
        let text = state.lock().unwrap().text.clone();

        Self {
            id: id.into(),
            links: Rc::new(links),
            highlights,
            text: text.clone(),
            styled_text: StyledText::new(text),
            state,
        }
    }

    fn link_for_position(
        layout: &TextLayout,
        links: &[(Range<usize>, LinkMark)],
        position: Point<Pixels>,
    ) -> Option<LinkMark> {
        let offset = layout.index_for_position(position).ok()?;

        links.iter().find_map(|(range, link)| {
            if range.contains(&offset) {
                Some(link.clone())
            } else {
                None
            }
        })
    }

    #[allow(unused)]
    fn paint_selected_bounds(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        window.paint_quad(gpui::PaintQuad {
            bounds,
            background: cx.theme().blue.alpha(0.01).into(),
            corner_radii: gpui::Corners::default(),
            border_color: gpui::transparent_black(),
            border_style: BorderStyle::default(),
            border_widths: gpui::Edges::all(px(0.)),
        });
    }

    fn layout_selections(
        &self,
        text_layout: &TextLayout,
        window: &mut Window,
        cx: &mut App,
    ) -> (bool, bool, Option<Selection>) {
        let Some(text_view_state) = GlobalState::global(cx).text_view_state() else {
            return (false, false, None);
        };

        let text_view_state = text_view_state.read(cx);
        let is_selectable = text_view_state.is_selectable();

        if !text_view_state.has_selection() {
            return (is_selectable, false, None);
        }

        let line_height = window.line_height();
        let selection_bounds = text_view_state.selection_bounds();
        let mut selection: Option<Selection> = None;
        let mut offset = 0;

        for character in self.text.chars() {
            let Some(position) = text_layout.position_for_index(offset) else {
                offset += character.len_utf8();
                continue;
            };

            let mut character_width = line_height.half();

            if let Some(next_position) =
                text_layout.position_for_index(offset + character.len_utf8())
            {
                if next_position.y == position.y {
                    character_width = next_position.x - position.x;
                }
            }

            if point_in_text_selection(
                position,
                character_width,
                &selection_bounds,
                line_height,
            ) {
                if selection.is_none() {
                    selection = Some((offset..offset).into());
                }

                selection.as_mut().unwrap().end = offset + character.len_utf8();
            }

            offset += character.len_utf8();
        }

        (true, true, selection)
    }

    fn paint_selection(
        selection: &Selection,
        text_layout: &TextLayout,
        bounds: &Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut start = selection.start;
        let mut end = selection.end;

        if end < start {
            std::mem::swap(&mut start, &mut end);
        }

        let Some(start_position) = text_layout.position_for_index(start) else {
            return;
        };

        let Some(end_position) = text_layout.position_for_index(end) else {
            return;
        };

        let line_height = text_layout.line_height();

        if start_position.y == end_position.y {
            window.paint_quad(quad(
                Bounds::from_corners(
                    start_position,
                    point(end_position.x, end_position.y + line_height),
                ),
                px(0.),
                cx.theme().selection,
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));

            return;
        }

        window.paint_quad(quad(
            Bounds::from_corners(
                start_position,
                point(bounds.right(), start_position.y + line_height),
            ),
            px(0.),
            cx.theme().selection,
            Edges::default(),
            gpui::transparent_black(),
            BorderStyle::default(),
        ));

        if end_position.y > start_position.y + line_height {
            window.paint_quad(quad(
                Bounds::from_corners(
                    point(bounds.left(), start_position.y + line_height),
                    point(bounds.right(), end_position.y),
                ),
                px(0.),
                cx.theme().selection,
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
        }

        window.paint_quad(quad(
            Bounds::from_corners(
                point(bounds.left(), end_position.y),
                point(end_position.x, end_position.y + line_height),
            ),
            px(0.),
            cx.theme().selection,
            Edges::default(),
            gpui::transparent_black(),
            BorderStyle::default(),
        ));
    }
}

impl IntoElement for Inline {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Inline {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_element_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let text_style = window.text_style();
        let mut runs = Vec::new();
        let mut offset = 0;

        for (range, highlight) in &self.highlights {
            if offset < range.start {
                runs.push(text_style.clone().to_run(range.start - offset));
            }

            runs.push(
                text_style
                    .clone()
                    .highlight(*highlight)
                    .to_run(range.len()),
            );

            offset = range.end;
        }

        if offset < self.text.len() {
            runs.push(text_style.to_run(self.text.len() - offset));
        }

        self.styled_text = StyledText::new(self.text.clone()).with_runs(runs);

        let (layout_id, _) =
            self.styled_text
                .request_layout(global_element_id, inspector_id, window, cx);

        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled_text
            .prepaint(id, inspector_id, bounds, &mut (), window, cx);

        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let hitbox = prepaint;
        let text_layout = self.styled_text.layout().clone();

        self.styled_text
            .paint(global_id, None, bounds, &mut (), &mut (), window, cx);

        let (is_selectable, is_selection, selection) =
            self.layout_selections(&text_layout, window, cx);

        {
            let mut state = self.state.lock().unwrap();
            state.selection = selection;

            if let Some(selection) = &state.selection {
                Self::paint_selection(selection, &text_layout, &bounds, window, cx);
            }
        }

        if is_selection || is_selectable {
            window.set_cursor_style(CursorStyle::IBeam, hitbox);
        }

        if !self.links.is_empty()
            && Self::link_for_position(&text_layout, &self.links, window.mouse_position()).is_some()
        {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        }

        if !is_selection && !self.links.is_empty() {
            window.on_mouse_event({
                let links = self.links.clone();
                let text_layout = text_layout.clone();

                move |event: &MouseUpEvent, phase, _, cx| {
                    if !bounds.contains(&event.position) || !phase.bubble() {
                        return;
                    }

                    if let Some(link) =
                        Self::link_for_position(&text_layout, &links, event.position)
                    {
                        cx.stop_propagation();
                        cx.open_url(&link.url);
                    }
                }
            });
        }
    }
}

fn point_in_text_selection(
    position: Point<Pixels>,
    character_width: Pixels,
    bounds: &Bounds<Pixels>,
    line_height: Pixels,
) -> bool {
    let top = bounds.top();
    let bottom = bounds.bottom();
    let left = bounds.left();
    let right = bounds.right();

    if position.y + line_height < top || position.y >= bottom {
        return false;
    }

    let character_center = position.x + character_width.half();
    let single_line = bottom - top <= line_height;

    if single_line {
        return character_center >= left && character_center <= right;
    }

    let is_above = position.y <= top;
    let is_below = position.y + line_height >= bottom;

    if is_above {
        character_center >= left
    } else if is_below {
        character_center <= right
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::point_in_text_selection;

    #[test]
    fn test_point_in_text_selection() {
        let line_height = px(20.);
        let character_width = px(10.);
        let bounds = Bounds {
            origin: point(px(50.), px(50.)),
            size: size(px(100.), px(100.)),
        };

        assert!(point_in_text_selection(
            point(px(50.), px(40.)),
            character_width,
            &bounds,
            line_height,
        ));

        assert!(point_in_text_selection(
            point(px(50.), px(50.)),
            character_width,
            &bounds,
            line_height,
        ));

        assert!(!point_in_text_selection(
            point(px(40.), px(50.)),
            character_width,
            &bounds,
            line_height,
        ));

        assert!(point_in_text_selection(
            point(px(160.), px(50.)),
            character_width,
            &bounds,
            line_height,
        ));

        assert!(point_in_text_selection(
            point(px(100.), px(70.)),
            character_width,
            &bounds,
            line_height,
        ));

        assert!(point_in_text_selection(
            point(px(40.), px(70.)),
            character_width,
            &bounds,
            line_height,
        ));

        assert!(point_in_text_selection(
            point(px(160.), px(70.)),
            character_width,
            &bounds,
            line_height,
        ));

        assert!(point_in_text_selection(
            point(px(100.), px(140.)),
            character_width,
            &bounds,
            line_height,
        ));

        assert!(point_in_text_selection(
            point(px(40.), px(140.)),
            character_width,
            &bounds,
            line_height,
        ));

        assert!(!point_in_text_selection(
            point(px(160.), px(140.)),
            character_width,
            &bounds,
            line_height,
        ));

        assert!(!point_in_text_selection(
            point(px(100.), px(20.)),
            character_width,
            &bounds,
            line_height,
        ));

        assert!(!point_in_text_selection(
            point(px(100.), px(160.)),
            character_width,
            &bounds,
            line_height,
        ));
    }
}