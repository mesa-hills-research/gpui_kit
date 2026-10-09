use gpui::{
    App, Entity, InteractiveElement as _, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _, px, relative,
};
use rust_i18n::t;

use super::FontPickerState;
use crate::{
    ActiveTheme as _, Disableable as _, StyledExt as _, checkbox::Checkbox, h_flex,
    input::NumberInput, list::List, select::Select, v_flex,
};

/// The text the preview shows unless [`FontPicker::preview_text`] sets
/// another: letters, figures easy to confuse, and pairs that ligatures join.
const PREVIEW_TEXT: &str = "The quick brown fox jumps over the lazy dog. 0O 1lI {} => != ==";

/// A font picker: a searchable list of font families, the chosen family's
/// weights, italic, size, line height and OpenType features, and a preview
/// line in the chosen font.
///
/// The state, [`FontPickerState`], owns the catalog and the chosen font.
/// The picker fills its parent's width. Give it a height with `h`, or let
/// the family list keep its own.
#[derive(IntoElement)]
pub struct FontPicker {
    state: Entity<FontPickerState>,
    style: StyleRefinement,
    preview_text: SharedString,
}

impl FontPicker {
    pub fn new(state: &Entity<FontPickerState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            preview_text: PREVIEW_TEXT.into(),
        }
    }

    /// The text of the preview line.
    pub fn preview_text(mut self, text: impl Into<SharedString>) -> Self {
        self.preview_text = text.into();
        self
    }
}

impl Styled for FontPicker {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

/// A labelled control in the options column.
fn field(label: SharedString, control: impl IntoElement, cx: &App) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(control)
}

impl RenderOnce for FontPicker {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let settings = state.settings().clone();
        let family = state.family().cloned();
        let has_italic = family.as_ref().is_some_and(|family| family.has_italic());
        let features = family
            .as_ref()
            .map(|family| family.features())
            .unwrap_or_default();
        let monospace_only = state.is_monospace_only();
        let (families, weights, size, line_height) = (
            state.families.clone(),
            state.weights.clone(),
            state.size.clone(),
            state.line_height.clone(),
        );
        let picker = self.state.clone();

        v_flex()
            .id(("font-picker", self.state.entity_id()))
            .w_full()
            .gap_4()
            .refine_style(&self.style)
            .child(
                h_flex()
                    .w_full()
                    .gap_6()
                    .items_start()
                    .child(
                        v_flex()
                            .w(px(260.))
                            .flex_shrink_0()
                            .gap_2()
                            .child(
                                div()
                                    .h(px(240.))
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .rounded(cx.theme().radius)
                                    .overflow_hidden()
                                    .child(
                                        List::new(&families).search_placeholder(
                                            t!("FontPicker.search").to_string(),
                                        ),
                                    ),
                            )
                            .child(
                                Checkbox::new("font-picker-monospace")
                                    .label(t!("FontPicker.monospace_only").to_string())
                                    .checked(monospace_only)
                                    .on_click({
                                        let picker = picker.clone();
                                        move |checked, _, cx| {
                                            picker.update(cx, |picker, cx| {
                                                picker.set_monospace_only(*checked, cx)
                                            });
                                        }
                                    }),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_3()
                            .child(field(
                                t!("FontPicker.weight").into(),
                                h_flex()
                                    .gap_3()
                                    .child(
                                        div()
                                            .w(px(160.))
                                            .child(Select::new(&weights).menu_width(px(200.))),
                                    )
                                    .child(
                                        div().child(
                                            Checkbox::new("font-picker-italic")
                                                .label(t!("FontPicker.italic").to_string())
                                                .checked(settings.is_italic())
                                                .disabled(!has_italic)
                                                .on_click({
                                                    let picker = picker.clone();
                                                    move |checked, window, cx| {
                                                        picker.update(cx, |picker, cx| {
                                                            picker.set_italic(*checked, window, cx)
                                                        });
                                                    }
                                                }),
                                        ),
                                    ),
                                cx,
                            ))
                            .child(
                                h_flex()
                                    .gap_3()
                                    .child(field(
                                        t!("FontPicker.size").into(),
                                        NumberInput::new(&size).w(px(110.)),
                                        cx,
                                    ))
                                    .child(field(
                                        t!("FontPicker.line_height").into(),
                                        NumberInput::new(&line_height).w(px(110.)),
                                        cx,
                                    )),
                            )
                            .when(!features.is_empty(), |this| {
                                this.child(field(
                                    t!("FontPicker.features").into(),
                                    // A font can list dozens of character variants,
                                    // so the list scrolls past a few rows.
                                    div()
                                        .id("font-picker-features")
                                        .max_h(px(156.))
                                        .overflow_y_scroll()
                                        .child(h_flex().flex_wrap().gap_x_4().gap_y_1().children(
                                            features.into_iter().map(|feature| {
                                                let tag = feature.tag().clone();
                                                let on = settings
                                                    .feature(&tag)
                                                    .unwrap_or(feature.is_on_by_default());
                                                Checkbox::new(SharedString::from(format!(
                                                    "font-picker-feature-{tag}"
                                                )))
                                                .label(feature.label())
                                                .checked(on)
                                                .on_click({
                                                    let picker = picker.clone();
                                                    move |checked, window, cx| {
                                                        picker.update(cx, |picker, cx| {
                                                            picker.set_feature(
                                                                tag.clone(),
                                                                *checked,
                                                                window,
                                                                cx,
                                                            )
                                                        });
                                                    }
                                                })
                                            }),
                                        )),
                                    cx,
                                ))
                            }),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .px_3()
                    .py_2()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().background)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font(settings.font())
                    .text_size(settings.size())
                    .line_height(relative(settings.line_height()))
                    .child(self.preview_text),
            )
    }
}
