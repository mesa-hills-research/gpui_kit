use super::InlineTokenContext;
use crate::{
    ActiveTheme as _, Icon, StyledExt as _,
    tooltip::{ManagedTooltipExt as _, Tooltip},
};
use gpui::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, div, prelude::FluentBuilder as _,
};

/// The element an [`InlineToken`](super::InlineToken) renders as by default. Editing
/// and activation belong to the input.
#[derive(IntoElement)]
pub struct InputToken {
    context: InlineTokenContext,
    icon: Option<Icon>,
    tooltip: Option<SharedString>,
    style: StyleRefinement,
}
impl InputToken {
    pub fn new(context: &InlineTokenContext) -> Self {
        Self {
            context: context.clone(),
            icon: None,
            tooltip: None,
            style: Default::default(),
        }
    }
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Shows extra context on hover, such as a reference's full path.
    /// Disabled tokens do not show tooltips. Readonly tokens do.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }
}
impl Styled for InputToken {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl RenderOnce for InputToken {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .id("input-token")
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .h(self.context.line_height())
            .max_w(self.context.available_width())
            .rounded(cx.theme().radius)
            .border_1()
            .map(|this| {
                // The selection color is translucent as a fill; its opaque form
                // is the matching border.
                if self.context.is_selected() {
                    this.bg(cx.theme().selection)
                        .border_color(cx.theme().selection.alpha(1.))
                } else {
                    this.bg(cx.theme().muted).border_color(cx.theme().border)
                }
            })
            .text_color(cx.theme().foreground)
            .when(self.context.is_disabled(), |this| this.opacity(0.5))
            .when_some(self.icon, |this, icon| {
                this.child(icon.size_3().flex_shrink_0())
            })
            .child(
                div()
                    .min_w_0()
                    .text_ellipsis()
                    .child(self.context.token().label().clone()),
            )
            .refine_style(&self.style)
            .when_some(
                self.tooltip.filter(|_| !self.context.is_disabled()),
                |this, tooltip| {
                    this.managed_tooltip(move |window, cx| {
                        Tooltip::new(tooltip.clone()).build(window, cx)
                    })
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{InlineToken, Input, InputState};
    use gpui::{AppContext as _, Context, Entity, Modifiers, Render, TestAppContext, px};
    use std::time::Duration;

    struct TokenField {
        input: Entity<InputState>,
        disabled: bool,
        readonly: bool,
    }

    impl Render for TokenField {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().p_4().child(
                Input::new(&self.input)
                    .disabled(self.disabled)
                    .readonly(self.readonly)
                    .token(|context, _, _| {
                        let token = InputToken::new(context);
                        assert!(token.tooltip.is_none());
                        let token = token.tooltip("Open the project notes");
                        assert_eq!(token.tooltip.as_deref(), Some("Open the project notes"));
                        div().debug_selector(|| "reference".into()).child(token)
                    }),
            )
        }
    }

    #[gpui::test]
    fn token_tooltips_follow_hover_and_disabled_state(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::init(cx);
            cx.set_reduce_motion(true);
        });
        for (disabled, readonly) in [(false, false), (false, true), (true, false)] {
            let (_, cx) = cx.add_window_view(|window, cx| {
                let input = cx.new(|cx| {
                    let mut input = InputState::new(window, cx);
                    input
                        .replace_with_token(InlineToken::new("notes", "notes.md"), window, cx)
                        .unwrap();
                    input
                });
                let content = cx.new(|_| TokenField {
                    input,
                    disabled,
                    readonly,
                });
                crate::Root::new(content, window, cx)
            });
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let bounds = cx.debug_bounds("reference").expect("the token is drawn");
            cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
            cx.run_until_parked();
            cx.executor().advance_clock(Duration::from_secs(2));
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(cx.debug_bounds("tooltip").is_some(), !disabled);

            cx.simulate_mouse_move(gpui::point(px(0.), px(0.)), None, Modifiers::default());
            cx.run_until_parked();
            cx.executor().advance_clock(Duration::from_secs(2));
            cx.run_until_parked();
            cx.update(|window, cx| window.draw(cx).clear(cx));
            assert!(cx.debug_bounds("tooltip").is_none());
        }
    }
}
