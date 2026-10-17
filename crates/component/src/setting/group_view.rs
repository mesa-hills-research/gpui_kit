use gpui::{
    AnyElement, AnyView, App, AvailableSpace, Context, Entity, IntoElement, ParentElement as _,
    Pixels, Render, SharedString, StyleRefinement, Styled, Window, div, size,
};

use super::{RenderOptions, SettingGroup};

/// One reactive boundary per visible keyed group. The list needs the intrinsic
/// height even though GPUI's cached view takes an explicit layout size.
pub(super) struct GroupView {
    group: SettingGroup,
    query: SharedString,
    options: RenderOptions,
    width: Pixels,
    rem: Pixels,
    height: Option<Pixels>,
}

impl GroupView {
    pub(super) fn new(
        group: SettingGroup,
        query: SharedString,
        options: RenderOptions,
        width: Pixels,
        rem: Pixels,
    ) -> Self {
        Self {
            group,
            query,
            options,
            width,
            rem,
            height: None,
        }
    }

    pub(super) fn update(
        &mut self,
        group: SettingGroup,
        query: SharedString,
        options: RenderOptions,
        width: Pixels,
        rem: Pixels,
        cx: &mut Context<Self>,
    ) {
        if self.group.key != group.key
            || self.query != query
            || self.options != options
            || self.width != width
            || self.rem != rem
        {
            self.height = None;
            cx.notify();
        }
        self.group = group;
        self.query = query;
        self.options = options;
        self.width = width;
        self.rem = rem;
    }

    pub(super) fn element(view: Entity<Self>, window: &mut Window, cx: &mut App) -> AnyElement {
        let height = if let Some(height) = view.read(cx).height {
            height
        } else {
            let width = view.read(cx).width;
            let measured = view.clone().into_any_element().layout_as_root(
                size(AvailableSpace::Definite(width), AvailableSpace::MaxContent),
                window,
                cx,
            );
            view.update(cx, |view, _| view.height = Some(measured.height));
            measured.height
        };
        AnyView::from(view)
            .cached(StyleRefinement::default().w_full().h(height))
            .into_any_element()
    }
}

impl Render for GroupView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.weak_entity();
        div()
            .w_full()
            .child(
                self.group
                    .clone()
                    .py_4()
                    .render(&self.query, &self.options, window, cx),
            )
            .on_children_prepainted(move |bounds, window, cx| {
                let Some(bounds) = bounds.first() else { return };
                let height = bounds.size.height;
                // An entity-backed field can grow without changing the app's key.
                // Keep the virtual list's next measurement in step with its content.
                let _ = view.update(cx, |view, cx| {
                    if view.height != Some(height) {
                        view.height = Some(height);
                        cx.defer_in(window, |_, _, cx| cx.notify());
                    }
                });
            })
    }
}
