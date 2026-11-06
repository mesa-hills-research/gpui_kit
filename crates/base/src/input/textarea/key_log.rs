//! A record of the editing in one textarea, with times, for studying how
//! someone types and how word suggestions keep up.
//!
//! Nothing is recorded unless an application attaches a [`KeyLog`] to a
//! textarea with [`TextareaState::key_log`]. The record goes to one JSON
//! Lines file on this machine. It starts at the first edit, with the text as
//! it stood when the log was attached, so a reader can rebuild the text event
//! by event. That first line, the header, also holds the suggestion mode and
//! any fields the application adds with [`KeyLog::header`].
//!
//! Every event has `t`, milliseconds since the log was attached, and `k`, its
//! kind:
//!
//! - An edit has `at` (the byte offset it starts at), `n` (bytes removed),
//!   `s` (the text inserted) and `len` (the text's length after it, in
//!   bytes). Its kind says where it came from: `type`, `compose` (text an
//!   input method is still composing), `ime` (its commit), `suggestion`,
//!   `correction` (a spelling fix), `paste`, `cut`, `undo`, `redo`,
//!   `backspace`, `delete`, `backspace-word`, `delete-word`,
//!   `backspace-line`, `delete-line`, `enter`, or `edit` for anything else.
//! - `move` is the caret moving other than by an edit, with `c` and, for a
//!   selection, `sel`. It's noted when the textarea next draws, at most a
//!   frame after the key or click.
//! - `focus` and `blur` follow the textarea's focus and its window's.
//! - `shown` is a suggestion appearing or changing, with `s` (the text shown
//!   after the caret), `mode` and `pause_ms` (the wait before the request
//!   that brought it). A shown suggestion ends with exactly one of
//!   `accepted` (with `key`, `tab` or `word`, and `s`, the text inserted),
//!   `dismissed` (Escape), `typed-along` (a typed key that continues it),
//!   `typed-past` (a typed key that doesn't), or `hidden` (any other edit,
//!   a caret move or losing focus).
//! - `end` is the textarea closing.

use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;

use gpui::{Context, Window};
use serde_json::{Value, json};
use web_time::{Instant, SystemTime, UNIX_EPOCH};

use super::{EditOrigin, InputBaseState, InputModeKind, Rope, TextareaMode};

/// Lines kept from before the first edit, to write with it.
const MAX_PENDING: usize = 64;

/// A record of one textarea's editing. See the [module docs](self).
pub struct KeyLog {
    path: PathBuf,
    file: Option<File>,
    /// Set after a write fails, which ends the record.
    failed: bool,
    started: Instant,
    started_unix_ms: u128,
    mode: Box<dyn Fn() -> String>,
    /// The application's own fields for the header.
    header: Vec<(&'static str, String)>,
    /// The text and caret when the log was attached, for the header.
    text: Rope,
    caret: usize,
    /// Lines from before the first edit.
    pending: Vec<String>,
    /// The selection as the log last knew it, as (start, end, head).
    selection: (usize, usize, usize),
    /// The kind of the edits of the action running now, when it names them.
    pub(super) kind: Option<&'static str>,
    /// The wait before the latest request for suggestions.
    pause_ms: u128,
    /// The suggestion shown after the caret, as last recorded.
    ghost: Option<String>,
    /// A shown suggestion that closed as a key was typed, which that key's
    /// edit settles as typed along or typed past.
    typed_over: Option<String>,
}

impl KeyLog {
    /// A log written to `path`, a file that doesn't exist yet. It's created
    /// at the first edit.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            file: None,
            failed: false,
            started: Instant::now(),
            started_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_millis()),
            mode: Box::new(String::new),
            header: Vec::new(),
            text: Rope::new(),
            caret: 0,
            pending: Vec::new(),
            selection: (0, 0, 0),
            kind: None,
            pause_ms: 0,
            ghost: None,
            typed_over: None,
        }
    }

    /// Names the suggestion mode, asked again at each shown suggestion.
    pub fn mode(mut self, mode: impl Fn() -> String + 'static) -> Self {
        self.mode = Box::new(mode);
        self
    }

    /// Adds `value` to the header under `key`, for anything else a reader
    /// should know about the recording. The log's own fields keep their
    /// values.
    pub fn header(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.header.push((key, value.into()));
        self
    }

    fn millis(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    fn write(&mut self, mut event: Value) {
        if self.failed {
            return;
        }
        event["t"] = json!(self.millis());
        let mut line = event.to_string();
        line.push('\n');
        let is_edit = event.get("at").is_some();
        let Some(file) = self.file.as_mut() else {
            if is_edit {
                self.open(line);
            } else {
                if self.pending.len() == MAX_PENDING {
                    self.pending.remove(0);
                }
                self.pending.push(line);
            }
            return;
        };
        if let Err(error) = file.write_all(line.as_bytes()) {
            self.fail(error);
        }
    }

    /// Creates the file with the header, the lines kept so far and `line`.
    fn open(&mut self, line: String) {
        let mut header = json!({
            "v": 1,
            "started_unix_ms": self.started_unix_ms,
            "mode": (self.mode)(),
            "text": self.text.to_string(),
            "caret": self.caret,
        });
        if let Value::Object(fields) = &mut header {
            for (key, value) in &self.header {
                fields.entry(*key).or_insert_with(|| json!(value));
            }
        }
        let mut lines = header.to_string();
        lines.push('\n');
        lines.extend(self.pending.drain(..));
        lines.push_str(&line);
        self.text = Rope::new();
        let opened = File::options()
            .write(true)
            .create_new(true)
            .open(&self.path)
            .and_then(|mut file| file.write_all(lines.as_bytes()).map(|()| file));
        match opened {
            Ok(file) => self.file = Some(file),
            Err(error) => self.fail(error),
        }
    }

    fn fail(&mut self, error: std::io::Error) {
        tracing::warn!("stopped recording keys to {}: {error}", self.path.display());
        self.failed = true;
        self.file = None;
        self.pending.clear();
    }

    /// Ends a suggestion that closed as a key was typed but that no typed
    /// edit settled.
    fn settle_typed_over(&mut self) {
        if let Some(ghost) = self.typed_over.take() {
            self.write(json!({"k": "hidden", "s": ghost}));
        }
    }

    fn event(&mut self, kind: &str) {
        self.settle_typed_over();
        self.write(json!({"k": kind}));
    }

    fn edit(&mut self, kind: &str, at: usize, removed: usize, inserted: &str, len: usize) {
        if let Some(ghost) = self.typed_over.take() {
            let along = kind == "type" && !inserted.is_empty() && ghost.starts_with(inserted);
            let ending = match (kind, along) {
                (_, true) => "typed-along",
                ("type" | "compose" | "ime", false) => "typed-past",
                _ => "hidden",
            };
            self.write(json!({"k": ending, "s": ghost}));
        }
        self.write(json!({"k": kind, "at": at, "n": removed, "s": inserted, "len": len}));
        let caret = at + inserted.len();
        self.selection = (caret, caret, caret);
    }

    /// Notes the selection, recording a move when it changed other than by
    /// an edit.
    fn select(&mut self, start: usize, end: usize, head: usize) {
        if self.selection == (start, end, head) {
            return;
        }
        self.selection = (start, end, head);
        self.settle_typed_over();
        let mut event = json!({"k": "move", "c": head});
        if start != end {
            event["sel"] = json!([start, end]);
        }
        self.write(event);
    }

    /// Notes the suggestion shown after the caret. `typing` says a typed key
    /// is closing it, which the key's edit then settles.
    fn ghost(&mut self, ghost: Option<&str>, typing: bool) {
        if ghost == self.ghost.as_deref() {
            return;
        }
        match ghost {
            Some(ghost) => {
                self.settle_typed_over();
                self.ghost = Some(ghost.to_string());
                let mode = (self.mode)();
                let pause_ms = self.pause_ms;
                self.write(json!({"k": "shown", "s": ghost, "mode": mode, "pause_ms": pause_ms}));
            }
            None => {
                let ended = self.ghost.take();
                if typing {
                    self.typed_over = ended;
                } else if let Some(ended) = ended {
                    self.write(json!({"k": "hidden", "s": ended}));
                }
            }
        }
    }

    /// Ends the shown suggestion as the user's choice: `event` is
    /// `accepted` or `dismissed`.
    fn chose(&mut self, event: Value) {
        self.settle_typed_over();
        self.ghost = None;
        self.write(event);
    }
}

impl Drop for KeyLog {
    fn drop(&mut self) {
        if self.file.is_some() {
            self.event("end");
        }
    }
}

/// Wraps an action's handler so the key log records its edits as `kind`.
pub(super) fn logged<M: InputModeKind, A: 'static>(
    kind: &'static str,
    handler: fn(&mut InputBaseState<M>, &A, &mut Window, &mut Context<InputBaseState<M>>),
) -> impl Fn(&mut InputBaseState<M>, &A, &mut Window, &mut Context<InputBaseState<M>>) + 'static {
    move |state, action, window, cx| {
        state.with_key_log_kind(kind, |state| handler(state, action, window, cx))
    }
}

impl<M: InputModeKind> InputBaseState<M> {
    /// Runs `f`, whose edits the key log records as `kind`.
    pub(crate) fn with_key_log_kind<R>(
        &mut self,
        kind: &'static str,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let outer = self.key_log.as_mut().map(|log| log.kind.replace(kind));
        let result = f(self);
        if let (Some(log), Some(outer)) = (self.key_log.as_mut(), outer) {
            log.kind = outer;
        }
        result
    }

    /// Records an edit. See [`super::InputModeKind::did_edit`].
    pub(crate) fn log_edit(&mut self, range: &std::ops::Range<usize>, new_len: usize) {
        let Some(log) = self.key_log.as_mut() else {
            return;
        };
        let kind = log.kind.unwrap_or(match self.edit_origin {
            EditOrigin::Typed => "type",
            EditOrigin::Suggestion => "suggestion",
            EditOrigin::Correction => "correction",
            EditOrigin::Pasted => "paste",
            EditOrigin::History => "undo",
            _ => "edit",
        });
        let end = range.start + new_len;
        let inserted = self.text.slice(range.start..end).to_string();
        log.edit(kind, range.start, range.len(), &inserted, self.text.len());
    }

    /// Records a caret move, when the selection changed other than by an
    /// edit.
    pub(crate) fn log_selection(&mut self) {
        let range = self.selected_range();
        let head = self.cursor();
        if let Some(log) = self.key_log.as_mut() {
            log.select(range.start, range.end, head);
        }
    }

    /// Records the textarea or its window gaining or losing focus.
    pub(crate) fn log_focus(&mut self, focused: bool) {
        if let Some(log) = self.key_log.as_mut() {
            log.event(if focused { "focus" } else { "blur" });
        }
    }
}

impl InputBaseState<TextareaMode> {
    /// Records this textarea's editing in `log`, from its text now. See
    /// [`KeyLog`].
    pub fn key_log(mut self, log: KeyLog) -> Self {
        self.attach_key_log(log);
        self
    }

    pub(crate) fn attach_key_log(&mut self, mut log: KeyLog) {
        log.text = self.text.clone();
        let range = self.selected_range();
        log.caret = self.cursor();
        log.selection = (range.start, range.end, log.caret);
        self.key_log = Some(Box::new(log));
    }

    /// Notes the wait before a request for suggestions.
    pub(crate) fn log_request(&mut self, pause: std::time::Duration) {
        if let Some(log) = self.key_log.as_mut() {
            log.pause_ms = pause.as_millis();
        }
    }

    /// Records the suggestion shown after the caret, if it changed.
    pub(crate) fn log_ghost(&mut self) {
        let typing = self.edit_origin == EditOrigin::Typed;
        let ghost = self.extras.suggestions.ghost_text().map(str::to_string);
        if let Some(log) = self.key_log.as_mut() {
            log.ghost(ghost.as_deref(), typing);
        }
    }

    /// Records the shown suggestion being taken with Tab, or a word of it
    /// with the next-word key, inserting `text`.
    pub(crate) fn log_accepted(&mut self, partial: bool, text: &str) {
        if let Some(log) = self.key_log.as_mut() {
            let key = if partial { "word" } else { "tab" };
            log.chose(json!({"k": "accepted", "key": key, "s": text}));
        }
    }

    /// Records the shown suggestion being turned down with Escape.
    pub(crate) fn log_dismissed(&mut self) {
        if let Some(log) = self.key_log.as_mut() {
            log.chose(json!({"k": "dismissed"}));
        }
    }
}
