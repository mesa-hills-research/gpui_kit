use std::rc::Rc;

use gpui::{
    AnyElement, App, Entity, Half as _, HighlightStyle, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Styled as _, StyledText, Window, div, prelude::FluentBuilder as _,
    px, relative,
};
use gpui_base::input::{SuggestionItemContext, SuggestionItemRenderer, SuggestionMenu};

use crate::{ActiveTheme, ThemeStyled as _, h_flex, input::TextareaState, label::Label};

const MAX_MENU_HEIGHT: Pixels = px(240.);
const MIN_MENU_WIDTH: Pixels = px(120.);
const MAX_MENU_WIDTH: Pixels = px(320.);

/// The menu a textarea offers its suggestions in, styled like the editor's
/// completion menu. `None` while the menu is closed.
///
/// Base owns the menu's behavior and placement; this gives it the popover
/// surface and, unless the application renders them itself, the rows.
pub(crate) fn suggestion_menu(
    state: &Entity<TextareaState>,
    render_item: Option<SuggestionItemRenderer>,
    cx: &App,
) -> Option<AnyElement> {
    if !state.read(cx).is_suggestion_menu_open() {
        return None;
    }
    let render_item = render_item.unwrap_or_else(|| {
        Rc::new(|item, window, cx| render_suggestion(item, window, cx).into_any_element())
    });
    Some(
        SuggestionMenu::new(state)
            .popover_style(cx)
            .shadow_md()
            .text_xs()
            .p_1()
            .min_w(MIN_MENU_WIDTH)
            .max_w(MAX_MENU_WIDTH)
            .max_h(MAX_MENU_HEIGHT)
            .item_renderer(render_item)
            .into_any_element(),
    )
}

/// A suggestion as the menu shows it by default: its label, with what was
/// typed of it highlighted, and its detail after it.
fn render_suggestion(item: &SuggestionItemContext, _: &mut Window, cx: &App) -> impl IntoElement {
    let suggestion = item.suggestion();
    let label = suggestion.label().clone();
    // What was typed is highlighted when the label starts with it, ignoring
    // case: "hel" in "Hello".
    let query = item.query();
    let matched = label
        .get(..query.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(query.as_ref()))
        .map_or(0, str::len);
    let highlights = (matched > 0)
        .then(|| {
            (
                0..matched,
                HighlightStyle {
                    color: Some(cx.theme().blue),
                    ..Default::default()
                },
            )
        })
        .into_iter()
        .collect::<Vec<_>>();

    h_flex()
        .id(item.ix())
        .gap_2()
        .p_1()
        .line_height(relative(1.))
        .rounded(cx.theme().radius.half())
        .hover(|this| this.bg(cx.theme().accent.opacity(0.8)))
        .when(item.is_selected(), |this| {
            this.bg(cx.theme().tokens.accent)
                .text_color(cx.theme().accent_foreground)
        })
        .child(div().child(StyledText::new(label).with_highlights(highlights)))
        .when_some(suggestion.detail(), |this, detail| {
            this.child(
                Label::new(detail.clone())
                    .text_color(cx.theme().muted_foreground)
                    .italic(),
            )
        })
}
