# Keybinding schemes

A textarea follows one keybinding scheme at a time: CUA, the platform's usual shortcuts and the
default, Emacs or Vim. An application picks one with `TextareaState::keymap(Keymap::Vim)` and
switches with `set_keymap`. This page is for working on the schemes themselves.

## Where things go

```
crates/base/src/input/textarea/keymap/
  mod.rs        Keymap, KeymapPlatform, CursorShape, KeymapState, the textarea's API, dispatch
  common.rs     keys every scheme shares while editing: arrows, Backspace, Enter, Tab, Escape…
  commands.rs   editing commands any scheme can bind (paragraphs, lines, case, transpose)
  cua/          CUA: its tables, and the commands it binds beyond the engine's
  emacs.rs      Emacs: its table, its state and its own actions
  vim/mod.rs    Vim: its table, VimState (the mode) and its own actions, one module per area
  test.rs       KeymapTest, for driving a textarea with keystrokes in tests
```

Each scheme module provides the same four functions, which `mod.rs` calls for the active scheme:

| Function | What it does |
|---|---|
| `bindings(platform)` | The scheme's binding table on one platform |
| `new_state()` | A fresh `KeymapState` for one textarea, or `None` |
| `register_actions(element, entity, window)` | Registers the actions only this scheme has |
| `typed_text(state, text, window, cx)` | Sees typed text before it is inserted, and returns `true` to keep it out |

Work on one scheme stays in its own module. A command two schemes need goes in `commands.rs`,
added at the end of the file and listed in the module docs of `mod.rs`.

## Key contexts

Every input's element has the key context `Input` and a `keymap` entry: `cua` for single-line
inputs and editors, and the textarea's scheme for a textarea. A scheme binds in
`Keymap::context()`, such as `Input && keymap == emacs`, so only the active scheme's keys apply.
A scheme's state adds its own entries through `KeymapState::key_context`, and bindings that
depend on them extend the scheme's context:

```rust
let normal = format!("{} && vim_mode == normal", Keymap::Vim.context());
bind("d d", DeleteLine, &normal)
```

Bindings at the same depth resolve by order, the later one winning. Each table is added in one
go, CUA's first, then Emacs's and Vim's, so a scheme overrides a key from `common::bindings` by
binding it again after it in its own table. Bindings an application adds in plain `Input` apply
in every scheme.

## Binding per platform

A table is built by a function that takes the platform as a value, never with `#[cfg]`, so tests
on Linux check the macOS and Windows tables too:

```rust
pub(super) fn bindings(platform: KeymapPlatform) -> Vec<KeyBinding> {
    let cx = Keymap::Emacs.context();
    let mut bindings = common::bindings(cx);
    bindings.push(bind("alt-f", MoveToNextWord, cx));
    if platform.is_macos() {
        bindings.push(bind("cmd-c", Copy, cx));
    }
    bindings
}
```

The conventions:

- CUA uses Cmd on macOS and Ctrl elsewhere for the system's shortcuts. `platform.primary()` gives
  the modifier.
- Emacs's Meta is Alt on Windows and Linux and Option on macOS, written `alt-` everywhere. On
  macOS, Cmd keeps the clipboard, history and Select All.
- Vim binds the same keys on every platform.

## CUA

Single-line inputs and editors follow CUA whatever a textarea does, so its commands
(`cua/commands.rs`, the `cua` actions) work in every input: the engine registers them on each
one. A command from `commands.rs` registers on textareas only, so Ctrl-T on macOS, bound to
`TransposeCharacters`, transposes in textareas alone.

Each table follows the platform's own text views:

| | macOS (Cocoa) | Windows (Word, Notepad) | Linux (GTK) |
|---|---|---|---|
| By word | Option-arrows: left to a word's start, right to its end | Ctrl-arrows: to word starts, stopping at line starts and ends | Ctrl-arrows: as on macOS |
| By paragraph (a line) | Option-Up and Down: to the line's start, or end, then the next line's | Ctrl-Up as on macOS, Ctrl-Down to the next line's start | Ctrl-Up and Down as on macOS |
| Home, End, Page Up, Page Down | Scroll and leave the caret. Shift-Home and Shift-End select to the ends of the text | Move the caret along the line or a page | As on Windows |
| Redo | Cmd-Shift-Z | Ctrl-Y, Ctrl-Shift-Z | Ctrl-Shift-Z, Ctrl-Y |
| Clipboard | Cmd-X, C, V | Ctrl-X, C, V and Shift-Delete, Ctrl-Insert, Shift-Insert | As on Windows |

The less obvious choices:

- Words split as the engine's other word commands split them, with Unicode's word boundaries:
  `snake_case` and `café` are one word each, and a run of punctuation is a word of its own.
  Windows stops at the start of every word, punctuation included, and at line starts and ends,
  so Ctrl-Backspace at a line's start joins it to the line above.
- Linux redoes with Ctrl-Shift-Z, the GNOME and KDE shortcut, and with Ctrl-Y, which GTK's text
  widgets and many Linux applications also take. Windows takes both too.
- Alt-Backspace undoes on Windows, the original CUA key that Windows edit controls and Office
  still accept. Linux leaves it free.
- Insert stays free: the editor has no overwrite mode.
- macOS's Fn keys scroll as NSTextView does. Option-Page Up and Down and Ctrl-V move the caret a
  page.
- Ctrl-K on macOS cuts to the end of the line into a kill buffer the app shares, apart from the
  clipboard. Kills in a row add to it, and Ctrl-Y puts it back, as in Cocoa.
- Find and replace keep their shortcuts: Cmd-F and Ctrl-F search, Cmd-Option-F (and Cmd-Shift-F)
  and Ctrl-H replace. In an input that isn't searchable the key passes on to the application, so
  an application's own search still gets it. Shift-F10 and the Menu key open the context menu.

`cua/tests.rs` presses every key of each platform's table in a textarea and checks the text,
selection and caret, and its table of differences lists what each platform binds to the keys
that differ. A new binding needs a case there, or the test that checks every binding is tested
fails.

## State for modal schemes

A scheme that keeps state per textarea implements `KeymapState` and returns it from `new_state`.
The textarea creates it whenever it switches to the scheme. The state answers four questions:

- `key_context`: entries for the key context, such as `vim_mode = normal`.
- `cursor_shape`: `Bar`, `Block` or `Underline`.
- `mode_label`: a short name an application can show, such as `NORMAL`.
- `accepts_text_input`: whether typed text goes into the text. The platform reads it to route
  printable keys to bindings first.

The scheme's actions reach the state with `state.keymap_state_mut::<VimState>()` and call
`cx.notify()` after changing it, so the key context, the caret and the label follow. Typed text
arrives in `typed_text` first, which is where a command that takes the next character, such as
Vim's `f{char}` or `r{char}`, reads it.

## Testing

`KeymapTest` opens a focused textarea with one scheme and one platform's tables, and checks the
text, selection and caret written with markers: `ˇ` is the caret and `«` `»` enclose a
selection, whose caret is at `»`, or at `«` when written `«ˇ`.

```rust
#[gpui::test]
fn ctrl_k_kills_to_the_end_of_the_line(cx: &mut TestAppContext) {
    let mut test = KeymapTest::new(cx, Keymap::Emacs, KeymapPlatform::Linux, "heˇllo\nworld");
    test.keys("ctrl-k");
    test.assert("heˇ\nworld");
    test.keys("ctrl-y");
    test.assert("helloˇ\nworld");
}
```

`keys` presses space-separated keystrokes, `type_text` types as an input method would,
`dispatch` sends an action, and `mode_label` and `cursor_shape` read the scheme's state. Run a
scheme's tests with:

```sh
cargo test -p gpui-base --lib --locked -- input::keymap::emacs
```
