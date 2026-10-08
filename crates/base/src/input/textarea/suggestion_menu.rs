//! The unstyled suggestion menu: the list a textarea's suggestions are offered
//! in, under the word they complete.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Empty, Entity, InteractiveElement as _, IntoElement, ListSizingBehavior,
    ParentElement as _, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    StyleRefinement, Styled, Window, deferred, div, px, uniform_list,
};

use super::{Suggestion, TextareaState};
use crate::{Align, POPUP_PRIORITY, Placement, Positioner, StyledExt as _};

/// Read-only context for rendering one suggestion in the menu.
#[derive(Clone)]
pub struct SuggestionItemContext {
    suggestion: Suggestion,
    ix: usize,
    selected: bool,
    query: SharedString,
}

impl SuggestionItemContext {
    /// The suggestion to render.
    pub fn suggestion(&self) -> &Suggestion {
        &self.suggestion
    }

    /// Its position in the menu, from zero.
    pub fn ix(&self) -> usize {
        self.ix
    }

    /// Whether it is the selected suggestion, which Enter and Tab accept.
    pub fn is_selected(&self) -> bool {
        self.selected
    }

    /// The text the suggestion replaces: what was typed of the word, for a
    /// suggestion that completes it. Highlight it to show why the suggestion
    /// matched.
    pub fn query(&self) -> &SharedString {
        &self.query
    }
}

/// A renderer installed by a styled control. Not part of the supported API.
#[doc(hidden)]
pub type SuggestionItemRenderer =
    Rc<dyn Fn(&SuggestionItemContext, &mut Window, &mut App) -> AnyElement>;

/// An unstyled menu of a textarea's suggestions, anchored under the word they
/// complete.
///
/// It renders nothing while the menu is closed. The [`TextareaState`] owns
/// what is offered, which suggestion is selected and what accepting does, and
/// takes the keyboard while the menu is open; this element owns placement,
/// scrolling and the pointer. Clicking a suggestion accepts it, and pressing
/// the mouse anywhere else closes the menu.
///
/// Style the surface through [`Styled`] and give it a maximum height: the list
/// scrolls inside it. Each suggestion renders through [`Self::item`].
///
/// ```ignore
/// div()
///     .child(Textarea::new(&state))
///     .child(
///         SuggestionMenu::new(&state)
///             .max_h(px(240.))
///             .bg(cx.theme().popover)
///             .item(|item, _, _| div().child(item.suggestion().label().clone())),
///     )
/// ```
#[derive(IntoElement)]
pub struct SuggestionMenu {
    state: Entity<TextareaState>,
    style: StyleRefinement,
    item: Option<SuggestionItemRenderer>,
}

impl SuggestionMenu {
    pub fn new(state: &Entity<TextareaState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            item: None,
        }
    }

    /// The element each suggestion renders as. Without one, a suggestion
    /// renders as its label.
    pub fn item<R: IntoElement>(
        mut self,
        render: impl Fn(&SuggestionItemContext, &mut Window, &mut App) -> R + 'static,
    ) -> Self {
        self.item = Some(Rc::new(move |item, window, cx| {
            render(item, window, cx).into_any_element()
        }));
        self
    }

    /// Install an item renderer a styled control already holds.
    #[doc(hidden)]
    pub fn item_renderer(mut self, renderer: SuggestionItemRenderer) -> Self {
        self.item = Some(renderer);
        self
    }
}

impl Styled for SuggestionMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SuggestionMenu {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let Some(anchor) = state.suggestion_anchor() else {
            return Empty.into_any_element();
        };
        // The line box at the start of the word, in window coordinates.
        let Some(anchor_bounds) = state.range_to_bounds(&(anchor..anchor)) else {
            return Empty.into_any_element();
        };
        let suggestions = state.suggestions();
        let count = suggestions.len();
        // Every row is as wide as the widest label, so measure that one.
        let widest_ix = suggestions
            .iter()
            .enumerate()
            .max_by_key(|(_, suggestion)| {
                suggestion.label().chars().count()
                    + suggestion
                        .detail()
                        .map_or(0, |detail| detail.chars().count())
            })
            .map(|(ix, _)| ix);
        let scroll_handle = state.extras.suggestions.scroll_handle.clone();

        let render_item = self.item.unwrap_or_else(|| {
            Rc::new(
                |item: &SuggestionItemContext, _: &mut Window, _: &mut App| {
                    div()
                        .child(item.suggestion().label().clone())
                        .into_any_element()
                },
            )
        });
        let entity = self.state.clone();
        let list = uniform_list("suggestions", count, move |range, window, cx| {
            let state = entity.read(cx);
            let selected_ix = state.selected_suggestion_ix();
            let items: Vec<SuggestionItemContext> = range
                .filter_map(|ix| {
                    let suggestion = state.suggestions().get(ix)?.clone();
                    Some(SuggestionItemContext {
                        query: state.suggestion_query(&suggestion),
                        suggestion,
                        ix,
                        selected: selected_ix == Some(ix),
                    })
                })
                .collect();
            items
                .into_iter()
                .map(|item| {
                    let ix = item.ix;
                    let entity = entity.clone();
                    div()
                        .id(ix)
                        .role(Role::ListBoxOption)
                        .aria_label(item.suggestion.label().clone())
                        .aria_selected(item.selected)
                        .debug_selector(move || format!("suggestion-{ix}"))
                        .on_click(move |_, window, cx| {
                            entity.update(cx, |state, cx| {
                                state.accept_suggestion_at(ix, window, cx);
                            });
                        })
                        .child(render_item(&item, window, cx))
                })
                .collect()
        })
        .with_sizing_behavior(ListSizingBehavior::Infer)
        .with_width_from_item(widest_ix)
        .track_scroll(&scroll_handle)
        // Shrinks to the surface's maximum height and scrolls inside it.
        .min_h_0();

        let state = self.state.clone();
        deferred(
            Positioner::side(anchor_bounds)
                .placement(Placement::Bottom)
                .align(Align::Start)
                .offset(px(2.))
                .occlude()
                .child(
                    div()
                        .id("suggestion-menu")
                        .role(Role::ListBox)
                        .debug_selector(|| "suggestion-menu".into())
                        .flex()
                        .flex_col()
                        .refine_style(&self.style)
                        // Pressing anywhere else leaves the suggestions behind.
                        // The textarea itself closes them too when the press
                        // lands in it, and this covers everywhere else.
                        .on_mouse_down_out(move |_, _, cx| {
                            state.update(cx, |state, cx| state.hide_suggestions(cx));
                        })
                        .child(list),
                ),
        )
        .with_priority(POPUP_PRIORITY)
        .into_any_element()
    }
}
