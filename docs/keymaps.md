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
  emacs/        Emacs: its table, EmacsState (the mark, the kill ring) and its own actions
  vim/          Vim: its table, VimState (the mode) and its own actions, one module per area
  switching.rs  tests for switching schemes while one is in use
  test.rs       KeymapTest, for driving a textarea with keystrokes in tests
```

A scheme module provides these functions, which `mod.rs` calls for the active scheme:

| Function | What it does |
|---|---|
| `bindings(platform)` | The scheme's binding table on one platform |
| `new_state()` | A fresh `KeymapState` for one textarea, or `None` |
| `register_actions(element, entity, window)` | Registers the actions only this scheme has |
| `typed_text(state, text, window, cx)` | Sees typed text before it is inserted, and returns `true` to keep it out |
| `on_render(state, cx)` | Catches up with changes made around the scheme, such as a selection made with the mouse, before the textarea renders |

Emacs and Vim provide the first four, and Vim `on_render` too. CUA keeps no state and takes no
typed text, so it has `bindings` and `register_actions`, and the engine registers its actions on
every input (see [CUA](#cua)).

Work on one scheme stays in its own module. A command two schemes need goes in `commands.rs`,
added at the end of the file and listed in the module docs of `mod.rs`.

## Key contexts

Every input's element has the key context `Input` and a `keymap` entry: `cua` for single-line
inputs and editors, and the textarea's scheme for a textarea. A scheme binds in
`Keymap::context()`, such as `Input && keymap == emacs`, so only the active scheme's keys apply.
Every scheme's actions are registered on a textarea, and CUA's on every input, so the context
alone decides which scheme a key reaches. A scheme's state adds its own entries through `KeymapState::key_context`, and bindings that
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
    let mut bindings = common::bindings(platform, cx);
    bindings.push(bind("alt-f", MoveToNextWord, cx));
    if platform.is_macos() {
        bindings.push(bind("cmd-c", Copy, cx));
    }
    bindings
}
```

The conventions:

- CUA uses Cmd on macOS and Ctrl elsewhere for the system's shortcuts. `platform.primary()` gives
  the modifier. GPUI's `secondary-` would give the modifier of the platform the code runs on, so
  the tables never use it.
- Emacs's Meta is Alt on Windows and Linux and Option on macOS, written `alt-` everywhere. On
  macOS, Cmd keeps the clipboard, history and Select All.
- Vim binds the same keys on every platform.

## State for modal schemes

A scheme that keeps state per textarea implements `KeymapState` and returns it from `new_state`.
The textarea creates it whenever it switches to the scheme. The state answers these questions:

- `key_context`: entries for the key context, such as `vim_mode = normal`.
- `cursor_shape`: `Bar`, `Block` or `Underline`.
- `mode_label`: a short name an application can show, such as `NORMAL`.
- `accepts_text_input`: whether typed text goes into the text. The platform reads it to route
  printable keys to bindings first.
- `caret_offset`: where to draw the caret when the scheme draws one of its own, as Vim's visual
  modes do on the last selected character. The other selections then draw none.

It also hears every edit through `adjust_for_edit`, to keep offsets it holds, such as Emacs's
mark, on the same text.

`set_keymap` drops the old scheme's state, so the next scheme starts afresh: Vim in normal mode,
Emacs with no mark or prefix argument, and the caret back to a bar. The selection and the undo
history belong to the text and stay. Whatever a scheme should forget at a switch belongs in its
state.

The scheme's actions reach the state with `state.keymap_state_mut::<VimState>()` and call
`cx.notify()` after changing it, so the key context, the caret and the label follow. Typed text
arrives in `typed_text` first, which is where a command that takes the next character, such as
Vim's `f{char}` or `r{char}`, reads it.

## The smooth caret

The smooth caret glides only for the movements a handler records with `caret_motion`: typing,
Backspace, and moves by a character or a word. A scheme's own motion of that kind runs its move
inside it, as Vim's `h`, `l`, `w`, `b` and `e` do:

```rust
state.caret_motion(CaretMotion::Word, |state| state.move_to(offset, None, cx))
```

Every other movement puts the caret in place at once.

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
- A menu shows an action's latest binding, so where two keys do the same, the platform's usual
  one comes later in the table: Ctrl-X, C and V after Shift-Delete, Ctrl-Insert and
  Shift-Insert, Ctrl-Z after Alt-Backspace, and for Redo Ctrl-Y on Windows and Ctrl-Shift-Z on
  Linux.
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

## Emacs

Emacs keeps an `EmacsState` per textarea: the mark and a mark ring of 16, a kill ring of 120
entries and the prefix argument being typed. Every key runs one of the scheme's own actions, such
as `emacs::KillLine`, and each action takes the prefix argument and what the command before it
left as it starts. The module has one file per area: `mod.rs` for the table, the state, the mark
and the prefix argument, `motion.rs`, `kill.rs` and `edit.rs`.

**The region is the selection.** `C-Space` sets the mark at the caret and activates it, and the
motions then extend the selection from the mark. The mark stays active while the selection's
anchor is the mark, so a click, a Shift selection or an edit ends it. A Shift selection is a
region too: `C-w` kills it, and a motion collapses it and moves on from the caret. Typing or
yanking replaces the selection, as with Emacs's `delete-selection-mode`. `C-w` and `M-w` without a
selection take the text between the caret and the mark, active or not.

**Some commands depend on the one before.** Kills in a row join into one entry of the kill ring,
and `M-y` only follows `C-y`. A command records what it did with a fingerprint of the text's
revision and the selections, and the next one compares it with the current fingerprint, so an edit
or a caret move in between, from an engine action or a click, breaks the chain. A prefix argument
reaches the next command the same way. It repeats motions, kills, deletions, transposing, case and
typed text, and shows as the mode label, such as `C-u 4`, while it is typed.

**Every kill also goes to the clipboard.** `C-y` first takes the clipboard into the kill ring when
it holds text the ring has not seen, such as text copied in another application or with Cmd-C on
macOS. Each textarea has its own kill ring, and the clipboard carries the latest kill between
them.

**Platforms.** The tables differ only in macOS's Cmd keys.

- Meta is Alt on Windows and Linux and Option on macOS, where Cmd-A, C, X, V, Z and Shift-Z keep
  their system meaning.
- Escape followed by a key is Meta with that key. GPUI waits up to a second for the second key, so
  a lone Escape reaches the textarea and the application after that second.
- On macOS, Ctrl-Space switches input sources by default, and some Linux input methods take it as
  well. `C-@` sets the mark too.
- On Windows, GPUI handles Alt keys itself and the kit's `AppMenuBar` opens its menus with the
  pointer, so Meta keys never open a menu. An application's own Alt binding, such as Alt-F for a
  File menu, is outside the textarea's context and loses to Emacs's while the textarea has focus.

**Hooks for the application.** `C-x C-s` and `C-x C-w` dispatch `SaveBuffer` and `WriteFile`,
which the textarea leaves to the application: handle them with `on_action`, or bind the
application's own save actions to those keys in `Keymap::Emacs.context()`, which then win. `C-s`
and `C-r` open the search panel of a `searchable` textarea and move to the next or the previous
match. Without the panel they dispatch `Search` for a custom search UI.

**Still to come:** `C-s` and `C-r` inside the open search panel, whose field follows CUA (Enter
and Shift-Enter move between matches), the rest of incremental search, `C-l`, `M-q`, `M-z`,
`M-SPC`, `C-x C-t`, `C-M-` keys, registers, rectangles and keyboard macros, and counts for undo
and Enter.

## Vim

Vim lives in `keymap/vim/`:

```
vim/
  mod.rs        VimMode, VimState, the actions, and running them
  bindings.rs   the binding table
  text.rs       lines, character classes, and a cursor that steps the way Vim's does
  motion.rs     motions
  object.rs     text objects
  operator.rs   operators and the regions they act on
  normal.rs     counts, normal mode's commands, undo and `.`
  insert.rs     insert and replace modes
  visual.rs     the visual modes
  register.rs   registers
  search.rs     searching, and Vim's patterns
  ex.rs         the command line and Ex commands
  tests/        tests by area
```

### How a key runs

Each key is a binding to an action in its mode's context. The state adds these entries to the
key context:

| Entry | Values |
|---|---|
| `vim_mode` | `normal`, `insert`, `replace`, `visual`, `operator`, `waiting` while a key such as `f` waits for its character, `command` while the command line is open |
| `vim_visual` | `char`, `line` or `block`, in visual mode |
| `vim_operator` | the operator waiting for its motion, such as `delete` |
| `vim_count` | present while a count is typed, so `0` continues it |

A command of several keys is one binding, such as `g g` or `i w`, or an operator that changes the
context for the keys after it: `d` waits in `vim_mode == operator`, where `d` again is the doubled
form. Neither waits on a timeout. A prefix with no binding of its own, in a mode that inserts
nothing, stays open as long as the user takes.

Every action runs through `update`, which takes the `VimState` out of the textarea so a command
can borrow both. It first catches up with changes made around Vim: a selection made with the
mouse starts visual mode, and a caret left past a line's end steps back onto its last character.

A motion gives a `Target`: where it lands, and whether an operator takes the text up to it,
through it or by whole lines. `Region::for_motion` applies Vim's rules for exclusive motions that
end at the start of a line. Operators act on a `Region` of characters, lines or a block. The word
motions follow Vim's own `fwd_word`, `bck_word`, `end_word` and `bckend_word`, so `dw` on a
line's last word keeps the line break and `cw` changes to the end of the word.

Everything that changes text is a `Command`, which `.` runs again with its count, its register
and the text typed after it. That text is read back from the undo history when the insert ends,
so whatever put it there, typing, an input method, an accepted suggestion or a closing bracket,
is repeated as it landed. The same history makes the insert one undo step:
`UndoManager::group_since` merges what was recorded since the command began.

Visual mode keeps its anchor and head in the state and selects the text between them, the
character under the caret included, and the textarea draws the caret on that character through
`caret_offset`. A block is one selection per row, so `I`, `A` and `c` type into every row with
the textarea's multiple cursors.

Registers live in an app global, so a yank in one textarea pastes in another, as Vim's registers
are shared by its buffers.

### Adding to it

- A motion: a `Motion` variant, its case in `Motion::evaluate`, and a key in the `motions` list
  in `bindings.rs`. It then works after every operator, in visual mode and with counts.
- A text object: an `Object` variant, its range in `object.rs`, and a key in the `objects` list.
- An operator: an `Operator` variant, its `vim_operator` name, its case in `apply_operator`, and
  a key in the `operators` list.
- A command that changes text: a `Command` variant and its case in `VimState::execute`, which
  makes `.` repeat it.
- An Ex command: a name in `run_ex`. A command `run_ex` doesn't know is dispatched as
  `VimCommand` for the application.

### What it covers

- Modes: normal, insert, replace (`R`), visual, visual line, visual block, and operator-pending.
  The caret is a block in normal and visual modes, a bar in insert mode, and an underline in
  replace mode and while an operator or a key such as `r` waits. The labels are `NORMAL`,
  `INSERT`, `REPLACE`, `VISUAL`, `VISUAL LINE`, `VISUAL BLOCK` and `O-PENDING`, and the command
  line while it is typed.
- Counts on motions, operators (`2d3w`), inserts (`3ix`), puts, `r`, `~`, `J`, Ctrl-A and `.`.
- Motions: `h` `j` `k` `l` and the arrows, Backspace and Space across lines, `gj` `gk`, `w` `b`
  `e` `ge` and `W` `B` `E` `gE`, `0` `^` `$` `g_` Home End, `+` `-` `_` Enter, `gg` `G`
  `{count}G` `{count}%`, `f` `F` `t` `T` `;` `,`, `%`, `{` `}`, `H` `M` `L`, Ctrl-D Ctrl-U
  Ctrl-F Ctrl-B, and `/` `?` `n` `N` `*` `#`.
- Operators: `d` `c` `y` `>` `<` `g~` `gu` `gU`, doubled for lines, and `D` `C` `Y` `x` `X` `s` `S`
  `r` `~` `J` `gJ`, Ctrl-A and Ctrl-X.
- Text objects: `iw` `aw` `iW` `aW`, `i"` `a"` `i'` `a'` `` i` `` `` a` ``, `i(` `a(` `ib` `ab`,
  `i[` `a[`, `i{` `a{` `iB` `aB`, `i<` `a<`, `it` `at` and `ip` `ap`. Brackets nest across lines,
  and in visual mode the same object again grows to the next pair out.
- Registers: the unnamed register, `a`–`z` and `A`–`Z` to append, `0` for the last yank, `1`–`9`
  for deleted lines, `-` for smaller deletes, `_` the black hole, `+` and `*` the system clipboard,
  and `.` `/` `:` to read. `p` `P` `gp` `gP` put characters, lines and blocks, with counts.
- Undo and redo with `u` and Ctrl-R, each insert one step, and `.` with the text typed.
- Visual modes: motions and text objects extend the selection, operators act on it, `o` and `O`
  swap its ends, `gv` selects the last one again, `p` replaces it, `r` fills it, `I` and `A` type
  before or after a block.
- Insert mode: `i` `a` `I` `A` `gI` `o` `O`, Escape, Ctrl-[ and Ctrl-C back to normal mode,
  Ctrl-W, Ctrl-U, Ctrl-H, Ctrl-T and Ctrl-D, Ctrl-R with a register, Ctrl-V with a key, and
  Ctrl-N and Ctrl-P in the suggestion menu. Replace mode puts back what Backspace takes.
- The command line: `:{n}` and `:$`, `:s/pattern/replacement/[gIi]` on the line, a range
  (`%`, `n,m`, `.`, `$`, `'<,'>`, with `+n` and `-n`) or the visual selection, `:d` and `:y` with
  a register, `:noh`, `:u` and `:red`. `:w`, `:q`, `:wq`, `:x`, `ZZ` and `ZQ` dispatch `VimWrite`
  and `VimQuit`, and any other command `VimCommand`, for the application to handle.
- Patterns: Vim's magic syntax, `\v` for very magic, `\c` and `\C` for case, and `\<` `\>`
  for word edges. Plain patterns are highlighted with the textarea's search.

### Around Vim

- Escape in insert mode closes an open suggestion first, then leaves insert mode.
- Shift-F10 and the Menu key open the context menu in normal and visual mode, as in the others.
- Vim binds the same keys on every platform, with Vim's Unix behaviour: Ctrl-V starts visual
  block mode, and in insert mode inserts the next key as it is. The platform's clipboard and
  history shortcuts stay where they don't clash: Cmd-C, Cmd-X, Cmd-V, Cmd-A, Cmd-Z, Cmd-Shift-Z
  and Cmd-F on macOS, Ctrl-Shift-C and Ctrl-Shift-V on Windows and Linux.
- Vim's own Ctrl keys win over the application's: Ctrl-A, Ctrl-B, Ctrl-C, Ctrl-D, Ctrl-F,
  Ctrl-H, Ctrl-R, Ctrl-T, Ctrl-U, Ctrl-V, Ctrl-W, Ctrl-X and Ctrl-[, in the modes that use them.
  Ctrl-N and Ctrl-P are Vim's while the suggestion menu is open. Every other Ctrl and Cmd
  shortcut, such as Ctrl-S, Ctrl-Z or Cmd-S, reaches the application.

### Not yet

Macros (`q`, `@`), marks (`m`, `'`, `` ` ``) and the jump list, Ctrl-O in insert mode, scrolling
with Ctrl-E, Ctrl-Y, `zz`, `zt` and `zb`, sentence objects (`is`, `as`), `gq` and `=`, `:g`,
`:normal` and `:set`, `ignorecase` and `smartcase`, case changes (`\u`, `\U`) in `:s`
replacements, patterns across lines in `:s`, `.` after a block insert, command-line history and
`U`.

Run Vim's tests with:

```sh
cargo test -p gpui-base --lib --locked -- input::keymap::vim
```
