use std::ops::Range;

use gpui::{
    App, BorderStyle, Bounds, Corners, Edges, Element, ElementId, GlobalElementId, HighlightStyle,
    Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels,
    Point, SharedString, StyledText, TextLayout, TextStyleRefinement, Window, transparent_black,
};

use crate::{TextSelection, TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};

/// Plain text that participates in the window-scoped [`TextSelection`].
///
/// Use [`Self::new`] for an independent run, or [`Self::with_handle`] when
/// several elements form one selectable document.
///
/// Applications must render one [`crate::TextSelectionLayer`] above their
/// content and call [`crate::init`] during startup. Selection and copy then
/// work without depending on `gpui-component`.
pub struct SelectableText {
    id: ElementId,
    handle: Option<TextSelectionHandle>,
    text: SharedString,
    styled_text: StyledText,
    document_order: u64,
    document_position: Option<(u64, u64)>,
    text_style: Option<TextStyleRefinement>,
    selection_color: Option<Hsla>,
}

impl SelectableText {
    /// Creates a run that owns its own selection.
    pub fn new(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        Self::build(id.into(), None, text.into())
    }

    /// Creates a run that joins the document `handle` belongs to.
    pub fn with_handle(
        id: impl Into<ElementId>,
        handle: TextSelectionHandle,
        text: impl Into<SharedString>,
    ) -> Self {
        Self::build(id.into(), Some(handle), text.into())
    }

    fn build(id: ElementId, handle: Option<TextSelectionHandle>, text: SharedString) -> Self {
        Self {
            id,
            handle,
            styled_text: StyledText::new(text.clone()),
            text,
            document_order: 0,
            document_position: None,
            text_style: None,
            selection_color: None,
        }
    }

    /// Places this run in reading order among the others sharing its handle.
    pub fn document_order(mut self, order: u64) -> Self {
        self.document_order = order;
        self
    }

    /// Places this run at `position` within `document`, for a document the
    /// application shows as several elements. A run made with
    /// [`Self::with_handle`] that stops being drawn while the selection covers
    /// it stays in the selection, as described for
    /// [`crate::text::TextView::document_position`], whose documents it can
    /// share. The application keeps the handle, so the run outlives its
    /// element.
    pub fn document_position(mut self, document: u64, position: u64) -> Self {
        self.document_position = Some((document, position));
        self
    }

    /// Sets the text style the run is laid out and painted with.
    pub fn text_style(mut self, style: TextStyleRefinement) -> Self {
        self.text_style = Some(style);
        self
    }

    /// Highlights ranges without changing selection or copied text.
    ///
    /// Ranges are UTF-8 byte offsets, ordered and non-overlapping, with both
    /// endpoints on character boundaries. A later call replaces the highlights.
    pub fn highlights(
        mut self,
        highlights: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    ) -> Self {
        self.styled_text = self.styled_text.with_highlights(highlights);
        self
    }

    /// Overrides the selection background, which defaults to the theme's
    /// `colors.selection` token.
    pub fn selection_color(mut self, color: Hsla) -> Self {
        self.selection_color = Some(color);
        self
    }

    /// The layout the text is drawn with, for finding where a byte offset is
    /// drawn, and so which line holds it, without shaping the text again.
    ///
    /// The layout fills in as the element is laid out and prepainted. Read it
    /// from the `prepaint` or `paint` of an element that wraps this one, or
    /// keep a clone and read it after the frame. Its position methods panic
    /// before the first prepaint.
    pub fn layout(&self) -> &TextLayout {
        self.styled_text.layout()
    }

    fn paint_selection(
        layout: &gpui::TextLayout,
        range: Range<usize>,
        color: Hsla,
        window: &mut Window,
    ) {
        let (Some(start), Some(end)) = (
            layout.position_for_index(range.start),
            layout.position_for_index(range.end),
        ) else {
            return;
        };
        for bounds in selection_quad_bounds(start, end, layout.bounds(), layout.line_height()) {
            window.paint_quad(PaintQuad {
                bounds,
                background: color.into(),
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
                border_color: transparent_black(),
                border_style: BorderStyle::default(),
            });
        }
    }
}

fn selection_quad_bounds(
    start: Point<Pixels>,
    end: Point<Pixels>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<Bounds<Pixels>> {
    if start.y == end.y {
        return vec![Bounds::from_corners(
            start,
            Point::new(end.x, end.y + line_height),
        )];
    }

    let mut quads = vec![Bounds::from_corners(
        start,
        Point::new(bounds.right(), start.y + line_height),
    )];
    if end.y > start.y + line_height {
        quads.push(Bounds::from_corners(
            Point::new(bounds.left(), start.y + line_height),
            Point::new(bounds.right(), end.y),
        ));
    }
    quads.push(Bounds::from_corners(
        Point::new(bounds.left(), end.y),
        Point::new(end.x, end.y + line_height),
    ));
    quads
}

impl IntoElement for SelectableText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableText {
    type RequestLayoutState = TextSelectionHandle;
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let handle = self.handle.clone().unwrap_or_else(|| {
            window.with_element_state(
                global_id.expect("SelectableText must have a stable element id"),
                |retained: Option<TextSelectionHandle>, _| {
                    let handle =
                        retained.unwrap_or_else(|| TextSelectionHandle::new(self.text.clone(), cx));
                    (handle.clone(), handle)
                },
            )
        });
        let (layout_id, ()) = if let Some(style) = self.text_style.clone() {
            window.with_text_style(Some(style), |window| {
                self.styled_text
                    .request_layout(global_id, inspector_id, window, cx)
            })
        } else {
            self.styled_text
                .request_layout(global_id, inspector_id, window, cx)
        };
        (layout_id, handle)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled_text
            .prepaint(global_id, inspector_id, bounds, &mut (), window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let registration = TextSelectionRegistration::new(hitbox.clone(), bounds)
            .with_document_order(self.document_order)
            .with_text_bounds(vec![bounds])
            .with_rendered_element(handle, window, cx);
        let registration = match self.document_position {
            Some((document, position)) => registration.with_document_position(document, position),
            None => registration,
        };
        handle.register(registration, window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled_text.layout().clone();
        let selected_text_before = TextSelection::selected_text(window, cx);
        let projection = handle.update_runs(
            &[
                TextSelectionRun::new(self.text.clone(), layout.clone(), bounds)
                    .with_document_order(self.document_order),
            ],
            cx,
        );
        if selected_text_before != TextSelection::selected_text(window, cx) {
            window.refresh();
        }
        let color = self
            .selection_color
            .unwrap_or_else(|| crate::Theme::global(cx).tokens.colors.selection);
        for range in projection.ranges().iter().flatten().cloned() {
            Self::paint_selection(&layout, range, color, window);
        }
        self.styled_text.paint(
            global_id,
            inspector_id,
            bounds,
            &mut (),
            &mut (),
            window,
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use gpui::{
        Bounds, Context, IntoElement, Modifiers, MouseButton, ParentElement as _, Render,
        Styled as _, TestAppContext, Window, div, point, px, size,
    };

    use std::{cell::RefCell, rc::Rc};

    use super::SelectableText;
    use crate::{TextSelection, TextSelectionHandle, TextSelectionLayer};

    struct SelectableTextTestView;

    impl Render for SelectableTextTestView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(TextSelectionLayer).child(
                div()
                    .w(px(240.))
                    .h(px(32.))
                    .child(SelectableText::new("local", "alpha beta")),
            )
        }
    }

    #[gpui::test]
    fn explicit_handle_constructor_preserves_document_contract(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let handle = TextSelectionHandle::new("alpha beta", cx);
            let _ = SelectableText::with_handle("plain", handle, "alpha beta").document_order(42);
        });
    }

    #[gpui::test]
    fn local_handle_participates_in_window_selection(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| SelectableTextTestView);
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        cx.simulate_mouse_down(
            gpui::point(px(1.), px(12.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            gpui::point(px(220.), px(12.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            gpui::point(px(220.), px(12.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.update(|window, cx| {
            let _ = window.draw(cx);
            assert_eq!(TextSelection::selected_text(window, cx), "alpha beta");
        });
    }

    /// A right-click on selectable text asks the application's handler for
    /// its menu when it is let go, with the selection as it was, and a press
    /// on a menu that registered its bounds keeps the selection. A control
    /// that takes the press opens no menu.
    #[gpui::test]
    fn a_right_click_asks_for_the_menu_and_keeps_the_selection(cx: &mut TestAppContext) {
        let requests: Rc<RefCell<Vec<crate::TextSelectionMenuRequest>>> = Rc::default();
        cx.update(|cx| {
            crate::init(cx);
            let requests = requests.clone();
            TextSelection::on_context_menu(
                move |request, _, _| requests.borrow_mut().push(request.clone()),
                cx,
            );
        });
        let (_, cx) = cx.add_window_view(|_, _| SelectableTextTestView);
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.simulate_mouse_down(
            point(px(1.), px(12.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            point(px(220.), px(12.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            point(px(220.), px(12.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let position = point(px(20.), px(12.));
        cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::default());
        assert!(requests.borrow().is_empty(), "the menu opens on release");
        cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        let request = requests.borrow_mut().pop().expect("the menu was asked for");
        assert_eq!(request.position(), position);
        assert_eq!(request.selected_text().as_ref(), "alpha beta");
        assert!(request.has_selection());
        assert!(!request.supports_select_all());

        cx.update(|window, cx| {
            TextSelection::register_menu(Bounds::new(position, size(px(40.), px(20.))), window, cx)
        });
        cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
        cx.update(|window, cx| {
            assert_eq!(TextSelection::selected_text(window, cx), "alpha beta");
            let _ = window.draw(cx);
        });

        // A control under the pointer that takes the press.
        use gpui::InteractiveElement as _;
        struct Claimed;
        impl Render for Claimed {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().size_full().child(TextSelectionLayer).child(
                    div()
                        .w(px(240.))
                        .h(px(32.))
                        .on_mouse_down(MouseButton::Right, |_, _, cx| {
                            crate::GlobalState::suppress_text_selection(cx)
                        })
                        .child(SelectableText::new("claimed", "alpha beta")),
                )
            }
        }
        let (_, cx) = cx.add_window_view(|_, _| Claimed);
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        assert!(requests.borrow().is_empty());
    }

    /// Runs the application places in one document, with handles it keeps,
    /// keep a selection across them while the one holding its start is not
    /// drawn.
    #[gpui::test]
    fn runs_of_one_document_keep_a_selection_across_them(cx: &mut TestAppContext) {
        struct Runs {
            handles: [TextSelectionHandle; 2],
            first_shown: bool,
        }
        impl Render for Runs {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let [first, second] = self.handles.clone();
                div()
                    .size_full()
                    .child(TextSelectionLayer)
                    .children(self.first_shown.then(|| {
                        div().h(px(32.)).child(
                            SelectableText::with_handle("first", first, "alpha beta")
                                .document_position(3, 0),
                        )
                    }))
                    .child(
                        div().h(px(32.)).child(
                            SelectableText::with_handle("second", second, "gamma delta")
                                .document_position(3, 1),
                        ),
                    )
            }
        }

        cx.update(crate::init);
        let (root, cx) = cx.add_window_view(|_, cx| Runs {
            handles: [
                TextSelectionHandle::new("alpha beta", cx),
                TextSelectionHandle::new("gamma delta", cx),
            ],
            first_shown: true,
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.simulate_mouse_down(
            point(px(1.), px(12.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            point(px(300.), px(44.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            point(px(300.), px(44.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let selected = cx.update(|window, cx| TextSelection::selected_text(window, cx));
        assert!(selected.contains("alpha beta") && selected.contains("gamma delta"));

        root.update(cx, |root, cx| {
            root.first_shown = false;
            cx.notify();
        });
        for _ in 0..3 {
            cx.update(|window, cx| {
                let _ = window.draw(cx);
            });
        }
        let kept = cx.update(|window, cx| TextSelection::selected_text(window, cx));
        assert_eq!(kept, selected);
    }

    #[gpui::test]
    fn highlighted_text_keeps_unicode_selection(cx: &mut TestAppContext) {
        struct HighlightedText;
        impl Render for HighlightedText {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().size_full().child(TextSelectionLayer).child(
                    SelectableText::new("highlighted", "alpha 中文 beta").highlights([
                        (
                            0..5,
                            gpui::HighlightStyle {
                                font_weight: Some(gpui::FontWeight::BOLD),
                                ..Default::default()
                            },
                        ),
                        (
                            6..12,
                            gpui::HighlightStyle {
                                font_style: Some(gpui::FontStyle::Italic),
                                ..Default::default()
                            },
                        ),
                    ]),
                )
            }
        }
        let (_, cx) = cx.add_window_view(|_, _| HighlightedText);
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.simulate_mouse_down(
            point(px(0.), px(12.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            point(px(300.), px(12.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            point(px(300.), px(12.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.update(|window, cx| {
            let _ = window.draw(cx);
            assert_eq!(TextSelection::selected_text(window, cx), "alpha 中文 beta");
        });
    }

    #[gpui::test]
    fn layout_finds_the_line_of_an_offset_in_wrapped_text(cx: &mut TestAppContext) {
        struct Wrapped(Rc<RefCell<Option<gpui::TextLayout>>>);
        impl Render for Wrapped {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let text = SelectableText::new("wrapped", "alpha beta gamma delta");
                *self.0.borrow_mut() = Some(text.layout().clone());
                div()
                    .size_full()
                    .child(TextSelectionLayer)
                    .child(div().w(px(60.)).child(text))
            }
        }

        let layout = Rc::new(RefCell::new(None));
        let (_, cx) = cx.add_window_view({
            let layout = layout.clone();
            move |_, _| Wrapped(layout)
        });
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let layout = layout.borrow().clone().unwrap();
        let first = layout.position_for_index(0).unwrap();
        let delta = layout
            .position_for_index("alpha beta gamma ".len())
            .unwrap();
        assert!(
            delta.y >= first.y + layout.line_height(),
            "\"delta\" wraps below the first line, at {delta:?}"
        );
    }

    #[test]
    fn wrapped_selection_paints_full_width_middle_lines() {
        let bounds = Bounds::new(point(px(10.), px(20.)), size(px(100.), px(100.)));
        let quads = super::selection_quad_bounds(
            point(px(40.), px(20.)),
            point(px(30.), px(80.)),
            bounds,
            px(20.),
        );

        assert_eq!(
            quads,
            vec![
                Bounds::from_corners(point(px(40.), px(20.)), point(px(110.), px(40.))),
                Bounds::from_corners(point(px(10.), px(40.)), point(px(110.), px(80.))),
                Bounds::from_corners(point(px(10.), px(80.)), point(px(30.), px(100.))),
            ]
        );
    }
}
