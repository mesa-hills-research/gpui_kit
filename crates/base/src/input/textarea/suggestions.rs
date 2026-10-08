//! Suggestions for ordinary text.
//!
//! A [`TextareaState`] asks an application's [`SuggestionProvider`] what could
//! complete the text at the caret, and offers the answer as a menu, as ghost
//! text after the caret, or both. The provider owns what to suggest and in
//! which order; the textarea owns when to ask, which answers still apply, and
//! how the keyboard reaches them.
//!
//! # Stale answers
//!
//! Every request gets a fresh id and records the text revision and caret
//! offset it was made for. An answer is offered only when it answers the
//! newest request *and* the revision and caret are still the ones it was made
//! for. Any edit or caret movement closes what is offered and drops the
//! request in flight, so a slow provider can never offer suggestions for text
//! the user has since changed.
//!
//! # Keys
//!
//! While suggestions are offered the input's own keys reach them first:
//! Up and Down move through the menu, Enter and Tab accept (each can be turned
//! off with [`SuggestionOptions`]) and Escape dismisses. The actions in this
//! module ([`ShowSuggestions`], [`AcceptSuggestion`], [`AcceptSuggestionWord`],
//! [`SelectNextSuggestion`], [`SelectPreviousSuggestion`] and
//! [`DismissSuggestions`]) have no default bindings. Bind them in the `Input`
//! context to give suggestions keys of their own: when there is nothing for
//! one to act on it propagates, so the key falls through to its next binding.

use std::{
    any::Any,
    fmt,
    ops::Range,
    rc::Rc,
    task::{Context as TaskContext, Poll},
    time::Duration,
};

use anyhow::Result;
use futures::FutureExt as _;
use gpui::{
    Action, App, Context, EventEmitter, SharedString, Task, UniformListScrollHandle, Window,
    actions,
};
use ropey::Rope;

use super::{
    Enter, Escape, InputBaseState, MoveDown, MoveUp, TextareaMode, TextareaState,
    undo_manager::EditIntent,
};

actions!(
    input,
    [
        /// Ask the provider for suggestions now, without waiting for typing.
        ShowSuggestions,
        /// Accept the selected suggestion.
        AcceptSuggestion,
        /// Accept the next word of the selected suggestion and keep offering
        /// the rest.
        AcceptSuggestionWord,
        /// Select the next suggestion in the menu.
        SelectNextSuggestion,
        /// Select the previous suggestion in the menu.
        SelectPreviousSuggestion,
        /// Close the suggestions without accepting one.
        DismissSuggestions,
    ]
);

/// A suggestion offered for the text at the caret.
///
/// Accepting it replaces [`Self::range`] with [`Self::text`]. The menu shows
/// [`Self::label`] and [`Self::detail`]; the inline preview shows what
/// accepting would add after the caret.
///
/// ```
/// use gpui_base::input::Suggestion;
///
/// // The user typed "hel" at bytes 10..13; offer to complete the word.
/// let suggestion = Suggestion::new("hello")
///     .with_range(10..13)
///     .with_detail("used 12 times");
///
/// assert_eq!(suggestion.text(), "hello");
/// assert_eq!(suggestion.label(), "hello");
/// ```
#[derive(Clone)]
pub struct Suggestion {
    text: SharedString,
    range: Option<Range<usize>>,
    label: Option<SharedString>,
    detail: Option<SharedString>,
    data: Option<Rc<dyn Any>>,
}

impl Suggestion {
    /// A suggestion to insert `text` at the caret.
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            range: None,
            label: None,
            detail: None,
            data: None,
        }
    }

    /// The byte range of the requested text this suggestion replaces.
    ///
    /// It must contain the caret: a range ending at the caret replaces what
    /// was typed of the word, and an empty range at the caret, the default,
    /// inserts. A suggestion whose range does not contain the caret, or does
    /// not fall on character boundaries, is not offered.
    pub fn with_range(mut self, range: Range<usize>) -> Self {
        self.range = Some(range);
        self
    }

    /// What the menu shows for this suggestion, when it is not the text.
    pub fn with_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Secondary text the menu shows after the label, such as a source or a
    /// kind.
    pub fn with_detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Application data for a custom item renderer or an event handler, read
    /// back with [`Self::data`].
    pub fn with_data(mut self, data: impl Any) -> Self {
        self.data = Some(Rc::new(data));
        self
    }

    /// The text accepting this suggestion inserts.
    pub fn text(&self) -> &SharedString {
        &self.text
    }

    /// The byte range this suggestion replaces.
    ///
    /// `None` until the textarea offers it; from then on it is the range the
    /// suggestion was offered with, an empty range at the caret by default.
    pub fn range(&self) -> Option<Range<usize>> {
        self.range.clone()
    }

    /// What the menu shows for this suggestion: its label, or its text.
    pub fn label(&self) -> &SharedString {
        self.label.as_ref().unwrap_or(&self.text)
    }

    /// The secondary text the menu shows after the label.
    pub fn detail(&self) -> Option<&SharedString> {
        self.detail.as_ref()
    }

    /// The application data set with [`Self::with_data`], if it is a `T`.
    pub fn data<T: 'static>(&self) -> Option<&T> {
        self.data.as_ref()?.downcast_ref()
    }
}

impl fmt::Debug for Suggestion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Suggestion")
            .field("text", &self.text)
            .field("range", &self.range)
            .field("label", &self.label)
            .field("detail", &self.detail)
            .field("data", &self.data.is_some())
            .finish()
    }
}

impl PartialEq for Suggestion {
    /// Equal when they would look and act the same; application data is
    /// compared by identity.
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
            && self.range == other.range
            && self.label == other.label
            && self.detail == other.detail
            && match (&self.data, &other.data) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

/// Why suggestions were requested.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SuggestionTrigger {
    /// The user typed this text, which ends at the caret.
    Typed(SharedString),
    /// [`ShowSuggestions`] or [`TextareaState::show_suggestions`] asked.
    Invoked,
}

/// What a [`SuggestionProvider`] is asked to complete.
#[derive(Clone)]
pub struct SuggestionRequest {
    text: Rope,
    offset: usize,
    revision: u64,
    trigger: SuggestionTrigger,
}

impl SuggestionRequest {
    /// The text as it stands at [`Self::revision`].
    ///
    /// A rope shares its storage, so holding on to it costs nothing; read
    /// windows of it rather than converting all of it to a string.
    pub fn text(&self) -> &Rope {
        &self.text
    }

    /// The caret, as a byte offset into [`Self::text`].
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// The text revision this request was made for: the same count
    /// [`TextChange::revision`] reports.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Why suggestions were requested.
    pub fn trigger(&self) -> &SuggestionTrigger {
        &self.trigger
    }
}

/// One replacement in a [`TextChange`], in the coordinates of the text as it
/// stood just before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    range: Range<usize>,
    text: String,
}

impl TextEdit {
    /// The byte range that was replaced.
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// The text that replaced it.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// How the text changed since a [`SuggestionProvider`] last heard.
///
/// Applying [`Self::edits`] in order to the text the provider last saw gives
/// [`Self::text`].
#[derive(Clone, Debug)]
pub struct TextChange {
    edits: Vec<TextEdit>,
    text: Rope,
    revision: u64,
}

impl TextChange {
    /// The replacements, in the order they were applied.
    pub fn edits(&self) -> &[TextEdit] {
        &self.edits
    }

    /// The text after every edit.
    pub fn text(&self) -> &Rope {
        &self.text
    }

    /// The text revision after every edit. It increases with every change.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

/// Supplies the suggestions a [`TextareaState`] offers.
///
/// The provider decides what to suggest and in which order; the textarea
/// shows the items in the order given. Every method is called on the main
/// thread while the textarea is being updated, so a provider must not read or
/// update the textarea's entity: what it needs arrives as arguments.
pub trait SuggestionProvider {
    /// Returns suggestions for the text at the request's caret.
    ///
    /// A task that is ready when returned is offered in the same update, so
    /// a synchronous provider never shows a frame without its answer. A
    /// pending one is awaited and its answer dropped if the text or caret
    /// changed meanwhile. An error is logged and offers nothing.
    fn suggestions(
        &self,
        request: &SuggestionRequest,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<Suggestion>>>;

    /// Whether typing `text`, which starts at byte `offset`, should ask for
    /// suggestions.
    ///
    /// Defaults to any text with a letter or digit in it.
    fn is_trigger(&self, offset: usize, text: &str, cx: &mut App) -> bool {
        _ = (offset, cx);
        text.chars().any(char::is_alphanumeric)
    }

    /// How long typing must pause before suggestions are requested.
    ///
    /// Defaults to none. [`ShowSuggestions`] never waits.
    fn debounce(&self) -> Duration {
        Duration::ZERO
    }

    /// Hears about every change to the text, so the provider can keep its
    /// own model of it current without copying the whole text.
    ///
    /// Changes made before the provider was installed are not reported: it
    /// starts from the text the textarea held then. Edits are batched per
    /// update and always reported before the next request.
    fn did_change(&self, change: &TextChange, cx: &mut App) {
        _ = (change, cx);
    }
}

/// What happened to offered suggestions.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum SuggestionEvent {
    /// A suggestion was inserted. `partial` when only its next word was,
    /// through [`AcceptSuggestionWord`]; the rest stays offered.
    Accepted {
        suggestion: Suggestion,
        partial: bool,
    },
    /// The user closed the suggestions without accepting one, through Escape
    /// or [`DismissSuggestions`]. Carries the suggestion that was selected.
    ///
    /// Suggestions that typing, moving the caret or losing focus replace or
    /// close are not dismissed: the user went on without them.
    Dismissed { suggestion: Suggestion },
}

/// How a textarea presents and accepts suggestions.
///
/// ```
/// use gpui_base::input::SuggestionOptions;
///
/// // Ghost text only, accepted with Tab; Enter keeps inserting newlines.
/// let options = SuggestionOptions::default().menu(false).inline(true);
/// # _ = options;
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuggestionOptions {
    menu: bool,
    inline: bool,
    accept_on_enter: bool,
    accept_on_tab: bool,
}

impl Default for SuggestionOptions {
    fn default() -> Self {
        Self {
            menu: true,
            inline: false,
            accept_on_enter: true,
            accept_on_tab: true,
        }
    }
}

impl SuggestionOptions {
    /// Whether suggestions are listed in a menu under the word being
    /// completed. Default: `true`.
    pub fn menu(mut self, menu: bool) -> Self {
        self.menu = menu;
        self
    }

    /// Whether the selected suggestion is previewed as ghost text after the
    /// caret. Default: `false`.
    ///
    /// Only a suggestion that continues the text before the caret has a
    /// preview: one that rewrites what was typed shows in the menu alone.
    pub fn inline(mut self, inline: bool) -> Self {
        self.inline = inline;
        self
    }

    /// Whether Enter accepts the selected suggestion while the menu is open.
    /// Default: `true`.
    ///
    /// Enter never accepts a suggestion offered only inline: it inserts a
    /// newline, which leaves the suggestion behind.
    pub fn accept_on_enter(mut self, accept_on_enter: bool) -> Self {
        self.accept_on_enter = accept_on_enter;
        self
    }

    /// Whether Tab accepts the selected suggestion. Default: `true`.
    pub fn accept_on_tab(mut self, accept_on_tab: bool) -> Self {
        self.accept_on_tab = accept_on_tab;
        self
    }
}

/// A textarea's suggestion session: the provider, what is offered, and the
/// request in flight.
pub(crate) struct Suggestions {
    provider: Option<Rc<dyn SuggestionProvider>>,
    options: SuggestionOptions,
    /// What is offered. Empty when nothing is.
    items: Vec<Suggestion>,
    selected_ix: usize,
    /// The caret offset `items` were offered at.
    offset: usize,
    /// The selected suggestion's preview, when the inline presentation has one.
    ghost: Option<SharedString>,
    /// Identifies the newest request. An answer to any other is stale.
    request_id: u64,
    /// The request in flight, or its debounce. Dropping it cancels both.
    task: Task<()>,
    /// Edits applied since the provider last heard of a change.
    pending_edits: Vec<TextEdit>,
    flush_scheduled: bool,
    pub(crate) scroll_handle: UniformListScrollHandle,
}

impl Default for Suggestions {
    fn default() -> Self {
        Self {
            provider: None,
            options: SuggestionOptions::default(),
            items: Vec::new(),
            selected_ix: 0,
            offset: 0,
            ghost: None,
            request_id: 0,
            task: Task::ready(()),
            pending_edits: Vec::new(),
            flush_scheduled: false,
            scroll_handle: UniformListScrollHandle::new(),
        }
    }
}

impl Suggestions {
    pub(crate) fn ghost_text(&self) -> Option<&str> {
        self.ghost.as_deref()
    }

    fn is_offered(&self) -> bool {
        !self.items.is_empty()
    }

    fn is_menu_open(&self) -> bool {
        self.options.menu && self.is_offered()
    }

    fn selected(&self) -> Option<&Suggestion> {
        self.items.get(self.selected_ix)
    }

    /// Drops the request in flight and makes any answer to it stale.
    fn cancel_request(&mut self) {
        self.request_id = self.request_id.wrapping_add(1);
        self.task = Task::ready(());
    }
}

/// The part of `remainder` that [`AcceptSuggestionWord`] takes: leading
/// whitespace, then one word, or one other character.
fn next_word(remainder: &str) -> &str {
    let start = remainder.len() - remainder.trim_start().len();
    let mut chars = remainder[start..].char_indices();
    let Some((_, first)) = chars.next() else {
        return remainder;
    };
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    if !is_word(first) {
        return &remainder[..start + first.len_utf8()];
    }
    let end = chars
        .find(|&(_, c)| !is_word(c))
        .map_or(remainder.len(), |(ix, _)| start + ix);
    &remainder[..end]
}

/// Methods for the suggestions a textarea offers. See [`SuggestionProvider`].
impl TextareaState {
    /// Offer suggestions from `provider`.
    pub fn suggestion_provider(mut self, provider: Rc<dyn SuggestionProvider>) -> Self {
        self.extras.suggestions.provider = Some(provider);
        self
    }

    /// Replace the suggestion provider, or remove it with `None`. Whatever
    /// the old one offered is closed.
    pub fn set_suggestion_provider(
        &mut self,
        provider: Option<Rc<dyn SuggestionProvider>>,
        cx: &mut Context<Self>,
    ) {
        self.close_suggestions(cx);
        let suggestions = &mut self.extras.suggestions;
        suggestions.pending_edits.clear();
        suggestions.provider = provider;
    }

    /// Set how suggestions are presented and accepted.
    pub fn suggestion_options(mut self, options: SuggestionOptions) -> Self {
        self.extras.suggestions.options = options;
        self
    }

    /// Change how suggestions are presented and accepted. Whatever is offered
    /// is closed.
    pub fn set_suggestion_options(&mut self, options: SuggestionOptions, cx: &mut Context<Self>) {
        if self.extras.suggestions.options == options {
            return;
        }
        self.close_suggestions(cx);
        self.extras.suggestions.options = options;
    }

    /// The text revision: a count that increases with every change to the
    /// text. [`SuggestionRequest::revision`] and [`TextChange::revision`]
    /// report the same count.
    pub fn revision(&self) -> u64 {
        self.document_revision
    }

    /// The suggestions offered now, in the provider's order. Empty when none
    /// are.
    pub fn suggestions(&self) -> &[Suggestion] {
        &self.extras.suggestions.items
    }

    /// The index of the selected suggestion, while suggestions are offered.
    pub fn selected_suggestion_ix(&self) -> Option<usize> {
        let suggestions = &self.extras.suggestions;
        suggestions.is_offered().then_some(suggestions.selected_ix)
    }

    /// Whether the suggestion menu is open.
    pub fn is_suggestion_menu_open(&self) -> bool {
        self.extras.suggestions.is_menu_open()
    }

    /// Ask the provider for suggestions at the caret now, without a debounce.
    pub fn show_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_suggestions(SuggestionTrigger::Invoked, Duration::ZERO, window, cx);
    }

    /// Offer `suggestions` at the caret, without asking the provider.
    ///
    /// They are offered for the text as it stands, and closed like any other
    /// by the next edit or caret movement. An empty list closes what is
    /// offered.
    pub fn present_suggestions(&mut self, suggestions: Vec<Suggestion>, cx: &mut Context<Self>) {
        self.extras.suggestions.cancel_request();
        self.offer_suggestions(suggestions, cx);
    }

    /// Close the suggestions and drop any request in flight, without an event.
    pub fn hide_suggestions(&mut self, cx: &mut Context<Self>) {
        self.close_suggestions(cx);
    }

    /// Select the suggestion at `ix`. Out-of-range indices are ignored.
    pub fn select_suggestion(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.extras.suggestions.items.len() {
            return;
        }
        self.extras.suggestions.selected_ix = ix;
        self.update_suggestion_ghost();
        self.extras
            .suggestions
            .scroll_handle
            .scroll_to_item(ix, gpui::ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Select the next suggestion, wrapping past the last.
    pub fn select_next_suggestion(&mut self, cx: &mut Context<Self>) {
        let len = self.extras.suggestions.items.len();
        if len > 0 {
            self.select_suggestion((self.extras.suggestions.selected_ix + 1) % len, cx);
        }
    }

    /// Select the previous suggestion, wrapping past the first.
    pub fn select_previous_suggestion(&mut self, cx: &mut Context<Self>) {
        let len = self.extras.suggestions.items.len();
        if len > 0 {
            self.select_suggestion((self.extras.suggestions.selected_ix + len - 1) % len, cx);
        }
    }

    /// Insert the selected suggestion. Returns whether one was.
    pub fn accept_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.accept_selected_suggestion(false, window, cx)
    }

    /// Insert the next word of the selected suggestion, and keep offering the
    /// rest. Returns whether anything was inserted.
    ///
    /// A suggestion that rewrites what was typed, rather than continuing it,
    /// has no next word to take and is accepted whole.
    pub fn accept_suggestion_word(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.accept_selected_suggestion(true, window, cx)
    }

    /// Insert the suggestion at `ix`, as clicking it in the menu does.
    pub(crate) fn accept_suggestion_at(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if ix >= self.extras.suggestions.items.len() {
            return false;
        }
        self.extras.suggestions.selected_ix = ix;
        self.accept_selected_suggestion(false, window, cx)
    }

    /// Close the suggestions as the user's choice, reporting what was
    /// selected.
    fn dismiss_suggestions(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(suggestion) = self.extras.suggestions.selected().cloned() else {
            return false;
        };
        self.close_suggestions(cx);
        cx.emit(SuggestionEvent::Dismissed { suggestion });
        true
    }

    /// Close whatever is offered and drop the request in flight.
    ///
    /// This is what an edit, a caret movement or losing focus does: the
    /// suggestions are left behind rather than dismissed, so there is no
    /// event.
    pub(crate) fn close_suggestions(&mut self, cx: &mut Context<Self>) {
        let suggestions = &mut self.extras.suggestions;
        suggestions.cancel_request();
        if suggestions.is_offered() {
            suggestions.items.clear();
            suggestions.selected_ix = 0;
            suggestions.ghost = None;
            cx.notify();
        }
    }

    /// Record an edit for the provider. See [`crate::input::InputModeKind::did_edit`].
    pub(crate) fn record_suggestion_edit(
        &mut self,
        range: &Range<usize>,
        new_len: usize,
        cx: &mut Context<Self>,
    ) {
        // An edit leaves whatever was offered for the old text behind. Most
        // edit paths close it before they start, but IME composition does not.
        self.close_suggestions(cx);
        if self.extras.suggestions.provider.is_none() {
            return;
        }
        let end = range.start + new_len;
        let text = self.text.slice(range.start..end).to_string();
        let suggestions = &mut self.extras.suggestions;
        suggestions.pending_edits.push(TextEdit {
            range: range.clone(),
            text,
        });
        // The edit is still being applied: its revision is counted after this
        // returns. Report it once the whole update is done.
        if !suggestions.flush_scheduled {
            suggestions.flush_scheduled = true;
            let state = cx.weak_entity();
            cx.defer(move |cx| {
                _ = state.update(cx, |state, cx| state.flush_suggestion_edits(cx));
            });
        }
    }

    /// Tell the provider about the edits it has not heard of.
    fn flush_suggestion_edits(&mut self, cx: &mut App) {
        let suggestions = &mut self.extras.suggestions;
        suggestions.flush_scheduled = false;
        if suggestions.pending_edits.is_empty() {
            return;
        }
        let edits = std::mem::take(&mut suggestions.pending_edits);
        let Some(provider) = suggestions.provider.clone() else {
            return;
        };
        provider.did_change(
            &TextChange {
                edits,
                text: self.text.clone(),
                revision: self.document_revision,
            },
            cx,
        );
    }

    /// Ask for suggestions after the user typed `text`, which ends at the
    /// caret.
    pub(crate) fn suggest_after_typing(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) = self.extras.suggestions.provider.clone() else {
            return;
        };
        if !self.accepts_suggestions() {
            return;
        }
        let Some(start) = self.cursor().checked_sub(text.len()) else {
            return;
        };
        if !provider.is_trigger(start, text, cx) {
            return;
        }
        let trigger = SuggestionTrigger::Typed(SharedString::new(text));
        self.request_suggestions(trigger, provider.debounce(), window, cx);
    }

    /// Whether the caret is somewhere suggestions can be offered: one
    /// collapsed cursor, no IME composition, and text the user may change.
    fn accepts_suggestions(&self) -> bool {
        self.is_editable()
            && self.selections.is_single()
            && self.active_selection().is_empty()
            && self.ime_marked_range.is_none()
    }

    fn request_suggestions(
        &mut self,
        trigger: SuggestionTrigger,
        debounce: Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_suggestions(cx);
        if self.extras.suggestions.provider.is_none() || !self.accepts_suggestions() {
            return;
        }
        let id = self.extras.suggestions.request_id;
        if debounce.is_zero() {
            self.send_suggestion_request(id, trigger, window, cx);
            return;
        }
        let timer = cx.background_executor().timer(debounce);
        self.extras.suggestions.task = cx.spawn_in(window, async move |state, cx| {
            timer.await;
            _ = state.update_in(cx, |state, window, cx| {
                state.send_suggestion_request(id, trigger, window, cx)
            });
        });
    }

    fn send_suggestion_request(
        &mut self,
        id: u64,
        trigger: SuggestionTrigger,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if id != self.extras.suggestions.request_id {
            return;
        }
        let Some(provider) = self.extras.suggestions.provider.clone() else {
            return;
        };
        // The provider hears about every edit before it is asked about the
        // text they produced.
        self.flush_suggestion_edits(cx);
        let request = SuggestionRequest {
            text: self.text.clone(),
            offset: self.cursor(),
            revision: self.document_revision,
            trigger,
        };
        let mut task = provider.suggestions(&request, window, cx);

        // A synchronous answer is offered in this update, before the next
        // frame is drawn.
        let mut poll_cx = TaskContext::from_waker(futures::task::noop_waker_ref());
        if let Poll::Ready(answer) = task.poll_unpin(&mut poll_cx) {
            self.receive_suggestions(id, &request, answer, cx);
            return;
        }
        self.extras.suggestions.task = cx.spawn(async move |state, cx| {
            let answer = task.await;
            _ = state.update(cx, |state, cx| {
                state.receive_suggestions(id, &request, answer, cx)
            });
        });
    }

    /// Offer a provider's answer, unless it went stale while it was computed.
    fn receive_suggestions(
        &mut self,
        id: u64,
        request: &SuggestionRequest,
        answer: Result<Vec<Suggestion>>,
        cx: &mut Context<Self>,
    ) {
        if id != self.extras.suggestions.request_id
            || request.revision != self.document_revision
            || request.offset != self.cursor()
        {
            return;
        }
        // Answered: nothing is in flight any more.
        self.extras.suggestions.task = Task::ready(());
        let items = answer.unwrap_or_else(|error| {
            tracing::warn!("suggestion provider failed: {error:#}");
            Vec::new()
        });
        self.offer_suggestions(items, cx);
    }

    /// Offer `items` at the caret, dropping any whose range cannot apply.
    fn offer_suggestions(&mut self, items: Vec<Suggestion>, cx: &mut Context<Self>) {
        let offset = self.cursor();
        let items: Vec<Suggestion> = if self.accepts_suggestions() {
            items
                .into_iter()
                .filter_map(|mut item| {
                    let range = item.range.clone().unwrap_or(offset..offset);
                    let applies = range.start <= offset
                        && offset <= range.end
                        && range.end <= self.text.len()
                        && self.text.is_char_boundary(range.start)
                        && self.text.is_char_boundary(range.end);
                    applies.then(|| {
                        item.range = Some(range);
                        item
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        let had_items = self.extras.suggestions.is_offered();
        let suggestions = &mut self.extras.suggestions;
        suggestions.items = items;
        suggestions.selected_ix = 0;
        suggestions.offset = offset;
        if suggestions.is_offered() {
            suggestions
                .scroll_handle
                .scroll_to_item(0, gpui::ScrollStrategy::Top);
        }
        self.update_suggestion_ghost();
        if had_items || self.extras.suggestions.is_offered() {
            cx.notify();
        }
    }

    /// What accepting `suggestion` would add after the caret, when it
    /// continues the text before the caret.
    fn suggestion_remainder(&self, suggestion: &Suggestion) -> Option<SharedString> {
        let range = suggestion.range.clone()?;
        let offset = self.cursor();
        if range.end != offset {
            return None;
        }
        let typed = self.text.slice(range.start..offset).to_string();
        let remainder = suggestion.text.strip_prefix(typed.as_str())?;
        (!remainder.is_empty()).then(|| SharedString::new(remainder))
    }

    fn update_suggestion_ghost(&mut self) {
        let ghost = if self.extras.suggestions.options.inline {
            self.extras
                .suggestions
                .selected()
                .and_then(|selected| self.suggestion_remainder(selected))
        } else {
            None
        };
        self.extras.suggestions.ghost = ghost;
    }

    fn accept_selected_suggestion(
        &mut self,
        partial: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(suggestion) = self.extras.suggestions.selected().cloned() else {
            return false;
        };
        if !self.accepts_suggestions() {
            return false;
        }
        let Some(range) = suggestion.range.clone() else {
            return false;
        };
        let offset = self.cursor();

        // Taking one word only makes sense of a suggestion that continues the
        // text, and only while more than that word is left.
        let word = partial
            .then(|| self.suggestion_remainder(&suggestion))
            .flatten()
            .and_then(|remainder| {
                let word = next_word(&remainder);
                (word.len() < remainder.len()).then(|| word.to_string())
            });
        let (range, text) = match &word {
            Some(word) => (offset..offset, word.as_str()),
            None => (range, suggestion.text.as_ref()),
        };

        // An accepted suggestion is one step of its own in the history, apart
        // from the typing before it.
        self.undo_manager.break_transaction_coalescing();
        self.undo_manager.set_pending_intent(EditIntent::Atomic);
        let range_utf16 = self.range_to_utf16(&range);
        self.replace_text_in_range_silent(Some(range_utf16), text, window, cx);
        self.undo_manager.break_transaction_coalescing();

        let partial = word.is_some();
        if partial {
            // The edit closed the suggestion; offer the rest of it again, still
            // replacing from where it started.
            let start = suggestion
                .range
                .as_ref()
                .map_or(offset, |range| range.start);
            let rest = suggestion.clone().with_range(start..self.cursor());
            self.offer_suggestions(vec![rest], cx);
        }
        cx.emit(SuggestionEvent::Accepted {
            suggestion,
            partial,
        });
        true
    }

    /// Let offered suggestions consume an input action first. Returns whether
    /// they did.
    pub(crate) fn handle_suggestion_action(
        &mut self,
        action: &dyn Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let suggestions = &self.extras.suggestions;
        let (offered, menu_open) = (suggestions.is_offered(), suggestions.is_menu_open());
        let accept_on_enter = suggestions.options.accept_on_enter;
        if !offered {
            return false;
        }
        if action.partial_eq(&Escape) {
            return self.dismiss_suggestions(cx);
        }
        // Inline-only suggestions leave the arrows and Enter to the text.
        if !menu_open {
            return false;
        }
        if action.partial_eq(&MoveUp) {
            self.select_previous_suggestion(cx);
            true
        } else if action.partial_eq(&MoveDown) {
            self.select_next_suggestion(cx);
            true
        } else if accept_on_enter
            && action.partial_eq(&Enter {
                secondary: false,
                shift: false,
            })
        {
            self.accept_suggestion(window, cx)
        } else {
            false
        }
    }

    /// Tab: accept the selected suggestion, when Tab accepts.
    pub(crate) fn accept_suggestion_on_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.extras.suggestions.options.accept_on_tab && self.accept_suggestion(window, cx)
    }

    pub(crate) fn has_suggestion_ghost(&self) -> bool {
        self.extras.suggestions.ghost.is_some()
    }

    /// Where the menu is anchored: the start of the selected suggestion's
    /// range, so the menu lines up with the word it completes.
    pub(crate) fn suggestion_anchor(&self) -> Option<usize> {
        let suggestions = &self.extras.suggestions;
        if !suggestions.is_menu_open() {
            return None;
        }
        let offset = suggestions
            .selected()
            .and_then(|selected| selected.range.as_ref())
            .map_or(suggestions.offset, |range| range.start);
        Some(offset)
    }

    /// The text `suggestion` replaces.
    pub(crate) fn suggestion_query(&self, suggestion: &Suggestion) -> SharedString {
        suggestion
            .range
            .as_ref()
            .filter(|range| range.end <= self.text.len())
            .map(|range| SharedString::new(self.text.slice(range.clone()).to_string()))
            .unwrap_or_default()
    }

    fn on_action_show_suggestions(
        &mut self,
        _: &ShowSuggestions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.extras.suggestions.provider.is_none() || !self.accepts_suggestions() {
            cx.propagate();
            return;
        }
        self.show_suggestions(window, cx);
    }

    fn on_action_accept_suggestion(
        &mut self,
        _: &AcceptSuggestion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.accept_suggestion(window, cx) {
            cx.propagate();
        }
    }

    fn on_action_accept_suggestion_word(
        &mut self,
        _: &AcceptSuggestionWord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.accept_suggestion_word(window, cx) {
            cx.propagate();
        }
    }

    fn on_action_select_next_suggestion(
        &mut self,
        _: &SelectNextSuggestion,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_suggestion_menu_open() {
            self.select_next_suggestion(cx);
        } else {
            cx.propagate();
        }
    }

    fn on_action_select_previous_suggestion(
        &mut self,
        _: &SelectPreviousSuggestion,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_suggestion_menu_open() {
            self.select_previous_suggestion(cx);
        } else {
            cx.propagate();
        }
    }

    fn on_action_dismiss_suggestions(
        &mut self,
        _: &DismissSuggestions,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.dismiss_suggestions(cx) {
            cx.propagate();
        }
    }

    /// Registers the suggestion actions on the textarea's root element.
    pub(crate) fn register_suggestion_actions(
        element: gpui::Stateful<gpui::Div>,
        entity: &gpui::Entity<Self>,
        window: &mut Window,
    ) -> gpui::Stateful<gpui::Div> {
        use gpui::InteractiveElement as _;
        element
            .on_action(window.listener_for(entity, Self::on_action_show_suggestions))
            .on_action(window.listener_for(entity, Self::on_action_accept_suggestion))
            .on_action(window.listener_for(entity, Self::on_action_accept_suggestion_word))
            .on_action(window.listener_for(entity, Self::on_action_select_next_suggestion))
            .on_action(window.listener_for(entity, Self::on_action_select_previous_suggestion))
            .on_action(window.listener_for(entity, Self::on_action_dismiss_suggestions))
    }
}

impl EventEmitter<SuggestionEvent> for InputBaseState<TextareaMode> {}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use futures::channel::oneshot;
    use gpui::{
        AppContext as _, Entity, EntityInputHandler as _, InteractiveElement as _, IntoElement,
        Modifiers, ParentElement as _, Pixels, Render, Styled as _, Subscription, TestAppContext,
        VisualTestContext, div, point, px, size,
    };

    use super::*;
    use crate::input::{Redo, SuggestionMenu, Undo};

    /// Suggests the words it knows that continue the word before the caret.
    #[derive(Default)]
    struct WordProvider {
        words: Vec<&'static str>,
        debounce: Duration,
        /// Hold every answer until the test sends it.
        held: bool,
        answers: RefCell<Vec<(Vec<Suggestion>, oneshot::Sender<Vec<Suggestion>>)>>,
        requests: RefCell<Vec<SuggestionRequest>>,
        changes: RefCell<Vec<TextChange>>,
    }

    impl WordProvider {
        fn new(words: &[&'static str]) -> Self {
            Self {
                words: words.to_vec(),
                ..Default::default()
            }
        }

        /// Send the held answer to request `ix`.
        fn answer(&self, ix: usize) {
            let (items, sender) = self.answers.borrow_mut().remove(ix);
            _ = sender.send(items);
        }
    }

    impl SuggestionProvider for WordProvider {
        fn suggestions(
            &self,
            request: &SuggestionRequest,
            _: &mut Window,
            cx: &mut App,
        ) -> Task<Result<Vec<Suggestion>>> {
            self.requests.borrow_mut().push(request.clone());
            let before = request.text().slice(..request.offset()).to_string();
            let word = before
                .rsplit(|c: char| !c.is_alphanumeric())
                .next()
                .unwrap_or_default();
            let start = request.offset() - word.len();
            let items: Vec<Suggestion> = self
                .words
                .iter()
                .filter(|candidate| candidate.starts_with(word) && **candidate != word)
                .map(|candidate| Suggestion::new(*candidate).with_range(start..request.offset()))
                .collect();
            if !self.held {
                return Task::ready(Ok(items));
            }
            let (sender, receiver) = oneshot::channel();
            self.answers.borrow_mut().push((items, sender));
            cx.spawn(async move |_| Ok(receiver.await.unwrap_or_default()))
        }

        fn debounce(&self) -> Duration {
            self.debounce
        }

        fn did_change(&self, change: &TextChange, _: &mut App) {
            self.changes.borrow_mut().push(change.clone());
        }
    }

    struct Harness {
        textarea: Entity<TextareaState>,
        other: gpui::FocusHandle,
        _subscription: Subscription,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                // Clear of the window edge, which the menu keeps a margin from.
                .child(div().h(px(100.)).pl(px(20.)).child(self.textarea.clone()))
                .child(div().track_focus(&self.other).size(px(10.)))
                .child(
                    SuggestionMenu::new(&self.textarea)
                        .max_h(px(60.))
                        .item(|item, _, _| {
                            div().h(px(20.)).child(item.suggestion().label().clone())
                        }),
                )
        }
    }

    struct Test {
        cx: VisualTestContext,
        textarea: Entity<TextareaState>,
        other: gpui::FocusHandle,
        events: Rc<RefCell<Vec<SuggestionEvent>>>,
    }

    impl Test {
        fn new(
            cx: &mut TestAppContext,
            provider: Option<Rc<WordProvider>>,
            options: SuggestionOptions,
        ) -> Self {
            cx.update(crate::init);
            let events: Rc<RefCell<Vec<SuggestionEvent>>> = Default::default();
            let window = cx.open_window(size(px(300.), px(200.)), {
                let events = events.clone();
                move |window, cx| {
                    let textarea = cx.new(|cx| {
                        let state = TextareaState::new(window, cx).suggestion_options(options);
                        match provider {
                            Some(provider) => state.suggestion_provider(provider),
                            None => state,
                        }
                    });
                    let _subscription = cx.subscribe(&textarea, {
                        let events = events.clone();
                        move |_, _, event: &SuggestionEvent, _| {
                            events.borrow_mut().push(event.clone())
                        }
                    });
                    Harness {
                        textarea,
                        other: cx.focus_handle(),
                        _subscription,
                    }
                }
            });
            let mut cx = VisualTestContext::from_window(window.into(), cx);
            let (textarea, other) = window
                .read_with(&cx, |harness, _| {
                    (harness.textarea.clone(), harness.other.clone())
                })
                .unwrap();
            cx.update(|window, cx| {
                textarea.update(cx, |state, cx| state.focus(window, cx));
                window.draw(cx).clear(cx);
            });
            Self {
                cx,
                textarea,
                other,
                events,
            }
        }

        fn draw(&mut self) {
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
        }

        fn value(&self) -> String {
            self.textarea
                .read_with(&self.cx, |state, _| state.value().to_string())
        }

        fn labels(&self) -> Vec<String> {
            self.textarea.read_with(&self.cx, |state, _| {
                state
                    .suggestions()
                    .iter()
                    .map(|suggestion| suggestion.label().to_string())
                    .collect()
            })
        }

        fn selected(&self) -> Option<usize> {
            self.textarea
                .read_with(&self.cx, |state, _| state.selected_suggestion_ix())
        }

        fn ghost(&self) -> Option<String> {
            self.textarea.read_with(&self.cx, |state, _| {
                state.extras.suggestions.ghost_text().map(str::to_string)
            })
        }

        fn update<R>(
            &mut self,
            f: impl FnOnce(&mut TextareaState, &mut Window, &mut Context<TextareaState>) -> R,
        ) -> R {
            let textarea = self.textarea.clone();
            self.cx
                .update(|window, cx| textarea.update(cx, |state, cx| f(state, window, cx)))
        }

        fn take_events(&self) -> Vec<SuggestionEvent> {
            std::mem::take(&mut self.events.borrow_mut())
        }
    }

    fn menu() -> SuggestionOptions {
        SuggestionOptions::default()
    }

    fn inline() -> SuggestionOptions {
        SuggestionOptions::default().menu(false).inline(true)
    }

    fn accepted(text: &str, partial: bool) -> impl Fn(&SuggestionEvent) -> bool {
        let text = text.to_string();
        move |event| {
            matches!(event, SuggestionEvent::Accepted { suggestion, partial: p }
                if suggestion.text() == &text && *p == partial)
        }
    }

    #[test]
    fn next_word_takes_leading_space_and_one_word() {
        assert_eq!(next_word("lo world"), "lo");
        assert_eq!(next_word(" world again"), " world");
        assert_eq!(next_word("  again"), "  again");
        assert_eq!(next_word(", then"), ",");
        assert_eq!(next_word("   "), "   ");
        assert_eq!(next_word("don't"), "don");
        assert_eq!(next_word("naïve café"), "naïve");
    }

    /// Typing asks the provider and opens the menu with its answer, in its
    /// order; Down then Enter inserts the second suggestion, once.
    #[gpui::test]
    fn typing_opens_the_menu_and_enter_accepts_the_selection(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider::new(&["help", "hello", "helium"]));
        let mut test = Test::new(cx, Some(provider.clone()), menu());

        test.cx.simulate_input("hel");
        assert_eq!(test.labels(), ["help", "hello", "helium"]);
        assert_eq!(test.selected(), Some(0));
        let request = provider.requests.borrow().last().cloned().unwrap();
        assert_eq!(request.offset(), 3);
        assert_eq!(request.trigger(), &SuggestionTrigger::Typed("l".into()));
        test.draw();
        assert!(test.cx.debug_bounds("suggestion-menu").is_some());

        test.cx.simulate_keystrokes("down");
        assert_eq!(test.selected(), Some(1));
        assert_eq!(test.value(), "hel", "the menu takes Down, the caret stays");
        test.cx.simulate_keystrokes("enter");
        assert_eq!(test.value(), "hello");
        assert!(test.labels().is_empty());
        let events = test.take_events();
        assert_eq!(events.len(), 1);
        assert!(accepted("hello", false)(&events[0]));
        test.update(|state, _, _| assert_eq!(state.cursor(), 5));

        // Accepting is one step in the history, apart from the typing.
        test.update(|state, window, cx| state.undo(&Undo, window, cx));
        assert_eq!(test.value(), "hel");
        test.update(|state, window, cx| state.redo(&Redo, window, cx));
        assert_eq!(test.value(), "hello");
    }

    /// Up and Down wrap around the menu, and Escape dismisses it with the
    /// selection; a second Escape has nothing to dismiss.
    #[gpui::test]
    fn escape_dismisses_with_the_selected_suggestion(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider::new(&["help", "hello", "helium"]));
        let mut test = Test::new(cx, Some(provider), menu());

        test.cx.simulate_input("hel");
        test.cx.simulate_keystrokes("up");
        assert_eq!(test.selected(), Some(2));
        test.cx.simulate_keystrokes("down");
        assert_eq!(test.selected(), Some(0));
        test.cx.simulate_keystrokes("up");
        test.cx.simulate_keystrokes("escape");
        assert!(test.labels().is_empty());
        let events = test.take_events();
        assert_eq!(
            events,
            [SuggestionEvent::Dismissed {
                suggestion: Suggestion::new("helium").with_range(0..3),
            }]
        );
        test.cx.simulate_keystrokes("escape");
        assert!(test.take_events().is_empty());
        assert_eq!(test.value(), "hel");
    }

    /// A provider whose task is ready is offered in the same update as the
    /// keystroke, so the first frame drawn after it shows the menu.
    #[gpui::test]
    fn a_ready_answer_is_offered_in_the_same_update(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider::new(&["hello"]));
        let mut test = Test::new(cx, Some(provider), menu());

        let offered = test.update(|state, window, cx| {
            state.replace_text_in_range(None, "h", window, cx);
            state.suggestions().len()
        });
        assert_eq!(offered, 1);
        test.draw();
        let menu = test.cx.debug_bounds("suggestion-menu").unwrap();
        let caret = test.update(|state, _, _| state.range_to_bounds(&(0..0)).unwrap());
        assert!(
            menu.top() >= caret.bottom(),
            "the menu opens under the word"
        );
        assert!((menu.left() - caret.left()).abs() < px(1.));
    }

    /// An answer for text the user has since changed is never shown: typing on
    /// drops the request in flight, and only the newest answer is offered.
    #[gpui::test]
    fn an_answer_for_older_text_is_dropped(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider {
            held: true,
            ..WordProvider::new(&["hello", "help"])
        });
        let mut test = Test::new(cx, Some(provider.clone()), menu());

        test.cx.simulate_input("h");
        test.cx.simulate_input("e");
        assert_eq!(provider.answers.borrow().len(), 2);
        // The answer for "h" arrives after "e" was typed.
        provider.answer(0);
        test.cx.run_until_parked();
        assert!(test.labels().is_empty());

        provider.answer(0);
        test.cx.run_until_parked();
        assert_eq!(test.labels(), ["hello", "help"]);

        // Moving the caret while an answer is on its way drops it too.
        test.cx.simulate_input("l");
        test.cx.simulate_keystrokes("left");
        provider.answer(0);
        test.cx.run_until_parked();
        assert!(test.labels().is_empty());
        assert!(test.take_events().is_empty());
    }

    /// The answer itself is checked as well: one whose request no longer
    /// matches the text revision or the caret is not offered, whatever
    /// delivered it.
    #[gpui::test]
    fn an_answer_is_checked_against_revision_and_caret(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, Some(Rc::new(WordProvider::new(&[]))), menu());
        test.cx.simulate_input("he");
        test.update(|state, _, cx| {
            let id = state.extras.suggestions.request_id;
            let text = state.text.clone();
            let request = |offset, revision| SuggestionRequest {
                text: text.clone(),
                offset,
                revision,
                trigger: SuggestionTrigger::Invoked,
            };
            let answer = || Ok(vec![Suggestion::new("hello").with_range(0..2)]);
            let stale_revision = request(2, state.document_revision - 1);
            state.receive_suggestions(id, &stale_revision, answer(), cx);
            assert!(state.suggestions().is_empty());
            let stale_caret = request(1, state.document_revision);
            state.receive_suggestions(id, &stale_caret, answer(), cx);
            assert!(state.suggestions().is_empty());
            let superseded = request(2, state.document_revision);
            state.receive_suggestions(id.wrapping_sub(1), &superseded, answer(), cx);
            assert!(state.suggestions().is_empty());
            let current = request(2, state.document_revision);
            state.receive_suggestions(id, &current, answer(), cx);
            assert_eq!(state.suggestions().len(), 1);
        });
    }

    /// With a debounce, suggestions are requested once typing pauses, for the
    /// text as it then stands.
    #[gpui::test]
    fn debounce_waits_for_typing_to_pause(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider {
            debounce: Duration::from_millis(100),
            ..WordProvider::new(&["hello"])
        });
        let mut test = Test::new(cx, Some(provider.clone()), menu());

        test.cx.simulate_input("h");
        test.cx.executor().advance_clock(Duration::from_millis(60));
        test.cx.simulate_input("e");
        test.cx.executor().advance_clock(Duration::from_millis(60));
        assert!(provider.requests.borrow().is_empty());
        test.cx.executor().advance_clock(Duration::from_millis(50));
        test.cx.run_until_parked();
        let requests = provider.requests.borrow();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].text().to_string(), "he");
        drop(requests);
        assert_eq!(test.labels(), ["hello"]);
    }

    /// Suggestions can be shown and hidden without typing, from a provider or
    /// from the application itself.
    #[gpui::test]
    fn suggestions_open_and_close_programmatically(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider::new(&["hello"]));
        let mut test = Test::new(cx, Some(provider.clone()), menu());
        test.update(|state, window, cx| state.set_value("he", window, cx));
        test.update(|state, _, cx| state.set_selected_range(2..2, cx));

        test.update(|state, window, cx| state.show_suggestions(window, cx));
        assert_eq!(test.labels(), ["hello"]);
        assert_eq!(
            provider.requests.borrow().last().unwrap().trigger(),
            &SuggestionTrigger::Invoked
        );
        test.update(|state, _, cx| state.hide_suggestions(cx));
        assert!(test.labels().is_empty());
        assert!(test.take_events().is_empty(), "hiding is not dismissing");

        test.update(|state, _, cx| {
            state.present_suggestions(
                vec![
                    Suggestion::new("hey").with_range(0..2),
                    // Its range does not reach the caret, so it cannot apply.
                    Suggestion::new("x").with_range(0..1),
                ],
                cx,
            )
        });
        assert_eq!(test.labels(), ["hey"]);
        test.update(|state, window, cx| assert!(state.accept_suggestion(window, cx)));
        assert_eq!(test.value(), "hey");
    }

    /// Tab accepts like Enter does, and either key can be left to the text.
    #[gpui::test]
    fn tab_and_enter_accept_unless_turned_off(cx: &mut TestAppContext) {
        let words = ["hello"];
        let mut test = Test::new(cx, Some(Rc::new(WordProvider::new(&words))), menu());
        test.cx.simulate_input("hel");
        test.cx.simulate_keystrokes("tab");
        assert_eq!(test.value(), "hello");

        let options = menu().accept_on_tab(false).accept_on_enter(false);
        let mut test = Test::new(cx, Some(Rc::new(WordProvider::new(&words))), options);
        test.cx.simulate_input("hel");
        assert_eq!(test.labels(), ["hello"]);
        test.cx.simulate_keystrokes("enter");
        assert_eq!(test.value(), "hel\n");
        assert!(test.labels().is_empty());
        test.cx.simulate_input("hel");
        test.cx.simulate_keystrokes("tab");
        assert_ne!(test.value(), "hel\nhello", "Tab indents instead");
        assert!(test.labels().is_empty());
        assert!(test.take_events().is_empty());
    }

    /// The inline presentation shows the rest of the suggestion after the caret
    /// and leaves the arrows and Enter to the text.
    #[gpui::test]
    fn inline_suggestions_preview_after_the_caret(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider::new(&["hello"]));
        let mut test = Test::new(cx, Some(provider), inline());

        test.cx.simulate_input("hel");
        assert_eq!(test.ghost().as_deref(), Some("lo"));
        test.update(|state, _, _| assert!(!state.is_suggestion_menu_open()));
        test.draw();
        assert!(test.cx.debug_bounds("suggestion-menu").is_none());

        test.cx.simulate_keystrokes("tab");
        assert_eq!(test.value(), "hello");
        assert_eq!(test.ghost(), None);

        test.cx.simulate_input(" hel");
        assert_eq!(test.ghost().as_deref(), Some("lo"));
        test.cx.simulate_keystrokes("enter");
        assert_eq!(test.value(), "hello hel\n", "Enter is a newline");
        assert_eq!(test.ghost(), None);

        // A suggestion that rewrites what was typed has nothing to preview.
        test.update(|state, _, cx| {
            state.present_suggestions(vec![Suggestion::new("Hello").with_range(10..10)], cx)
        });
        assert_eq!(test.ghost().as_deref(), Some("Hello"));
        test.update(|state, window, cx| {
            state.replace_text_in_range(None, "h", window, cx);
            state.present_suggestions(vec![Suggestion::new("Hello").with_range(10..11)], cx)
        });
        assert_eq!(test.ghost(), None);
    }

    /// Accepting a word at a time inserts one word, keeps offering the rest,
    /// and accepts the last word as the whole suggestion.
    #[gpui::test]
    fn accept_word_inserts_one_word_at_a_time(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider::new(&["hello world again"]));
        let mut test = Test::new(cx, Some(provider), inline());
        test.cx.update(|_, cx| {
            cx.bind_keys([gpui::KeyBinding::new(
                "alt-right",
                AcceptSuggestionWord,
                Some("Input"),
            )])
        });

        test.cx.simulate_input("hel");
        assert_eq!(test.ghost().as_deref(), Some("lo world again"));
        test.cx.simulate_keystrokes("alt-right");
        assert_eq!(test.value(), "hello");
        assert_eq!(test.ghost().as_deref(), Some(" world again"));
        test.cx.simulate_keystrokes("alt-right");
        assert_eq!(test.value(), "hello world");
        assert_eq!(test.ghost().as_deref(), Some(" again"));
        test.cx.simulate_keystrokes("alt-right");
        assert_eq!(test.value(), "hello world again");
        assert_eq!(test.ghost(), None);

        let events = test.take_events();
        assert_eq!(events.len(), 3);
        assert!(accepted("hello world again", true)(&events[0]));
        assert!(accepted("hello world again", true)(&events[1]));
        assert!(accepted("hello world again", false)(&events[2]));

        // With nothing offered the key falls through to its next binding.
        test.cx.simulate_keystrokes("alt-right");
        assert_eq!(test.value(), "hello world again");
    }

    /// Bound keys reach the suggestions only while there is something for them
    /// to do, and fall through to the key's next binding otherwise.
    #[gpui::test]
    fn suggestion_actions_fall_through_when_idle(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider::new(&["hello", "help"]));
        let options = menu().accept_on_tab(false);
        let mut test = Test::new(cx, Some(provider.clone()), options);
        test.cx.update(|_, cx| {
            cx.bind_keys([
                gpui::KeyBinding::new("ctrl-space", ShowSuggestions, Some("Input")),
                gpui::KeyBinding::new("ctrl-n", SelectNextSuggestion, Some("Input")),
                gpui::KeyBinding::new("ctrl-p", SelectPreviousSuggestion, Some("Input")),
                gpui::KeyBinding::new("ctrl-g", DismissSuggestions, Some("Input")),
                // Over the input's own Tab, which indents.
                gpui::KeyBinding::new("tab", AcceptSuggestion, Some("Input")),
            ])
        });
        test.update(|state, window, cx| {
            state.set_value("he", window, cx);
            state.set_selected_range(2..2, cx);
        });

        test.cx.simulate_keystrokes("ctrl-space");
        assert_eq!(test.labels(), ["hello", "help"]);
        test.cx.simulate_keystrokes("ctrl-n");
        assert_eq!(test.selected(), Some(1));
        test.cx.simulate_keystrokes("ctrl-p");
        assert_eq!(test.selected(), Some(0));
        test.cx.simulate_keystrokes("ctrl-g");
        assert!(test.labels().is_empty());
        assert!(matches!(
            test.take_events().as_slice(),
            [SuggestionEvent::Dismissed { .. }]
        ));
        test.cx.simulate_keystrokes("ctrl-space tab");
        assert_eq!(test.value(), "hello");
        assert_eq!(provider.requests.borrow().len(), 2);

        // Nothing offered: Tab indents, as it would without the binding.
        test.cx.simulate_keystrokes("tab");
        assert_ne!(test.value(), "hello");
        assert_eq!(test.value().trim(), "hello");
        assert_eq!(test.take_events().len(), 1);
    }

    /// Clicking a suggestion accepts it; pressing anywhere else closes the menu
    /// without dismissing it.
    #[gpui::test]
    fn the_pointer_accepts_and_closes(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider::new(&["help", "hello", "helium"]));
        let mut test = Test::new(cx, Some(provider), menu());

        test.cx.simulate_input("hel");
        test.draw();
        let row = test.cx.debug_bounds("suggestion-1").unwrap();
        test.cx.simulate_click(row.center(), Modifiers::default());
        assert_eq!(test.value(), "hello");
        assert!(accepted("hello", false)(&test.take_events()[0]));

        test.cx.simulate_input(" hel");
        test.draw();
        let menu = test.cx.debug_bounds("suggestion-menu").unwrap();
        test.cx.simulate_click(
            point(menu.right() + px(20.), menu.top()),
            Modifiers::default(),
        );
        assert!(test.labels().is_empty());
        assert!(test.take_events().is_empty());
    }

    /// The menu scrolls inside the height it is given, and keeps the selected
    /// suggestion in view.
    #[gpui::test]
    fn the_menu_scrolls_to_the_selection(cx: &mut TestAppContext) {
        let words = ["a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8"];
        let mut test = Test::new(cx, Some(Rc::new(WordProvider::new(&words))), menu());
        test.cx.simulate_input("a");
        test.draw();
        let menu = test.cx.debug_bounds("suggestion-menu").unwrap();
        assert_eq!(menu.size.height, px(60.), "three rows of eight fit");
        assert!(test.cx.debug_bounds("suggestion-7").is_none());
        for _ in 0..7 {
            test.cx.simulate_keystrokes("down");
        }
        test.draw();
        test.draw();
        let last = test.cx.debug_bounds("suggestion-7").unwrap();
        assert!(last.bottom() <= menu.bottom() + px(0.5));
    }

    /// Suggestions are left behind, without an event, when focus moves away or
    /// the whole text is selected.
    #[gpui::test]
    fn focus_and_selection_close_suggestions(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider::new(&["hello"]));
        let mut test = Test::new(cx, Some(provider.clone()), menu());

        // Focus changes reach listeners only in the active window.
        test.cx.update(|window, _| window.activate_window());
        test.draw();
        test.cx.simulate_input("hel");
        assert!(!test.labels().is_empty());
        let other = test.other.clone();
        test.cx.update(|window, cx| other.focus(window, cx));
        // Blur is delivered once the frame with the new focus is drawn.
        test.draw();
        assert!(test.labels().is_empty());

        let textarea = test.textarea.clone();
        test.cx.update(|window, cx| {
            textarea.update(cx, |state, cx| {
                state.focus(window, cx);
                state.show_suggestions(window, cx);
                assert!(!state.suggestions().is_empty());
                state.select_all(window, cx);
                assert!(state.suggestions().is_empty());
                // Nothing applies to a selection.
                state.show_suggestions(window, cx);
                assert!(state.suggestions().is_empty());
            })
        });
        assert!(test.take_events().is_empty());
    }

    /// The provider hears about every change, in edits that turn the text it
    /// last saw into the current text, each change with a higher revision.
    #[gpui::test]
    fn the_provider_hears_every_change(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider::new(&[]));
        let mut test = Test::new(cx, Some(provider.clone()), menu());
        let model = RefCell::new(String::new());
        let last_revision = Cell::new(test.update(|state, _, _| state.revision()));

        let check = |test: &mut Test, step: &str| {
            for change in provider.changes.borrow_mut().drain(..) {
                let mut text = model.borrow_mut();
                for edit in change.edits() {
                    text.replace_range(edit.range(), edit.text());
                }
                assert!(change.revision() > last_revision.get(), "{step}");
                last_revision.set(change.revision());
                assert_eq!(*text, change.text().to_string(), "{step}");
            }
            assert_eq!(*model.borrow(), test.value(), "{step}");
            assert_eq!(
                last_revision.get(),
                test.update(|state, _, _| state.revision()),
                "{step}"
            );
        };

        test.cx.simulate_input("one two");
        check(&mut test, "typing");
        test.update(|state, window, cx| state.insert("three ", window, cx));
        check(&mut test, "insert");
        test.update(|state, window, cx| {
            state.set_selected_range(0..4, cx);
            state.replace("1 ", window, cx);
        });
        check(&mut test, "replace a selection");
        test.cx.simulate_keystrokes("backspace backspace");
        check(&mut test, "delete");
        test.update(|state, window, cx| state.undo(&Undo, window, cx));
        check(&mut test, "undo");
        test.update(|state, window, cx| state.redo(&Redo, window, cx));
        check(&mut test, "redo");
        test.update(|state, window, cx| {
            state.replace_and_mark_text_in_range(None, "ni", None, window, cx);
            state.replace_and_mark_text_in_range(None, "nih", None, window, cx);
            state.replace_text_in_range(None, "你好", window, cx);
        });
        check(&mut test, "IME");
        test.update(|state, window, cx| state.set_value("fresh\ntext", window, cx));
        check(&mut test, "set_value");
        test.update(|state, window, cx| {
            state.set_selected_range(0..0, cx);
            state.replace_text_in_range(None, "multi ", window, cx);
            state.add_cursor_at(12, cx);
            state.replace_text_in_range(None, "x", window, cx);
        });
        check(&mut test, "several cursors");
    }

    /// The width of `text` shaped the way the textarea shapes its own.
    fn shaped_width(test: &mut Test, text: &'static str) -> Pixels {
        test.cx.update(|window, _| {
            let style = window.text_style();
            let size = style.font_size.to_pixels(window.rem_size());
            window
                .text_system()
                .shape_line(text.into(), size, &[style.to_run(text.len())], None)
                .width
        })
    }

    /// Ghost text in the middle of a line moves the rest of the line right
    /// instead of covering it, and the caret stays in front of it.
    #[gpui::test]
    fn ghost_text_moves_the_rest_of_the_line(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, None, inline());
        test.update(|state, window, cx| {
            state.set_value("foo bar", window, cx);
            state.set_selected_range(3..3, cx);
        });
        test.draw();
        let x = |test: &mut Test, offset: usize| {
            test.update(|state, _, _| state.range_to_bounds(&(offset..offset)).unwrap().left())
        };
        let (foo, caret, bar) = (x(&mut test, 1), x(&mut test, 3), x(&mut test, 4));

        test.update(|state, _, cx| {
            state.present_suggestions(vec![Suggestion::new("foobaz").with_range(0..3)], cx)
        });
        test.draw();
        assert_eq!(test.ghost().as_deref(), Some("baz"));
        let ghost = shaped_width(&mut test, "baz");
        assert!(ghost > px(0.));
        assert_eq!(x(&mut test, 1), foo);
        assert_eq!(
            x(&mut test, 3),
            caret,
            "the caret stays in front of the ghost text"
        );
        let shift = x(&mut test, 4) - bar;
        assert!(
            (shift - ghost).abs() < px(0.01),
            "\" bar\" moves right by the ghost text's width: {shift:?} vs {ghost:?}"
        );
        let cursor = test.update(|state, _, _| state.last_layout.as_ref().unwrap().cursor_bounds);
        assert!((cursor.unwrap().left() - caret).abs() < px(1.));

        test.cx.simulate_keystrokes("tab");
        assert_eq!(test.value(), "foobaz bar");
        test.draw();
        assert_eq!(test.update(|state, _, _| state.cursor()), 6);
    }

    /// In a line with inline tokens the ghost text takes its place among them
    /// the same way.
    #[gpui::test]
    fn ghost_text_moves_the_rest_of_a_line_with_tokens(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, None, inline());
        test.update(|state, window, cx| {
            state.set_value("@a foo bar", window, cx);
            state
                .replace_range_with_token(
                    0..2,
                    crate::input::InlineToken::new("a", "@a"),
                    window,
                    cx,
                )
                .unwrap();
            state.set_selected_range(6..6, cx);
        });
        test.draw();
        let x = |test: &mut Test, offset: usize| {
            test.update(|state, _, _| state.range_to_bounds(&(offset..offset)).unwrap().left())
        };
        let (token, caret, bar) = (x(&mut test, 2), x(&mut test, 6), x(&mut test, 7));

        test.update(|state, _, cx| {
            state.present_suggestions(vec![Suggestion::new("foobaz").with_range(3..6)], cx)
        });
        test.draw();
        let ghost = shaped_width(&mut test, "baz");
        assert_eq!(x(&mut test, 2), token);
        assert_eq!(x(&mut test, 6), caret);
        assert!(((x(&mut test, 7) - bar) - ghost).abs() < px(0.01));
    }

    /// Ghost text that no longer fits on its row wraps the line, and the line
    /// unwraps when the ghost text goes.
    #[gpui::test]
    fn ghost_text_wraps_with_the_line(cx: &mut TestAppContext) {
        let mut test = Test::new(cx, None, inline());
        let rows = |test: &mut Test| {
            test.draw();
            test.update(|state, _, _| state.display_map.wrap_row_count())
        };
        // Fill the first row as far as whole words go.
        let mut text = String::from("word");
        loop {
            let longer = format!("{text} word");
            test.update(|state, window, cx| state.set_value(longer.clone(), window, cx));
            if rows(&mut test) > 1 {
                break;
            }
            text = longer;
        }
        test.update(|state, window, cx| {
            state.set_value(text.clone(), window, cx);
            state.set_selected_range(text.len()..text.len(), cx);
        });
        assert_eq!(rows(&mut test), 1);

        let end = text.len();
        test.update(|state, _, cx| {
            state.present_suggestions(
                vec![Suggestion::new("wordsmithing").with_range(end - 4..end)],
                cx,
            )
        });
        assert_eq!(rows(&mut test), 2);
        // The last word moved down with the ghost text, and the caret with it.
        let (first, word, caret) = test.update(|state, _, _| {
            let bounds = |offset| state.range_to_bounds(&(offset..offset)).unwrap();
            (bounds(0), bounds(end - 4), bounds(end))
        });
        assert!(word.top() > first.top());
        assert_eq!(word.top(), caret.top());

        test.update(|state, _, cx| state.hide_suggestions(cx));
        assert_eq!(rows(&mut test), 1);
    }

    /// Several cursors, a selection or a readonly textarea offer nothing.
    #[gpui::test]
    fn suggestions_need_one_caret_in_editable_text(cx: &mut TestAppContext) {
        let provider = Rc::new(WordProvider::new(&["hello"]));
        let mut test = Test::new(cx, Some(provider.clone()), menu());
        test.update(|state, window, cx| {
            state.set_value("he he", window, cx);
            state.set_selected_range(2..2, cx);
            state.add_cursor_at(5, cx);
            state.show_suggestions(window, cx);
            assert!(state.suggestions().is_empty());
            state.set_selected_range(0..2, cx);
            state.show_suggestions(window, cx);
            assert!(state.suggestions().is_empty());
            state.set_selected_range(2..2, cx);
            state.set_readonly(true, cx);
            state.show_suggestions(window, cx);
            assert!(state.suggestions().is_empty());
        });
        assert!(provider.requests.borrow().is_empty());
    }
}
