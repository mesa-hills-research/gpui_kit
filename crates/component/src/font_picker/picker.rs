use gpui::{
    App, Entity, InteractiveElement as _, IntoElement, ParentElement, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, div, percentage, prelude::FluentBuilder as _, px, relative,
};
use rust_i18n::t;

use super::{FontFeature, FontPickerState, FontSettings};
use crate::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::NumberInput,
    list::List,
    scroll::ScrollableElement as _,
    select::Select,
    v_flex,
};

/// The text the preview shows unless [`FontPicker::preview_text`] sets
/// another: letters, figures easy to confuse, and pairs that ligatures join.
const PREVIEW_TEXT: &str = "The quick brown fox jumps over the lazy dog. 0O 1lI {} => != ==";

/// The width of each feature's checkbox in the features grid, so that the
/// checkboxes line up in columns.
const FEATURE_WIDTH: f32 = 200.;

/// A font picker: a searchable list of font families, the chosen family's
/// weights, italic, size, line height and OpenType features, and a preview
/// line in the chosen font.
///
/// The state, [`FontPickerState`], owns the catalog and the chosen font.
/// The picker fills its parent's width. Give it a height with `h`, or let
/// the family list keep its own.
///
/// [`FontPicker::compact`] lays it out as labelled rows for a settings page,
/// with the families in a dropdown and the features behind a disclosure.
#[derive(IntoElement)]
pub struct FontPicker {
    state: Entity<FontPickerState>,
    style: StyleRefinement,
    preview_text: SharedString,
    compact: bool,
    collapsible_features: bool,
}

impl FontPicker {
    pub fn new(state: &Entity<FontPickerState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            preview_text: PREVIEW_TEXT.into(),
            compact: false,
            collapsible_features: false,
        }
    }

    /// The text of the preview line.
    pub fn preview_text(mut self, text: impl Into<SharedString>) -> Self {
        self.preview_text = text.into();
        self
    }

    /// Lay the picker out as a column of labelled rows, with each control at
    /// the end of its row: the families in a dropdown with a search field,
    /// then the weight, size and line height, and the OpenType features
    /// behind a disclosure. Suits a settings page, where the full layout's
    /// open list of families takes too much room.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Show the OpenType features behind a disclosure the user opens, as
    /// [`FontPicker::compact`] does, rather than always. A font can offer
    /// dozens of them. Off by default.
    pub fn collapsible_features(mut self, collapsible: bool) -> Self {
        self.collapsible_features = collapsible;
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

/// A row of the compact layout: the label at the start, the control at the end.
fn row(label: SharedString, control: impl IntoElement) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_4()
        .justify_between()
        .child(div().flex_1().min_w_0().text_sm().child(label))
        .child(h_flex().flex_shrink_0().gap_3().child(control))
}

/// What the picker shows of the chosen font.
struct Chosen {
    settings: FontSettings,
    has_italic: bool,
    features: Vec<FontFeature>,
    monospace_only: bool,
    features_open: bool,
}

impl Chosen {
    fn read(state: &FontPickerState) -> Self {
        let family = state.family();
        Self {
            settings: state.settings().clone(),
            has_italic: family.is_some_and(|family| family.has_italic()),
            features: family.map(|family| family.features()).unwrap_or_default(),
            monospace_only: state.is_monospace_only(),
            features_open: state.is_features_open(),
        }
    }

    /// How many features the user switched away from the font's own choice.
    fn changed_features(&self) -> usize {
        self.features
            .iter()
            .filter(|feature| {
                self.settings
                    .feature(feature.tag())
                    .is_some_and(|on| on != feature.is_on_by_default())
            })
            .count()
    }
}

impl FontPicker {
    fn monospace_checkbox(&self, monospace_only: bool) -> impl IntoElement {
        let picker = self.state.clone();
        Checkbox::new("font-picker-monospace")
            .label(t!("FontPicker.monospace_only").to_string())
            .checked(monospace_only)
            .on_click(move |checked, _, cx| {
                picker.update(cx, |picker, cx| picker.set_monospace_only(*checked, cx));
            })
    }

    fn italic_checkbox(&self, chosen: &Chosen) -> impl IntoElement {
        let picker = self.state.clone();
        Checkbox::new("font-picker-italic")
            .label(t!("FontPicker.italic").to_string())
            .checked(chosen.settings.is_italic())
            .disabled(!chosen.has_italic)
            .on_click(move |checked, window, cx| {
                picker.update(cx, |picker, cx| picker.set_italic(*checked, window, cx));
            })
    }

    fn feature_checkbox(&self, feature: &FontFeature, settings: &FontSettings) -> Checkbox {
        let tag = feature.tag().clone();
        let on = settings.feature(&tag).unwrap_or(feature.is_on_by_default());
        let picker = self.state.clone();
        Checkbox::new(SharedString::from(format!("font-picker-feature-{tag}")))
            .label(feature.label())
            .checked(on)
            .on_click(move |checked, window, cx| {
                picker.update(cx, |picker, cx| {
                    picker.set_feature(tag.clone(), *checked, window, cx)
                });
            })
    }

    /// The features as one wrapping list, scrolling past a few rows: a font
    /// can list dozens of character variants.
    fn features_list(&self, chosen: &Chosen) -> impl IntoElement {
        // The list grows with its features up to the maximum height, then
        // scrolls, the way a dialog's body does.
        v_flex().max_h(px(156.)).child(
            v_flex().flex_1().overflow_hidden().child(
                div().flex_1().overflow_hidden().child(
                    v_flex()
                        .id("font-picker-features")
                        .size_full()
                        .overflow_y_scrollbar()
                        .child(
                            h_flex().flex_wrap().gap_x_4().gap_y_1().children(
                                chosen.features.iter().map(|feature| {
                                    self.feature_checkbox(feature, &chosen.settings)
                                }),
                            ),
                        ),
                ),
            ),
        )
    }

    /// The features in groups, the font's named features first, then its
    /// stylistic sets and character variants, in columns.
    fn features_grid(&self, chosen: &Chosen, cx: &App) -> impl IntoElement {
        let group = |title: Option<SharedString>, features: Vec<&FontFeature>| {
            (!features.is_empty()).then(|| {
                v_flex()
                    .gap_1p5()
                    .when_some(title, |this, title| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(title),
                        )
                    })
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_y_1p5()
                            .children(features.into_iter().map(|feature| {
                                div()
                                    .w(px(FEATURE_WIDTH))
                                    .pr_3()
                                    .child(self.feature_checkbox(feature, &chosen.settings))
                            })),
                    )
            })
        };
        let kind = |feature: &&FontFeature| feature.order().0;
        let of_kind = |n| {
            chosen
                .features
                .iter()
                .filter(|feature| kind(feature) == n)
                .collect::<Vec<_>>()
        };
        v_flex()
            .id("font-picker-features")
            .w_full()
            .gap_3()
            .children(group(None, of_kind(0)))
            .children(group(
                Some(t!("FontPicker.stylistic_sets").into()),
                of_kind(1),
            ))
            .children(group(
                Some(t!("FontPicker.character_variants").into()),
                of_kind(2),
            ))
    }

    /// The features behind a disclosure: a button that shows and hides them,
    /// and how many the user changed.
    fn features_disclosure(&self, chosen: &Chosen, cx: &App) -> impl IntoElement {
        let open = chosen.features_open;
        let changed = chosen.changed_features();
        let picker = self.state.clone();
        v_flex()
            .w_full()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_between()
                    .child(
                        // The chevron lines up with the labels above it.
                        Button::new("font-picker-features-toggle")
                            .ghost()
                            .small()
                            .ml(px(-8.))
                            .icon(
                                Icon::new(IconName::ChevronRight).rotate(percentage(if open {
                                    0.25
                                } else {
                                    0.
                                })),
                            )
                            .label(t!("FontPicker.font_features").to_string())
                            .toggled(open)
                            .on_click(move |_, _, cx| {
                                picker.update(cx, |picker, cx| picker.set_features_open(!open, cx));
                            }),
                    )
                    .when(changed > 0, |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("FontPicker.changed", n = changed).to_string()),
                        )
                    }),
            )
            .when(open, |this| {
                this.child(div().pl_5().child(self.features_grid(chosen, cx)))
            })
    }

    fn preview(&self, settings: &FontSettings, cx: &App) -> impl IntoElement {
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
            .child(self.preview_text.clone())
    }

    fn render_full(self, chosen: Chosen, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let (families, weights, size, line_height) = (
            state.families.clone(),
            state.weights.clone(),
            state.size.clone(),
            state.line_height.clone(),
        );

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
                            .child(self.monospace_checkbox(chosen.monospace_only)),
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
                                        div().w(px(160.)).child(
                                            Select::new(&weights)
                                                .id("font-picker-weight")
                                                .menu_width(px(200.)),
                                        ),
                                    )
                                    .child(div().child(self.italic_checkbox(&chosen))),
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
                            .when(
                                !chosen.features.is_empty() && !self.collapsible_features,
                                |this| {
                                    this.child(field(
                                        t!("FontPicker.features").into(),
                                        self.features_list(&chosen),
                                        cx,
                                    ))
                                },
                            ),
                    ),
            )
            .when(
                !chosen.features.is_empty() && self.collapsible_features,
                |this| this.child(self.features_disclosure(&chosen, cx)),
            )
            .child(self.preview(&chosen.settings, cx))
    }

    fn render_compact(self, chosen: Chosen, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state.update(cx, |state, cx| {
            state.prepare_family_select(window, cx);
        });
        let state = self.state.read(cx);
        let loading = state.is_loading();
        let (family_select, weights, size, line_height) = (
            state.family_select.clone(),
            state.weights.clone(),
            state.size.clone(),
            state.line_height.clone(),
        );
        // The selects share a width, so that they line up in a column.
        let select_width = px(240.);

        v_flex()
            .id(("font-picker", self.state.entity_id()))
            .w_full()
            .gap_4()
            .refine_style(&self.style)
            .child(row(
                t!("FontPicker.family").into(),
                h_flex()
                    .gap_3()
                    .child(self.monospace_checkbox(chosen.monospace_only))
                    .child(
                        div().w(select_width).child(
                            Select::new(&family_select)
                                .id("font-picker-family")
                                .search_placeholder(t!("FontPicker.search").to_string())
                                .placeholder(if loading {
                                    t!("FontPicker.loading").to_string()
                                } else {
                                    chosen.settings.family().to_string()
                                })
                                .empty(|_, cx| {
                                    h_flex()
                                        .h_16()
                                        .justify_center()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(t!("FontPicker.no_match").to_string())
                                })
                                .menu_max_h(px(320.)),
                        ),
                    ),
            ))
            .child(row(
                t!("FontPicker.weight").into(),
                h_flex().gap_3().child(self.italic_checkbox(&chosen)).child(
                    div()
                        .w(select_width)
                        .child(Select::new(&weights).id("font-picker-weight")),
                ),
            ))
            .child(row(
                t!("FontPicker.size").into(),
                NumberInput::new(&size).w(px(120.)),
            ))
            .child(row(
                t!("FontPicker.line_height").into(),
                NumberInput::new(&line_height).w(px(120.)),
            ))
            .when(!chosen.features.is_empty(), |this| {
                this.child(self.features_disclosure(&chosen, cx))
            })
            .child(self.preview(&chosen.settings, cx))
    }
}

impl RenderOnce for FontPicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let chosen = Chosen::read(self.state.read(cx));
        if self.compact {
            self.render_compact(chosen, window, cx).into_any_element()
        } else {
            self.render_full(chosen, cx).into_any_element()
        }
    }
}
