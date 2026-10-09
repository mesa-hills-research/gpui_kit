//! Emacs keys.
//!
//! Control and Meta carry the commands. Meta is Alt on Windows and Linux and
//! Option on macOS, where Cmd keeps the system's shortcuts for the clipboard,
//! history and Select All. Escape followed by a key is Meta with that key.
//!
//! The scheme keeps an [`EmacsState`] per textarea: the mark, the kill ring
//! and the prefix argument being typed. Each key runs one of the actions
//! below, which reads the state as it starts. The commands are grouped by
//! area:
//!
//! - this module: the table, the mark and the region, the prefix argument and
//!   search.
//! - `motion`: moving, which extends the region while the mark is active.
//! - `kill`: the kill ring, killing and yanking, and the system clipboard.
//! - `edit`: deleting, transposing, case and lines.
//!
//! Some commands depend on the one before: consecutive kills join into one
//! entry of the kill ring, and Meta-Y only follows a yank. A command records
//! what it did with a [`Fingerprint`] of the text and the selections, so a
//! click or an edit in between breaks the chain. A prefix argument applies to
//! the next command the same way.

mod edit;
mod kill;
mod motion;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::ops::Range;

use gpui::{
    Action, Context, Div, Entity, EntityInputHandler as _, InteractiveElement as _, KeyBinding,
    SharedString, Stateful, Window, actions,
};
use serde::Deserialize;

use super::{Keymap, KeymapPlatform, KeymapState, bind, common};
use crate::input::{Copy, Cut, Enter, Paste, Redo, Search, SelectAll, TextareaState, Undo};
use kill::KillRing;
use motion::Motion;

actions!(
    emacs,
    [
        /// Move forward a character: C-f.
        ForwardChar,
        /// Move back a character: C-b.
        BackwardChar,
        /// Move down a line: C-n.
        NextLine,
        /// Move up a line: C-p.
        PreviousLine,
        /// Move to the end of the next word: M-f.
        ForwardWord,
        /// Move to the start of the word before the caret: M-b.
        BackwardWord,
        /// Move to the start of the line: C-a.
        BeginningOfLine,
        /// Move to the end of the line: C-e.
        EndOfLine,
        /// Move to the end of the sentence: M-e.
        ForwardSentence,
        /// Move to the start of the sentence: M-a.
        BackwardSentence,
        /// Move to the blank line after the paragraph: M-}.
        ForwardParagraph,
        /// Move to the blank line before the paragraph: M-{.
        BackwardParagraph,
        /// Move to the start of the text, leaving the mark where the caret
        /// was: M-<.
        BeginningOfBuffer,
        /// Move to the end of the text, leaving the mark where the caret was:
        /// M->.
        EndOfBuffer,
        /// Move down a page: C-v.
        NextPage,
        /// Move up a page: M-v.
        PreviousPage,
        /// Move to the line's first character that is not a space or tab:
        /// M-m.
        BackToIndentation,
        /// Set the mark at the caret and activate it, so that motions extend
        /// the region: C-Space. Pressed again, deactivate it. After C-u, jump
        /// to the mark and take the one before it from the mark ring.
        SetMark,
        /// Swap the caret and the mark, and activate the region: C-x C-x.
        ExchangePointAndMark,
        /// Select all the text, with the caret at the start: C-x h.
        MarkWholeBuffer,
        /// Deactivate the mark and drop the prefix argument: C-g.
        KeyboardQuit,
        /// Kill to the end of the line, or the line break when only spaces
        /// are left: C-k.
        KillLine,
        /// Kill to the end of the word: M-d.
        KillWord,
        /// Kill to the start of the word: M-Backspace.
        BackwardKillWord,
        /// Kill the region: C-w.
        KillRegion,
        /// Copy the region to the kill ring: M-w.
        KillRingSave,
        /// Insert the latest kill: C-y.
        Yank,
        /// Replace the text just yanked with the kill before it: M-y.
        YankPop,
        /// Delete the character after the caret: C-d.
        DeleteChar,
        /// Delete the character before the caret, or the selection:
        /// Backspace.
        DeleteBackwardChar,
        /// Swap the characters around the caret: C-t.
        TransposeChars,
        /// Swap the words around the caret: M-t.
        TransposeWords,
        /// Upper-case to the end of the word: M-u.
        UpcaseWord,
        /// Lower-case to the end of the word: M-l.
        DowncaseWord,
        /// Capitalize to the end of the word: M-c.
        CapitalizeWord,
        /// Insert a line break after the caret: C-o.
        OpenLine,
        /// Join the line to the one before, with one space between: M-^.
        DeleteIndentation,
        /// Open the search panel, or go to the next match: C-s.
        SearchForward,
        /// Open the search panel and go to the previous match: C-r.
        SearchBackward,
        /// Save: C-x C-s. The textarea leaves it to the application, which
        /// handles it like any action.
        SaveBuffer,
        /// Save as: C-x C-w. The textarea leaves it to the application.
        WriteFile,
        /// Start a prefix argument of 4, or multiply it by 4: C-u. Digits and
        /// a minus typed next make it a number.
        UniversalArgument,
        /// Make the prefix argument negative: M--.
        NegativeArgument,
    ]
);

/// Add a digit to the prefix argument: M-0 to M-9.
#[derive(Action, Clone, PartialEq, Eq, Deserialize)]
#[action(namespace = emacs, no_json)]
pub(crate) struct DigitArgument {
    pub(crate) digit: u8,
}

/// Bind `key` with Meta: Alt, or Option on macOS, and Escape followed by the
/// key.
fn meta<A: Action + Clone>(bindings: &mut Vec<KeyBinding>, key: &str, action: A, context: &str) {
    bindings.push(bind(&format!("alt-{key}"), action.clone(), context));
    bindings.push(bind(&format!("escape {key}"), action, context));
}

/// Emacs's bindings on `platform`.
pub(super) fn bindings(platform: KeymapPlatform) -> Vec<KeyBinding> {
    let cx = Keymap::Emacs.context();
    let mut bindings = common::bindings(cx);

    // Control keys, and the arrows, Home, End and the page keys, which extend
    // the region as Emacs's motions do.
    bindings.extend([
        bind("ctrl-f", ForwardChar, cx),
        bind("right", ForwardChar, cx),
        bind("ctrl-b", BackwardChar, cx),
        bind("left", BackwardChar, cx),
        bind("ctrl-n", NextLine, cx),
        bind("down", NextLine, cx),
        bind("ctrl-p", PreviousLine, cx),
        bind("up", PreviousLine, cx),
        bind("ctrl-a", BeginningOfLine, cx),
        bind("home", BeginningOfLine, cx),
        bind("ctrl-e", EndOfLine, cx),
        bind("end", EndOfLine, cx),
        bind("ctrl-right", ForwardWord, cx),
        bind("ctrl-left", BackwardWord, cx),
        bind("ctrl-down", ForwardParagraph, cx),
        bind("ctrl-up", BackwardParagraph, cx),
        bind("ctrl-home", BeginningOfBuffer, cx),
        bind("ctrl-end", EndOfBuffer, cx),
        bind("ctrl-v", NextPage, cx),
        bind("pagedown", NextPage, cx),
        bind("pageup", PreviousPage, cx),
        bind("ctrl-space", SetMark, cx),
        bind("ctrl-@", SetMark, cx),
        bind("ctrl-g", KeyboardQuit, cx),
        bind("ctrl-x ctrl-g", KeyboardQuit, cx),
        bind("escape ctrl-g", KeyboardQuit, cx),
        bind("ctrl-x ctrl-x", ExchangePointAndMark, cx),
        bind("ctrl-x h", MarkWholeBuffer, cx),
        bind("ctrl-k", KillLine, cx),
        bind("ctrl-delete", KillWord, cx),
        bind("ctrl-backspace", BackwardKillWord, cx),
        bind("ctrl-w", KillRegion, cx),
        bind("ctrl-y", Yank, cx),
        bind("ctrl-d", DeleteChar, cx),
        bind("backspace", DeleteBackwardChar, cx),
        bind("ctrl-t", TransposeChars, cx),
        bind("ctrl-o", OpenLine, cx),
        bind(
            "ctrl-j",
            Enter {
                secondary: false,
                shift: true,
            },
            cx,
        ),
        bind("ctrl-/", Undo, cx),
        bind("ctrl-_", Undo, cx),
        bind("ctrl-x u", Undo, cx),
        bind("ctrl-?", Redo, cx),
        bind("ctrl-s", SearchForward, cx),
        bind("ctrl-r", SearchBackward, cx),
        bind("ctrl-x ctrl-s", SaveBuffer, cx),
        bind("ctrl-x ctrl-w", WriteFile, cx),
        bind("ctrl-u", UniversalArgument, cx),
    ]);

    let bindings_ = &mut bindings;
    meta(bindings_, "f", ForwardWord, cx);
    meta(bindings_, "right", ForwardWord, cx);
    meta(bindings_, "b", BackwardWord, cx);
    meta(bindings_, "left", BackwardWord, cx);
    meta(bindings_, "e", ForwardSentence, cx);
    meta(bindings_, "a", BackwardSentence, cx);
    meta(bindings_, "}", ForwardParagraph, cx);
    meta(bindings_, "{", BackwardParagraph, cx);
    meta(bindings_, "<", BeginningOfBuffer, cx);
    meta(bindings_, ">", EndOfBuffer, cx);
    meta(bindings_, "v", PreviousPage, cx);
    meta(bindings_, "m", BackToIndentation, cx);
    meta(bindings_, "d", KillWord, cx);
    meta(bindings_, "backspace", BackwardKillWord, cx);
    meta(bindings_, "w", KillRingSave, cx);
    meta(bindings_, "y", YankPop, cx);
    meta(bindings_, "t", TransposeWords, cx);
    meta(bindings_, "u", UpcaseWord, cx);
    meta(bindings_, "l", DowncaseWord, cx);
    meta(bindings_, "c", CapitalizeWord, cx);
    meta(bindings_, "^", DeleteIndentation, cx);
    meta(bindings_, "-", NegativeArgument, cx);
    for digit in 0..10 {
        meta(bindings_, &digit.to_string(), DigitArgument { digit }, cx);
    }

    if platform.is_macos() {
        bindings.extend([
            bind("cmd-a", SelectAll, cx),
            bind("cmd-c", Copy, cx),
            bind("cmd-x", Cut, cx),
            bind("cmd-v", Paste, cx),
            bind("cmd-z", Undo, cx),
            bind("cmd-shift-z", Redo, cx),
        ]);
    }
    bindings
}

/// How many marks the mark ring keeps besides the mark.
const MARK_RING_MAX: usize = 16;

/// The largest prefix argument.
const MAX_COUNT: i64 = 100_000;

/// What Emacs keeps for one textarea: the mark and the mark ring, the kill
/// ring and the prefix argument being typed.
///
/// Reach it with [`TextareaState::keymap_state`].
#[derive(Debug, Default)]
pub struct EmacsState {
    mark: Option<usize>,
    mark_active: bool,
    mark_ring: VecDeque<usize>,
    kill_ring: KillRing,
    prefix: Option<Prefix>,
    last: Option<LastCommand>,
}

impl EmacsState {
    /// The mark, as a byte offset, once one was set. It moves with edits.
    pub fn mark(&self) -> Option<usize> {
        self.mark
    }

    /// The kill ring, the latest kill first.
    pub fn kill_ring(&self) -> impl Iterator<Item = &str> {
        self.kill_ring.iter()
    }

    /// Set the mark at `offset`, keeping the previous one in the mark ring.
    fn push_mark(&mut self, offset: usize) {
        if let Some(mark) = self.mark.replace(offset) {
            self.mark_ring.push_front(mark);
            self.mark_ring.truncate(MARK_RING_MAX);
        }
    }
}

impl KeymapState for EmacsState {
    /// The prefix argument while it is typed, such as `C-u 4`.
    fn mode_label(&self) -> Option<SharedString> {
        self.prefix.as_ref().map(|prefix| prefix.label().into())
    }

    /// Edits deactivate the mark, and the marks move with the text.
    fn adjust_for_edit(&mut self, range: &Range<usize>, new_len: usize) {
        self.mark_active = false;
        let adjust = |offset: &mut usize| {
            if *offset > range.start {
                *offset = if *offset >= range.end {
                    *offset - range.len() + new_len
                } else {
                    range.start
                };
            }
        };
        if let Some(mark) = &mut self.mark {
            adjust(mark);
        }
        self.mark_ring.iter_mut().for_each(adjust);
    }
}

/// The text's revision and the selections. Two equal fingerprints mean
/// nothing was edited or moved in between.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    revision: u64,
    selections: Vec<(usize, usize, bool)>,
}

impl Fingerprint {
    fn of(state: &TextareaState) -> Self {
        Self {
            revision: state.document_revision,
            selections: state
                .selections
                .iter()
                .map(|selection| (selection.start, selection.end, selection.reversed))
                .collect(),
        }
    }
}

/// What a command leaves for the next one.
#[derive(Clone, Debug)]
enum Last {
    Kill,
    /// The text each cursor yanked, for Meta-Y to replace.
    Yank {
        ranges: Vec<Range<usize>>,
        caret_at_start: bool,
    },
    SetMark,
}

#[derive(Debug)]
struct LastCommand {
    last: Last,
    fingerprint: Fingerprint,
}

/// A prefix argument: C-u pressed `universal` times, then digits or a minus.
#[derive(Clone, Debug)]
struct Prefix {
    universal: u32,
    digits: Option<i64>,
    negative: bool,
    /// Whether digits and a minus typed now go into it.
    open: bool,
    fingerprint: Fingerprint,
}

impl Prefix {
    fn new(fingerprint: Fingerprint) -> Self {
        Self {
            universal: 0,
            digits: None,
            negative: false,
            open: true,
            fingerprint,
        }
    }

    fn count(&self) -> i64 {
        let count = match self.digits {
            Some(digits) => digits,
            None if self.negative => 1,
            None => 4_i64.saturating_pow(self.universal),
        };
        let count = count.min(MAX_COUNT);
        if self.negative { -count } else { count }
    }

    /// C-u on its own, which some commands read as a flag: C-u C-Space jumps
    /// to the mark, C-u C-y leaves the caret before the text.
    fn is_plain(&self) -> bool {
        self.universal > 0 && self.digits.is_none() && !self.negative
    }

    fn push_digit(&mut self, digit: u8) {
        let digits = self.digits.unwrap_or(0) * 10 + i64::from(digit);
        self.digits = Some(digits.min(MAX_COUNT));
    }

    fn label(&self) -> String {
        let sign = if self.negative { "-" } else { "" };
        match self.digits {
            Some(digits) => format!("C-u {sign}{digits}"),
            None if self.negative => "C-u -".into(),
            None => format!("C-u {}", self.count()),
        }
    }
}

/// What a command reads as it starts: its prefix argument and what the
/// command before it left.
struct Invocation {
    prefix: Option<Prefix>,
    last: Option<Last>,
}

impl Invocation {
    fn count(&self) -> i64 {
        self.prefix.as_ref().map_or(1, Prefix::count)
    }

    fn is_plain_prefix(&self) -> bool {
        self.prefix.as_ref().is_some_and(Prefix::is_plain)
    }
}

fn emacs(state: &TextareaState) -> Option<&EmacsState> {
    state.keymap_state()
}

fn emacs_mut(state: &mut TextareaState) -> Option<&mut EmacsState> {
    state.keymap_state_mut()
}

/// Take the prefix argument and what the last command left, when nothing
/// changed since. `None` when the textarea follows another scheme.
fn begin(state: &mut TextareaState) -> Option<Invocation> {
    let fingerprint = Fingerprint::of(state);
    let emacs = emacs_mut(state)?;
    let prefix = emacs
        .prefix
        .take()
        .filter(|prefix| prefix.fingerprint == fingerprint);
    let last = emacs
        .last
        .take()
        .filter(|last| last.fingerprint == fingerprint)
        .map(|last| last.last);
    Some(Invocation { prefix, last })
}

/// Record what a command did, for the next one.
fn finish(state: &mut TextareaState, last: Last) {
    let fingerprint = Fingerprint::of(state);
    if let Some(emacs) = emacs_mut(state) {
        emacs.last = Some(LastCommand { last, fingerprint });
    }
}

/// Whether the mark is active, so that motions extend the region. A click
/// or anything else that moved the selection's anchor off the mark ends it.
fn region_active(state: &TextareaState) -> bool {
    let Some(emacs) = emacs(state) else {
        return false;
    };
    let selection = state.active_selection();
    let anchor = if selection.reversed {
        selection.end
    } else {
        selection.start
    };
    emacs.mark_active && emacs.mark == Some(anchor)
}

/// The region: the selections, or the text between the caret and the mark
/// when nothing is selected.
fn region(state: &TextareaState) -> Vec<Range<usize>> {
    let mut selected: Vec<Range<usize>> = state
        .selections
        .iter()
        .filter(|selection| !selection.is_empty())
        .map(|selection| selection.start..selection.end)
        .collect();
    if selected.is_empty()
        && let Some(mark) = emacs(state).and_then(EmacsState::mark)
    {
        let (mark, caret) = (mark.min(state.text.len()), state.cursor());
        selected.push(mark.min(caret)..mark.max(caret));
    }
    selected.retain(|range| !range.is_empty());
    selected.sort_by_key(|range| range.start);
    selected
}

/// Collapse every selection to its caret.
fn collapse_selections(state: &mut TextareaState) {
    let selections = state
        .selections
        .iter()
        .map(|selection| {
            let mut collapsed = *selection;
            collapsed.place_at(selection.cursor_offset(), selection.column_anchor);
            collapsed
        })
        .collect();
    state.selections.replace_all(selections);
    state.selections.merge_overlapping();
}

/// Deactivate the mark and collapse the selections.
fn deactivate_mark(state: &mut TextareaState, cx: &mut Context<TextareaState>) {
    if let Some(emacs) = emacs_mut(state) {
        emacs.mark_active = false;
    }
    state.undo_manager.break_transaction_coalescing();
    collapse_selections(state);
    cx.notify();
}

/// Select from `anchor` to `caret`, as one selection.
fn set_region(
    state: &mut TextareaState,
    anchor: usize,
    caret: usize,
    cx: &mut Context<TextareaState>,
) {
    state.undo_manager.break_transaction_coalescing();
    state.selections.remove_all_but_active();
    state.set_selection(anchor.min(caret), anchor.max(caret));
    state.active_selection_mut().reversed = caret < anchor;
    state.update_preferred_column();
    state.scroll_to(caret, None, cx);
    cx.notify();
}

fn set_mark(
    state: &mut TextareaState,
    invocation: Invocation,
    _: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    if invocation.is_plain_prefix() {
        pop_to_mark(state, cx);
        return;
    }
    let caret = state.cursor();
    let Some(emacs) = emacs_mut(state) else {
        return;
    };
    if matches!(invocation.last, Some(Last::SetMark)) && emacs.mark_active {
        emacs.mark_active = false;
    } else {
        emacs.push_mark(caret);
        emacs.mark_active = true;
        collapse_selections(state);
    }
    finish(state, Last::SetMark);
    cx.notify();
}

/// C-u C-Space: move to the mark, and take the one before it from the ring.
fn pop_to_mark(state: &mut TextareaState, cx: &mut Context<TextareaState>) {
    let Some(emacs) = emacs_mut(state) else {
        return;
    };
    let Some(mark) = emacs.mark else {
        return;
    };
    emacs.mark_active = false;
    emacs.mark_ring.push_back(mark);
    emacs.mark = emacs.mark_ring.pop_front();
    let mark = mark.min(state.text.len());
    state.move_to(mark, None, cx);
}

fn exchange_point_and_mark(
    state: &mut TextareaState,
    _: Invocation,
    _: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let caret = state.cursor();
    let len = state.text.len();
    let Some(emacs) = emacs_mut(state) else {
        return;
    };
    let Some(mark) = emacs.mark else {
        return;
    };
    emacs.mark = Some(caret);
    emacs.mark_active = true;
    set_region(state, caret, mark.min(len), cx);
}

fn mark_whole_buffer(
    state: &mut TextareaState,
    _: Invocation,
    _: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let (caret, len) = (state.cursor(), state.text.len());
    let Some(emacs) = emacs_mut(state) else {
        return;
    };
    emacs.push_mark(caret);
    emacs.push_mark(len);
    emacs.mark_active = true;
    set_region(state, len, 0, cx);
}

fn keyboard_quit(
    state: &mut TextareaState,
    _: Invocation,
    _: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    state.selections.remove_all_but_active();
    deactivate_mark(state, cx);
    state.close_suggestions(cx);
}

fn search(
    state: &mut TextareaState,
    forward: bool,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    if !state.searchable {
        // A custom search UI listens for `Search`.
        window.dispatch_action(Box::new(Search), cx);
        return;
    }
    let session = state.search_session();
    let resume = session.is_active() && !session.query.is_empty();
    state.open_search(false, cx);
    if forward {
        if resume {
            state.next_search_match(cx);
        }
    } else if !state.search_session().query.is_empty() {
        state.previous_search_match(cx);
    }
}

/// The prefix argument being typed, started afresh when another command ran
/// since.
fn prefix(state: &mut TextareaState) -> Option<&mut Prefix> {
    let fingerprint = Fingerprint::of(state);
    let emacs = emacs_mut(state)?;
    if emacs
        .prefix
        .as_ref()
        .is_none_or(|prefix| prefix.fingerprint != fingerprint)
    {
        emacs.prefix = Some(Prefix::new(fingerprint));
    }
    emacs.prefix.as_mut()
}

fn universal_argument(state: &mut TextareaState, cx: &mut Context<TextareaState>) {
    let Some(prefix) = prefix(state) else {
        cx.propagate();
        return;
    };
    if prefix.digits.is_none() && !prefix.negative {
        prefix.universal += 1;
    } else {
        prefix.open = false;
    }
    cx.notify();
}

fn digit_argument(state: &mut TextareaState, digit: u8, cx: &mut Context<TextareaState>) {
    let Some(prefix) = prefix(state) else {
        cx.propagate();
        return;
    };
    if !prefix.open {
        *prefix = Prefix::new(prefix.fingerprint.clone());
    }
    prefix.push_digit(digit);
    cx.notify();
}

fn negative_argument(state: &mut TextareaState, cx: &mut Context<TextareaState>) {
    let Some(prefix) = prefix(state) else {
        cx.propagate();
        return;
    };
    if !prefix.open || prefix.digits.is_some() {
        *prefix = Prefix::new(prefix.fingerprint.clone());
    }
    prefix.negative = true;
    cx.notify();
}

/// The state Emacs keeps for one textarea.
pub(super) fn new_state() -> Option<Box<dyn KeymapState>> {
    Some(Box::<EmacsState>::default())
}

type Command = fn(&mut TextareaState, Invocation, &mut Window, &mut Context<TextareaState>);

/// Run `command` for the action `A`, while the textarea follows Emacs.
fn on<A: Action>(
    element: Stateful<Div>,
    entity: &Entity<TextareaState>,
    window: &mut Window,
    command: Command,
) -> Stateful<Div> {
    element.on_action(
        window.listener_for(entity, move |state, _: &A, window, cx| {
            let Some(invocation) = begin(state) else {
                cx.propagate();
                return;
            };
            if invocation.prefix.is_some() {
                cx.notify();
            }
            command(state, invocation, window, cx);
            // The engine passes a key on at the edge of the text, for an ancestor
            // to use. In Emacs the key stays the command's.
            cx.stop_propagation();
        }),
    )
}

/// Registers the actions only Emacs has.
pub(super) fn register_actions(
    element: Stateful<Div>,
    entity: &Entity<TextareaState>,
    window: &mut Window,
) -> Stateful<Div> {
    macro_rules! commands {
        ($element:expr, $($action:ty => $command:expr),* $(,)?) => {{
            let element = $element;
            $(let element = on::<$action>(element, entity, window, $command);)*
            element
        }};
    }
    macro_rules! motions {
        ($element:expr, $($motion:ident),* $(,)?) => {
            commands!($element, $($motion => |state, invocation, window, cx| {
                motion::run(state, invocation, Motion::$motion, window, cx)
            }),*)
        };
    }

    let element = motions!(
        element,
        ForwardChar,
        BackwardChar,
        NextLine,
        PreviousLine,
        ForwardWord,
        BackwardWord,
        BeginningOfLine,
        EndOfLine,
        ForwardSentence,
        BackwardSentence,
        ForwardParagraph,
        BackwardParagraph,
        BeginningOfBuffer,
        EndOfBuffer,
        NextPage,
        PreviousPage,
        BackToIndentation,
    );
    let element = commands!(
        element,
        SetMark => set_mark,
        ExchangePointAndMark => exchange_point_and_mark,
        MarkWholeBuffer => mark_whole_buffer,
        KeyboardQuit => keyboard_quit,
        KillLine => kill::kill_line,
        KillWord => |state, invocation, window, cx| {
            kill::kill_word(state, invocation, true, window, cx)
        },
        BackwardKillWord => |state, invocation, window, cx| {
            kill::kill_word(state, invocation, false, window, cx)
        },
        KillRegion => kill::kill_region,
        KillRingSave => kill::kill_ring_save,
        Yank => kill::yank,
        YankPop => kill::yank_pop,
        DeleteChar => |state, invocation, window, cx| {
            edit::delete_char(state, invocation, true, window, cx)
        },
        DeleteBackwardChar => |state, invocation, window, cx| {
            edit::delete_char(state, invocation, false, window, cx)
        },
        TransposeChars => edit::transpose_chars,
        TransposeWords => edit::transpose_words,
        UpcaseWord => |state, invocation, window, cx| {
            edit::convert_words(state, invocation, str::to_uppercase, window, cx)
        },
        DowncaseWord => |state, invocation, window, cx| {
            edit::convert_words(state, invocation, str::to_lowercase, window, cx)
        },
        CapitalizeWord => |state, invocation, window, cx| {
            edit::convert_words(state, invocation, super::commands::title_case, window, cx)
        },
        OpenLine => edit::open_line,
        DeleteIndentation => edit::delete_indentation,
        SearchForward => |state, _, window, cx| search(state, true, window, cx),
        SearchBackward => |state, _, window, cx| search(state, false, window, cx),
    );
    element
        .on_action(
            window.listener_for(entity, |state, _: &UniversalArgument, _, cx| {
                universal_argument(state, cx)
            }),
        )
        .on_action(
            window.listener_for(entity, |state, _: &NegativeArgument, _, cx| {
                negative_argument(state, cx)
            }),
        )
        .on_action(
            window.listener_for(entity, |state, action: &DigitArgument, _, cx| {
                digit_argument(state, action.digit, cx)
            }),
        )
}

/// Text typed while Emacs is active, before it is inserted. Digits and a
/// minus go into a prefix argument being typed, and a prefix argument repeats
/// the text. Returns whether Emacs took it.
pub(super) fn typed_text(
    state: &mut TextareaState,
    text: &str,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) -> bool {
    let fingerprint = Fingerprint::of(state);
    let composing = state.ime_marked_range.is_some();
    let Some(emacs) = emacs_mut(state) else {
        return false;
    };
    let Some(mut prefix) = emacs
        .prefix
        .take()
        .filter(|prefix| prefix.fingerprint == fingerprint)
    else {
        return false;
    };
    cx.notify();
    if prefix.open {
        if let Some(digit) = single_digit(text) {
            prefix.push_digit(digit);
            emacs.prefix = Some(prefix);
            return true;
        }
        if text == "-" && prefix.digits.is_none() {
            prefix.negative = true;
            emacs.prefix = Some(prefix);
            return true;
        }
    }
    let count = prefix.count();
    if composing || count == 1 {
        return false;
    }
    if count > 1 {
        state.replace_text_in_range(None, &text.repeat(count as usize), window, cx);
    }
    true
}

fn single_digit(text: &str) -> Option<u8> {
    let mut chars = text.chars();
    let digit = chars.next()?.to_digit(10)?;
    chars.next().is_none().then_some(digit as u8)
}
