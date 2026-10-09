//! Vim's modes and keys.
//!
//! The scheme keeps a [`VimState`] per textarea: the mode, the count,
//! operator and register being typed, the last change for `.`, the last
//! search and `f`, and the insert being typed. Its mode goes into the key
//! context as `vim_mode`, so each mode has bindings of its own, and decides
//! the caret's shape, the mode label and whether typed text is inserted.
//! Registers are shared by every textarea in the app, as Vim's are by its
//! buffers.
//!
//! | Module | What it holds |
//! |---|---|
//! | `bindings` | the binding table |
//! | `text` | lines, character classes, and a cursor that steps the way Vim's does |
//! | `motion` | motions: where each one goes and how an operator takes it |
//! | `object` | text objects |
//! | `operator` | operators and the regions they act on |
//! | `normal` | counts, normal-mode commands, undo and `.` |
//! | `insert` | insert and replace modes |
//! | `visual` | the visual modes |
//! | `register` | registers |
//! | `search` | `/`, `?`, `n`, `*` and Vim's patterns |
//! | `ex` | the command line and its commands |
//!
//! Every action runs with the state taken out of the textarea, through
//! [`update`], so a command can borrow both. Before each render and each
//! action, the state catches up with what changed without it, such as a
//! selection made with the mouse.

mod bindings;
mod ex;
mod insert;
mod motion;
mod normal;
mod object;
mod operator;
mod register;
mod search;
#[cfg(test)]
mod tests;
mod text;
mod visual;

use gpui::{
    Action, Context, Div, Entity, InteractiveElement as _, KeyContext, SharedString, Stateful,
    Window, actions,
};

use super::{CursorShape, KeymapState};
use crate::input::TextareaState;

pub(super) use bindings::bindings;
use ex::{CommandKind, CommandLine};
pub use ex::{VimCommand, VimQuit, VimWrite};
use insert::{InsertAt, InsertSession};
use motion::Motion;
use normal::{Change, Command, Target};
use object::Object;
use operator::Operator;
use search::LastSearch;
use visual::{Visual, VisualKind};

/// The largest count Vim takes.
const MAX_COUNT: usize = 99_999;

actions!(
    vim,
    [
        /// Escape: leave insert, replace or visual mode, or drop a
        /// half-typed command.
        SwitchToNormalMode,
        /// `i`: insert before the caret.
        SwitchToInsertMode,
        /// `a`: insert after the caret.
        InsertAfter,
        /// `I`: insert before the line's first non-blank.
        InsertFirstNonBlank,
        /// `gI`: insert at the line's start.
        InsertLineStart,
        /// `A`: insert at the line's end.
        InsertEnd,
        /// `o`: open a line below.
        InsertLineBelow,
        /// `O`: open a line above.
        InsertLineAbove,
        /// `R`: replace mode.
        ReplaceMode,
        /// `v`
        ToggleVisual,
        /// `V`
        ToggleVisualLine,
        /// Ctrl-V
        ToggleVisualBlock,
        /// `gv`: select the last visual selection again.
        ReselectVisual,
        /// `o` in visual mode: go to the other end.
        SwapVisualEnds,
        /// `O` in visual block mode: go to the other end of the row.
        SwapVisualCorners,
        /// `u`
        Undo,
        /// Ctrl-R
        Redo,
        /// `.`: repeat the last change.
        Repeat,
        /// `x`
        DeleteChar,
        /// `X`
        DeleteCharBefore,
        /// `D`
        DeleteToEnd,
        /// `C`
        ChangeToEnd,
        /// `Y`: yank lines.
        YankLine,
        /// `s`
        Substitute,
        /// `S`
        SubstituteLine,
        /// `~`
        ToggleCaseChar,
        /// `J`
        Join,
        /// `gJ`
        JoinWithoutSpaces,
        /// `r`: replace characters with the next one typed.
        ReplaceChar,
        /// `"`: use the register named next.
        SelectRegister,
        /// `:`
        OpenCommandLine,
        /// Enter on the command line.
        CommandLineExecute,
        /// Backspace on the command line.
        CommandLineBackspace,
        /// Ctrl-U on the command line.
        CommandLineClear,
        /// Ctrl-W on the command line.
        CommandLineDeleteWord,
        /// `I` in visual block mode.
        VisualInsert,
        /// `A` in visual block mode.
        VisualAppend,
        /// `X` and `D` in visual mode.
        VisualDeleteLines,
        /// `D` in visual block mode.
        VisualDeleteToLineEnd,
        /// `S`, `R` and `C` in visual mode.
        VisualChangeLines,
        /// `C` in visual block mode.
        VisualChangeToLineEnd,
        /// `Y` in visual mode.
        VisualYankLines,
        /// Backspace in replace mode: put back what was replaced.
        ReplaceBackspace,
        /// Ctrl-U in insert mode.
        DeleteToLineStart,
        /// Ctrl-R in insert mode: insert a register.
        InsertRegister,
        /// Ctrl-V in insert mode: insert the next character as it is.
        InsertLiteral,
        /// Ctrl-T in insert mode.
        IndentLine,
        /// Ctrl-D in insert mode.
        OutdentLine,
        /// `ZZ`
        WriteAndQuit,
        /// Select all the text in visual mode.
        SelectAll,
        /// Copy the selection to the clipboard.
        CopyToClipboard,
        /// Cut the selection to the clipboard.
        CutToClipboard,
        /// Paste the clipboard.
        PasteFromClipboard,
    ]
);

/// A motion key.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = vim, no_json)]
pub(crate) struct Move {
    motion: Motion,
}

/// A text object after `i` or `a`.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = vim, no_json)]
pub(crate) struct SelectObject {
    object: Object,
    around: bool,
}

/// An operator key.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = vim, no_json)]
pub(crate) struct PushOperator {
    operator: Operator,
}

/// A digit of a count.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = vim, no_json)]
pub(crate) struct Digit {
    digit: usize,
}

/// `f`, `F`, `t` and `T`.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = vim, no_json)]
pub(crate) struct FindChar {
    backward: bool,
    till: bool,
}

/// `p`, `P`, `gp` and `gP`.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = vim, no_json)]
pub(crate) struct Put {
    before: bool,
    move_past: bool,
}

/// `/` and `?`.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = vim, no_json)]
pub(crate) struct StartSearch {
    backward: bool,
}

/// Ctrl-A and Ctrl-X.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = vim, no_json)]
pub(crate) struct IncrementNumber {
    step: i64,
}

/// A character typed with a key of its own, such as Enter after `r`.
#[derive(Action, Clone, Debug, PartialEq, Eq)]
#[action(namespace = vim, no_json)]
pub(crate) struct InputChar {
    character: char,
}

/// Vim's modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VimMode {
    /// Keys are commands. Typed text is not inserted.
    #[default]
    Normal,
    /// Keys insert text.
    Insert,
    /// Typed text replaces the text under the caret.
    Replace,
    /// A characterwise selection.
    Visual,
    /// A selection of whole lines.
    VisualLine,
    /// A selection of the same columns on several lines.
    VisualBlock,
    /// An operator such as `d` waits for its motion or text object.
    OperatorPending,
}

impl VimMode {
    /// The value of `vim_mode` in the key context: `normal`, `insert`,
    /// `replace`, `visual` for every visual mode, or `operator`. While Vim
    /// waits for a character, such as after `f`, it is `waiting`, and while a
    /// command line is open, `command`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Insert => "insert",
            Self::Replace => "replace",
            Self::Visual | Self::VisualLine | Self::VisualBlock => "visual",
            Self::OperatorPending => "operator",
        }
    }

    /// The mode's label: `NORMAL`, `INSERT`, `VISUAL LINE` and so on.
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Replace => "REPLACE",
            Self::Visual => "VISUAL",
            Self::VisualLine => "VISUAL LINE",
            Self::VisualBlock => "VISUAL BLOCK",
            Self::OperatorPending => "O-PENDING",
        }
    }

    pub fn is_visual(self) -> bool {
        matches!(self, Self::Visual | Self::VisualLine | Self::VisualBlock)
    }
}

/// What a key that takes the next character is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Awaiting {
    /// `f`, `F`, `t`, `T`
    Find { backward: bool, till: bool },
    /// `r`
    ReplaceChar,
    /// `"`
    Register,
    /// Ctrl-R in insert mode.
    InsertRegister,
    /// Ctrl-V in insert mode.
    Literal,
}

/// What Vim keeps for one textarea.
#[derive(Default)]
pub struct VimState {
    mode: VimMode,
    /// The count typed before an operator.
    operator_count: Option<usize>,
    /// The count being typed.
    count: Option<usize>,
    operator: Option<Operator>,
    register: Option<char>,
    awaiting: Option<Awaiting>,
    command_line: Option<CommandLine>,
    visual: Option<Visual>,
    /// The last visual selection, for `gv` and `'<,'>`.
    last_visual: Option<Visual>,
    /// The selections visual mode last made, to notice others.
    selected: Option<Vec<(usize, usize, bool)>>,
    /// The column `j` and `k` aim for, and the offset it was set at.
    want: Option<(usize, usize)>,
    last_find: Option<Motion>,
    last_search: Option<LastSearch>,
    last_change: Option<Change>,
    /// A change that went on into insert mode, kept when the insert ends.
    pending_change: Option<Change>,
    last_inserted: Option<String>,
    last_command: Option<String>,
    insert: Option<InsertSession>,
    /// The undo history when the running command began.
    command_mark: usize,
}

impl VimState {
    pub fn mode(&self) -> VimMode {
        self.mode
    }

    /// The command line being typed, with its `:`, `/` or `?`.
    pub fn command_line(&self) -> Option<String> {
        self.command_line.as_ref().map(CommandLine::label)
    }

    fn read_register(&self, name: Option<char>, cx: &mut gpui::App) -> Option<register::Register> {
        let chars =
            |text: &String| register::Register::new(text.clone(), register::RegisterKind::Chars);
        match name {
            Some('.') => self.last_inserted.as_ref().map(chars),
            Some('/') => self
                .last_search
                .as_ref()
                .map(|search| chars(&search.pattern)),
            Some(':') => self.last_command.as_ref().map(chars),
            name => register::read(name, cx),
        }
    }

    /// Catch up with what changed outside Vim: a selection made with the
    /// mouse starts visual mode, and normal mode's caret stays on a
    /// character.
    fn sync(&mut self, state: &mut TextareaState, cx: &mut Context<TextareaState>) {
        match self.mode {
            VimMode::Insert | VimMode::Replace => {}
            VimMode::Visual | VimMode::VisualLine | VimMode::VisualBlock => {
                if !self.visual_is_current(state) {
                    if state.active_selection().is_empty() {
                        self.leave_visual();
                        self.enter_normal(state, cx);
                    } else {
                        self.visual_from_selection(state, cx);
                    }
                }
            }
            VimMode::Normal | VimMode::OperatorPending => {
                if !state.active_selection().is_empty() {
                    self.reset_pending();
                    self.visual_from_selection(state, cx);
                } else {
                    let caret = state.cursor();
                    let normal = text::normal_offset(&state.text, caret);
                    if normal != caret {
                        state.set_cursor_to(normal);
                    }
                }
            }
        }
    }

    /// Escape.
    fn escape(
        &mut self,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        if self.command_line.is_some() {
            self.cancel_command_line();
            return;
        }
        if self.awaiting.take().is_some() {
            if !matches!(self.mode, VimMode::Insert | VimMode::Replace) {
                self.reset_pending();
            }
            return;
        }
        match self.mode {
            VimMode::Insert | VimMode::Replace => {
                // An open suggestion goes first.
                if state.handle_suggestion_action(&crate::input::Escape, window, cx) {
                    return;
                }
                self.leave_insert(state, window, cx);
            }
            VimMode::Visual | VimMode::VisualLine | VimMode::VisualBlock => {
                let head = self.visual.map_or(state.cursor(), |visual| visual.head);
                self.leave_visual();
                self.enter_normal_at(head, state, cx);
            }
            VimMode::OperatorPending => self.reset_pending(),
            VimMode::Normal => {
                if self.count.is_some() || self.register.is_some() {
                    self.reset_pending();
                } else {
                    state.escape(&crate::input::Escape, window, cx);
                }
            }
        }
    }

    /// Text typed while Vim is not inserting it.
    fn typed(
        &mut self,
        typed: &str,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        if let Some(line) = &mut self.command_line {
            line.text.push_str(typed);
            return;
        }
        if let Some(awaiting) = self.awaiting.take() {
            if let Some(character) = typed.chars().next() {
                self.complete(awaiting, character, state, window, cx);
            }
            return;
        }
        match self.mode {
            VimMode::Replace => self.replace_typed(typed, state, window, cx),
            VimMode::Insert => {}
            // A key that means nothing drops the command typed so far.
            _ => self.reset_pending(),
        }
    }

    /// The character a waiting key asked for.
    fn complete(
        &mut self,
        awaiting: Awaiting,
        character: char,
        state: &mut TextareaState,
        window: &mut Window,
        cx: &mut Context<TextareaState>,
    ) {
        match awaiting {
            Awaiting::Find { backward, till } => {
                let motion = Motion::Find {
                    character,
                    backward,
                    till,
                };
                self.motion(motion, state, window, cx);
            }
            Awaiting::ReplaceChar => {
                if self.mode.is_visual() {
                    self.visual_replace(character, state, window, cx);
                } else {
                    let count = self.take_count();
                    self.execute(Command::ReplaceChars(character), count, state, window, cx);
                }
            }
            Awaiting::Register => {
                if register::is_valid(character) {
                    self.register = Some(character);
                } else {
                    self.reset_pending();
                }
            }
            Awaiting::InsertRegister => {
                if let Some(register) = self.read_register(Some(character), cx) {
                    self.insert_literal(&register.text, state, window, cx);
                }
            }
            Awaiting::Literal => {
                self.insert_literal(&character.to_string(), state, window, cx);
            }
        }
    }
}

impl KeymapState for VimState {
    fn key_context(&self, context: &mut KeyContext) {
        let mode = if self.command_line.is_some() {
            "command"
        } else if self.awaiting.is_some() {
            "waiting"
        } else {
            self.mode.name()
        };
        context.set("vim_mode", mode);
        if let Some(visual) = &self.visual {
            context.set("vim_visual", visual.kind.name());
        }
        if let Some(operator) = self.operator {
            context.set("vim_operator", operator.name());
        }
        if self.count.is_some() {
            context.add("vim_count");
        }
    }

    fn cursor_shape(&self) -> CursorShape {
        let typing = matches!(self.mode, VimMode::Insert | VimMode::Replace);
        if self.awaiting.is_some() && !typing {
            return CursorShape::Underline;
        }
        match self.mode {
            VimMode::Insert => CursorShape::Bar,
            VimMode::Replace | VimMode::OperatorPending => CursorShape::Underline,
            _ => CursorShape::Block,
        }
    }

    fn mode_label(&self) -> Option<SharedString> {
        Some(match &self.command_line {
            Some(line) => line.label().into(),
            None => self.mode.label().into(),
        })
    }

    fn accepts_text_input(&self) -> bool {
        self.command_line.is_some()
            || self.awaiting.is_some()
            || matches!(self.mode, VimMode::Insert | VimMode::Replace)
    }

    fn caret_offset(&self) -> Option<usize> {
        self.visual.map(|visual| visual.head)
    }
}

pub(super) fn new_state() -> Option<Box<dyn KeymapState>> {
    Some(Box::<VimState>::default())
}

/// Before the textarea renders, catch up with changes made around Vim, so
/// the mode and the key context are right for the next key: a selection
/// made with the mouse starts visual mode, a click leaves it, and normal
/// mode's caret steps back onto a character.
pub(super) fn on_render(state: &mut TextareaState, cx: &mut Context<TextareaState>) {
    let Some(slot) = state.keymap_state_mut::<VimState>() else {
        return;
    };
    if matches!(slot.mode, VimMode::Insert | VimMode::Replace) {
        return;
    }
    let mut vim = std::mem::take(slot);
    vim.sync(state, cx);
    if let Some(slot) = state.keymap_state_mut::<VimState>() {
        *slot = vim;
    }
}

/// Run `f` with the textarea's Vim state taken out of it. Propagates the
/// action when Vim is not the textarea's scheme.
fn update(
    state: &mut TextareaState,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
    f: impl FnOnce(&mut VimState, &mut TextareaState, &mut Window, &mut Context<TextareaState>),
) {
    let Some(slot) = state.keymap_state_mut::<VimState>() else {
        cx.propagate();
        return;
    };
    let mut vim = std::mem::take(slot);
    vim.command_mark = state.undo_manager.mark();
    vim.sync(state, cx);
    f(&mut vim, state, window, cx);
    if let Some(slot) = state.keymap_state_mut::<VimState>() {
        *slot = vim;
    }
    cx.notify();
}

/// Register `f` for the action `A`, run through [`update`].
fn on<A: Action>(
    element: Stateful<Div>,
    entity: &Entity<TextareaState>,
    window: &mut Window,
    f: fn(&mut VimState, &A, &mut TextareaState, &mut Window, &mut Context<TextareaState>),
) -> Stateful<Div> {
    element.on_action(
        window.listener_for(entity, move |state, action: &A, window, cx| {
            update(state, window, cx, |vim, state, window, cx| {
                f(vim, action, state, window, cx)
            })
        }),
    )
}

/// Registers Vim's own actions.
pub(super) fn register_actions(
    element: Stateful<Div>,
    entity: &Entity<TextareaState>,
    window: &mut Window,
) -> Stateful<Div> {
    let e = element;
    let e = on(
        e,
        entity,
        window,
        |vim, _: &SwitchToNormalMode, s, w, cx| vim.escape(s, w, cx),
    );
    let e = on(
        e,
        entity,
        window,
        |vim, _: &SwitchToInsertMode, s, w, cx| {
            let count = vim.take_count();
            vim.insert(InsertAt::Before, count, s, w, cx)
        },
    );
    let e = on(e, entity, window, |vim, _: &InsertAfter, s, w, cx| {
        let count = vim.take_count();
        vim.insert(InsertAt::After, count, s, w, cx)
    });
    let e = on(
        e,
        entity,
        window,
        |vim, _: &InsertFirstNonBlank, s, w, cx| {
            let count = vim.take_count();
            vim.insert(InsertAt::FirstNonBlank, count, s, w, cx)
        },
    );
    let e = on(e, entity, window, |vim, _: &InsertLineStart, s, w, cx| {
        let count = vim.take_count();
        vim.insert(InsertAt::LineStart, count, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &InsertEnd, s, w, cx| {
        let count = vim.take_count();
        vim.insert(InsertAt::LineEnd, count, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &InsertLineBelow, s, w, cx| {
        let count = vim.take_count();
        vim.insert(InsertAt::LineBelow, count, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &InsertLineAbove, s, w, cx| {
        let count = vim.take_count();
        vim.insert(InsertAt::LineAbove, count, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &ReplaceMode, s, w, cx| {
        let count = vim.take_count();
        vim.start_replace(count, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &ToggleVisual, s, _, cx| {
        vim.toggle_visual(VisualKind::Char, s, cx)
    });
    let e = on(e, entity, window, |vim, _: &ToggleVisualLine, s, _, cx| {
        vim.toggle_visual(VisualKind::Line, s, cx)
    });
    let e = on(e, entity, window, |vim, _: &ToggleVisualBlock, s, _, cx| {
        vim.toggle_visual(VisualKind::Block, s, cx)
    });
    let e = on(e, entity, window, |vim, _: &ReselectVisual, s, _, cx| {
        vim.reselect_visual(s, cx)
    });
    let e = on(e, entity, window, |vim, _: &SwapVisualEnds, s, _, cx| {
        vim.swap_visual_ends(false, s, cx)
    });
    let e = on(e, entity, window, |vim, _: &SwapVisualCorners, s, _, cx| {
        vim.swap_visual_ends(true, s, cx)
    });
    let e = on(e, entity, window, |vim, _: &Undo, s, w, cx| {
        vim.undo_redo(true, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &Redo, s, w, cx| {
        vim.undo_redo(false, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &Repeat, s, w, cx| {
        vim.repeat(s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &DeleteChar, s, w, cx| {
        operate(
            vim,
            Operator::Delete,
            Target::Motion(Motion::Right),
            s,
            w,
            cx,
        )
    });
    let e = on(e, entity, window, |vim, _: &DeleteCharBefore, s, w, cx| {
        operate(
            vim,
            Operator::Delete,
            Target::Motion(Motion::Left),
            s,
            w,
            cx,
        )
    });
    let e = on(e, entity, window, |vim, _: &DeleteToEnd, s, w, cx| {
        operate(
            vim,
            Operator::Delete,
            Target::Motion(Motion::LineEnd),
            s,
            w,
            cx,
        )
    });
    let e = on(e, entity, window, |vim, _: &ChangeToEnd, s, w, cx| {
        operate(
            vim,
            Operator::Change,
            Target::Motion(Motion::LineEnd),
            s,
            w,
            cx,
        )
    });
    let e = on(e, entity, window, |vim, _: &YankLine, s, w, cx| {
        operate(vim, Operator::Yank, Target::Line, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &Substitute, s, w, cx| {
        operate(
            vim,
            Operator::Change,
            Target::Motion(Motion::Right),
            s,
            w,
            cx,
        )
    });
    let e = on(e, entity, window, |vim, _: &SubstituteLine, s, w, cx| {
        operate(vim, Operator::Change, Target::Line, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &ToggleCaseChar, s, w, cx| {
        let count = vim.take_count();
        vim.execute(Command::ToggleCaseChars, count, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &Join, s, w, cx| {
        let count = vim.take_count();
        vim.join(true, count, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &JoinWithoutSpaces, s, w, cx| {
        let count = vim.take_count();
        vim.join(false, count, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &ReplaceChar, _, _, _| {
        vim.awaiting = Some(Awaiting::ReplaceChar)
    });
    let e = on(e, entity, window, |vim, _: &SelectRegister, _, _, _| {
        vim.awaiting = Some(Awaiting::Register)
    });
    let e = on(e, entity, window, |vim, _: &OpenCommandLine, s, _, cx| {
        vim.open_command_line(CommandKind::Ex, s, cx)
    });
    let e = on(
        e,
        entity,
        window,
        |vim, _: &CommandLineExecute, s, w, cx| vim.execute_command_line(s, w, cx),
    );
    let e = on(
        e,
        entity,
        window,
        |vim, _: &CommandLineBackspace, _, _, _| vim.command_line_backspace(),
    );
    let e = on(e, entity, window, |vim, _: &CommandLineClear, _, _, _| {
        vim.command_line_clear()
    });
    let e = on(
        e,
        entity,
        window,
        |vim, _: &CommandLineDeleteWord, _, _, _| vim.command_line_delete_word(),
    );
    let e = on(e, entity, window, |vim, _: &VisualInsert, s, w, cx| {
        vim.visual_block_insert(false, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &VisualAppend, s, w, cx| {
        vim.visual_block_insert(true, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &VisualDeleteLines, s, w, cx| {
        vim.visual_line_operator(Operator::Delete, false, s, w, cx)
    });
    let e = on(
        e,
        entity,
        window,
        |vim, _: &VisualDeleteToLineEnd, s, w, cx| {
            vim.visual_line_operator(Operator::Delete, true, s, w, cx)
        },
    );
    let e = on(e, entity, window, |vim, _: &VisualChangeLines, s, w, cx| {
        vim.visual_line_operator(Operator::Change, false, s, w, cx)
    });
    let e = on(
        e,
        entity,
        window,
        |vim, _: &VisualChangeToLineEnd, s, w, cx| {
            vim.visual_line_operator(Operator::Change, true, s, w, cx)
        },
    );
    let e = on(e, entity, window, |vim, _: &VisualYankLines, s, w, cx| {
        vim.visual_line_operator(Operator::Yank, false, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &ReplaceBackspace, s, w, cx| {
        vim.replace_backspace(s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &DeleteToLineStart, s, w, cx| {
        vim.delete_to_line_start(s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &InsertRegister, _, _, _| {
        vim.awaiting = Some(Awaiting::InsertRegister)
    });
    let e = on(e, entity, window, |vim, _: &InsertLiteral, _, _, _| {
        vim.awaiting = Some(Awaiting::Literal)
    });
    let e = on(e, entity, window, |vim, _: &IndentLine, s, w, cx| {
        vim.shift_insert_line(true, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &OutdentLine, s, w, cx| {
        vim.shift_insert_line(false, s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &WriteAndQuit, s, w, cx| {
        vim.run_ex("x", s, w, cx)
    });
    let e = on(e, entity, window, |vim, _: &SelectAll, s, _, cx| {
        vim.select_all(s, cx)
    });
    let e = on(e, entity, window, |vim, _: &CopyToClipboard, s, w, cx| {
        if vim.mode.is_visual() {
            vim.register = Some('+');
            vim.visual_operator(Operator::Yank, s, w, cx);
        }
    });
    let e = on(e, entity, window, |vim, _: &CutToClipboard, s, w, cx| {
        if vim.mode.is_visual() {
            vim.register = Some('+');
            vim.visual_operator(Operator::Delete, s, w, cx);
        }
    });
    let e = on(
        e,
        entity,
        window,
        |vim, _: &PasteFromClipboard, s, w, cx| {
            vim.register = Some('+');
            if vim.mode.is_visual() {
                vim.visual_put(false, s, w, cx);
            } else {
                let count = vim.take_count();
                let command = Command::Put {
                    before: true,
                    move_past: false,
                };
                vim.execute(command, count, s, w, cx);
            }
        },
    );
    let e = on(e, entity, window, |vim, action: &Move, s, w, cx| {
        vim.motion(action.motion, s, w, cx)
    });
    let e = on(e, entity, window, |vim, action: &SelectObject, s, w, cx| {
        vim.object(action.object, action.around, s, w, cx)
    });
    let e = on(e, entity, window, |vim, action: &PushOperator, s, w, cx| {
        vim.push_operator(action.operator, s, w, cx)
    });
    let e = on(e, entity, window, |vim, action: &Digit, _, _, _| {
        vim.digit(action.digit)
    });
    let e = on(e, entity, window, |vim, action: &FindChar, _, _, _| {
        vim.awaiting = Some(Awaiting::Find {
            backward: action.backward,
            till: action.till,
        })
    });
    let e = on(e, entity, window, |vim, action: &Put, s, w, cx| {
        if vim.mode.is_visual() {
            vim.visual_put(!action.before, s, w, cx);
        } else {
            let count = vim.take_count();
            let command = Command::Put {
                before: action.before,
                move_past: action.move_past,
            };
            vim.execute(command, count, s, w, cx);
        }
    });
    let e = on(e, entity, window, |vim, action: &StartSearch, s, _, cx| {
        let kind = CommandKind::Search {
            backward: action.backward,
        };
        vim.open_command_line(kind, s, cx)
    });
    let e = on(
        e,
        entity,
        window,
        |vim, action: &IncrementNumber, s, w, cx| {
            let count = vim.take_count();
            vim.execute(Command::Increment(action.step), count, s, w, cx)
        },
    );
    on(e, entity, window, |vim, action: &InputChar, s, w, cx| {
        let text = action.character.to_string();
        vim.typed(&text, s, w, cx)
    })
}

/// A command made of an operator and its target, such as `x` for `dl`.
fn operate(
    vim: &mut VimState,
    operator: Operator,
    target: Target,
    state: &mut TextareaState,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let count = vim.take_count();
    vim.execute(
        Command::Operate { operator, target },
        count,
        state,
        window,
        cx,
    );
}

/// Text typed while Vim is active, before it is inserted. Vim takes it
/// unless it is inserting: a command line or a key waiting for a character
/// reads it, replace mode types over the text, and in the other modes a key
/// without a binding inserts nothing.
pub(super) fn typed_text(
    state: &mut TextareaState,
    typed: &str,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) -> bool {
    let Some(vim) = state.keymap_state::<VimState>() else {
        return false;
    };
    if vim.mode == VimMode::Insert && vim.command_line.is_none() && vim.awaiting.is_none() {
        return false;
    }
    update(state, window, cx, |vim, state, window, cx| {
        vim.typed(typed, state, window, cx)
    });
    true
}
