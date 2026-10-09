---
title: Textarea
description: Multi-line text input with fixed rows, soft wrapping, and auto-grow.
---

# Textarea

`Textarea` is the styled control for ordinary multi-line text. Use
[`Input`](./input.md) for a single line and [`Editor`](./editor.md) for source
code.

## Import

```rust
use gpui_kit::component::input::{Textarea, TextareaState};
```

## Basic usage

```rust
let notes = cx.new(|cx| {
    TextareaState::new(window, cx)
        .rows(5)
        .placeholder("Notes")
});

Textarea::new(&notes)
```

## Auto-grow

```rust
let message = cx.new(|cx| {
    TextareaState::new(window, cx)
        .auto_grow(2, 8)
        .placeholder("Write a message")
});

Textarea::new(&message)
```

The control grows until `max_rows`; overflowing content then scrolls.

## Value and events

```rust
let value = notes.read(cx).value();

notes.update(cx, |state, cx| {
    state.set_value("Updated notes", window, cx);
});

cx.subscribe(&notes, |this, state, event: &InputEvent, cx| {
    if matches!(event, InputEvent::Change) {
        this.notes = state.read(cx).value();
        cx.notify();
    }
});
```

`insert`, `replace`, `cursor_position`, `soft_wrap`, `searchable`, and
`submit_on_enter` are available on `TextareaState`.

## Sizes

```rust
Textarea::new(&notes).large()
Textarea::new(&notes) // medium (default)
Textarea::new(&notes).small()
```

The size changes the text size and the padding around the text together. It does
not set the height: use `h` for a fixed height, and let `rows` or `auto_grow`
decide the height of a growing textarea.

## Appearance

```rust
Textarea::new(&notes)
    .h(px(160.))
    .bordered(true)
    .disabled(false)
    .readonly(false)
    .aria_label("Notes")
```

Unlike `disabled`, a read-only textarea keeps the normal appearance and still
can be focused, selected and copied, it only rejects the changes made by the
user.

`Textarea` deliberately does not expose Input-only adornments such as `prefix`,
`suffix`, mask toggle, or the clear button. Compose related actions beside the
textarea.

## Atomic inline tokens

Use tokens to include mentions, file references or commands in a multi-line
message. Insert a token with `TextareaState::replace_with_token`, or restore a
saved draft with `set_value`. The default label is ready to use; add a renderer
when you want an icon or other custom content:

```rust
Textarea::new(&state)
    .token(|token, _, _| InputToken::new(token).icon(IconName::File))
```

Import `InputToken` from `gpui_kit::component::input` and `IconName` from
`gpui_kit::component`. A token wraps onto the next line as a whole, and auto-grow adjusts the textarea
height to fit. Put line breaks in the text between tokens. Use `on_token_hover`
to show a tooltip or preview without selecting or editing; disabled tokens never
report hover entry; disabling a hovered token sends its exit. See
[Input: atomic inline tokens](./input.md#atomic-inline-tokens) for editing,
activation, draft persistence, mode restrictions and JavaScript APIs.

## Suggestions

A textarea can offer suggestions from a `SuggestionProvider` the application
owns: word completions in prose, names in a message, snippets in a note. The
provider decides what to suggest and in which order. The textarea asks it as the
user types, shows the answer under the word being completed, and handles the
keys: Up and Down choose, Enter or Tab accept, Escape dismisses.

```rust
use gpui_kit::component::input::{
    RopeExt as _, Suggestion, SuggestionProvider, SuggestionRequest, Textarea, TextareaState,
};

struct Words(Vec<&'static str>);

impl SuggestionProvider for Words {
    fn suggestions(
        &self,
        request: &SuggestionRequest,
        _: &mut Window,
        _: &mut App,
    ) -> Task<anyhow::Result<Vec<Suggestion>>> {
        // Read the caret's line, not the whole text.
        let (text, offset) = (request.text(), request.offset());
        let line_start = text.line_start_offset(text.offset_to_point(offset).row);
        let before = text.slice(line_start..offset).to_string();
        let typed = before.rsplit(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
        let start = offset - typed.len();
        let items = self
            .0
            .iter()
            .filter(|word| !typed.is_empty() && word.starts_with(typed))
            .map(|word| Suggestion::new(*word).with_range(start..offset))
            .collect();
        Task::ready(Ok(items))
    }
}

let notes = cx.new(|cx| {
    TextareaState::new(window, cx)
        .rows(5)
        .suggestion_provider(Rc::new(Words(vec!["hello", "help"])))
});

Textarea::new(&notes)
```

A suggestion replaces its range, usually what was typed of the word, with its
text. A task that is ready when returned shows in the same frame. A slow one can
compute in the background: its answer is dropped if the text or the caret
changed meanwhile.

`SuggestionOptions` chooses the presentation and the keys. With `inline(true)`
the selected suggestion is previewed as ghost text after the caret, and the rest
of the line moves to make room for it. With `menu(false)` as well, there is no
menu, and Tab accepts the preview.

```rust
TextareaState::new(window, cx)
    .suggestion_provider(provider)
    .suggestion_options(SuggestionOptions::default().menu(false).inline(true))
```

`SuggestionEvent::Accepted` and `SuggestionEvent::Dismissed` report what the user
did with a suggestion. The provider's `did_change` hears every edit with the
text's revision, so it can keep its own index of the text current without
copying it, and `debounce` delays requests while the user types.

`ShowSuggestions`, `AcceptSuggestion`, `AcceptSuggestionWord`,
`SelectNextSuggestion`, `SelectPreviousSuggestion` and `DismissSuggestions` have
no default keys. Bind them in the `Input` context. With nothing to act on, an
action passes the key on to its next binding, so a key given to
`AcceptSuggestionWord` keeps its usual meaning the rest of the time.

`show_suggestions`, `present_suggestions` and `hide_suggestions` open and close
suggestions from code, and `suggestion_item` renders each row:

```rust
Textarea::new(&notes).suggestion_item(|item, _, _| {
    div()
        .px_2()
        .when(item.is_selected(), |this| this.bg(gpui::blue()))
        .child(item.suggestion().label().clone())
})
```

## Text editor

`TextareaState::text_editor` sets a textarea up for writing: line numbers, the
search panel and soft wrap. `TextEditor` renders it filling its parent, with
square corners and no border.

```rust
use gpui_kit::component::input::{TextEditor, TextareaState};

let document = cx.new(|cx| {
    TextareaState::new(window, cx)
        .text_editor()
        .spell_checker(Rc::new(Dictionary::default()))
        .suggestion_provider(Rc::new(Words::default()))
        .default_value(text)
});

TextEditor::new(&document)
```

Each part also works on its own, on any textarea: `line_number(true)`,
`searchable(true)`, `spell_checker` and [suggestions](#suggestions).

### Spell checking

The application owns the dictionary through a `SpellChecker`: it finds the
misspelled words, suggests replacements and learns new words. The textarea
decides when to check, draws the underlines and offers the fixes.

```rust
use gpui_kit::component::input::{SpellCheck, SpellCheckRequest, SpellChecker};

#[derive(Default)]
struct Dictionary {
    words: RefCell<HashSet<String>>,
}

impl SpellChecker for Dictionary {
    fn check(&self, request: &SpellCheckRequest, _: &mut App) -> Task<anyhow::Result<SpellCheck>> {
        let words = self.words.borrow();
        let mut misspelled = Vec::new();
        for range in request.ranges() {
            let line = request.text().slice(range.clone()).to_string();
            let mut start = range.start;
            for word in line.split(|c: char| !c.is_alphabetic()) {
                if !word.is_empty() && !words.contains(&word.to_lowercase()) {
                    misspelled.push(start..start + word.len());
                }
                start += word.len() + 1;
            }
        }
        Task::ready(Ok(SpellCheck {
            misspelled,
            ..Default::default()
        }))
    }

    fn suggestions(&self, word: &str, _: &mut App) -> Vec<SharedString> {
        // The closest known words, best first.
        nearest(&self.words.borrow(), word)
    }

    fn add_to_dictionary(&self, word: &str, _: &mut App) {
        self.words.borrow_mut().insert(word.to_lowercase());
    }
}
```

The first render checks all of the text. After that, an edit marks its lines,
and once typing pauses for `SpellChecker::debounce` (300 ms by default) only
those lines are checked again. Between checks the underlines move with the
text. A misspelling that ends at the caret right after typing is marked once
the caret leaves the word. A slow checker can return a pending task and work in
the background, and its answer moves along with the edits made meanwhile.

`set_spell_checking(false)` turns checking off and clears the underlines,
`set_spell_checker` swaps the checker, and `refresh_spelling(cx)` has every
textarea check again after the dictionary changed elsewhere, for example in a
settings page.

### Fixing a misspelling

Right-click an underlined word, or press Shift-F10 or the Menu key with the
caret in it, and the context menu lists up to five of the checker's
suggestions, **Add to Dictionary** and **Ignore** above Cut, Copy, Paste and
Select All. Choosing a suggestion replaces the word in one undo step. Add to
Dictionary calls `SpellChecker::add_to_dictionary` and clears every underline
of the word, and Ignore stops marking it in this textarea. `SpellEvent`
reports each of these.

The items dispatch `ReplaceMisspelling`, `AddToDictionary` and
`IgnoreMisspelling`, which the textarea handles. `misspelling_at(offset)` gives
the word under a position and the same actions, for fixes offered elsewhere,
such as a toolbar.

### Your own menu items

`context_menu_at` builds the menu knowing where it opened: the offset, the word
there, a misspelling and its suggestions, and the marks. `spelling_items` and
`standard_items` add the default groups, so an application's items can go
around them:

```rust
TextEditor::new(&document).context_menu_at(|menu, target, _, _| {
    let menu = target.spelling_items(menu);
    let menu = match target.word() {
        Some(word) => menu
            .menu(format!("Define “{word}”"), Box::new(Define(word.clone())))
            .separator(),
        None => menu,
    };
    target.standard_items(menu)
})
```

### Marks

Marks underline ranges of the text and move with edits. The spelling
underlines are marks, and an application adds its own, such as grammar hints
or comments:

```rust
use gpui_kit::component::input::{Mark, MarkStyle};

let hints = document.update(cx, |document, cx| document.create_mark_collection(cx));
hints.set(
    vec![Mark::new(4..9, MarkStyle::Grammar).with_data("Passive voice")],
    cx,
);
```

`MarkStyle::Spelling` and `MarkStyle::Grammar` draw wavy underlines in the
theme's error and info colours, and `MarkStyle::Underline` a straight or wavy
one in a colour of your choice. A mark keeps its place while text is typed
around it and goes away when an edit replaces all of its text. `splice`
replaces the marks in a range, for a check that covers part of the text.
`marks_at` reads back what is under a position, and `ContextMenuTarget::marks`
what is under the menu.

### Keybindings

A textarea follows one keybinding scheme: `Keymap::Cua`, the platform's usual
shortcuts and the default, `Keymap::Emacs` or `Keymap::Vim`. Choose one when
building the state and switch at any time:

```rust
use gpui_kit::component::input::Keymap;

let document = cx.new(|cx| TextareaState::new(window, cx).text_editor().keymap(Keymap::Vim));

document.update(cx, |document, cx| document.set_keymap(Keymap::Emacs, cx));
```

Only the active scheme's keys apply. A scheme with modes reports the current
one through `keymap_mode_label`, such as `NORMAL` or `INSERT` in Vim, and draws
the caret as `cursor_shape` says: a block in Vim's normal mode. Observe the
state to show the label in a status bar. CUA binds the platform's shortcuts,
and Emacs binds its basic motions so far.

#### Vim

| | Keys |
|---|---|
| Modes | `i` `a` `I` `A` `o` `O` insert, `R` replace, `v` `V` Ctrl-V visual, Escape back to normal |
| Motions | `h` `j` `k` `l`, `w` `b` `e` `ge`, `0` `^` `$`, `gg` `G`, `f` `t` `F` `T` `;` `,`, `%`, `{` `}`, `H` `M` `L`, `/` `?` `n` `N` `*` `#` |
| Operators | `d` `c` `y` `>` `<` `g~` `gu` `gU`, doubled for lines, with counts such as `2d3w` |
| Text objects | `iw` `aw`, quotes, brackets, `it` `at` for tags, `ip` `ap` |
| Editing | `x` `X` `D` `C` `Y` `s` `S` `r` `~` `J` `p` `P`, `u` and Ctrl-R, `.`, Ctrl-A and Ctrl-X |
| Registers | `"a` to `"z`, `"A` to append, `"0` to `"9`, `"_`, `"+` and `"*` for the clipboard |
| Command line | `:{n}`, `:s/pattern/replacement/g` on a line, a range or the selection, `:w`, `:q`, `:wq`, `:x` |

Vim's keys are the same on every platform. Ctrl-V starts visual block mode, as
in Vim on Unix. The clipboard and undo shortcuts stay: Cmd-C, Cmd-X, Cmd-V,
Cmd-A and Cmd-Z on macOS, Ctrl-Shift-C and Ctrl-Shift-V on Windows and Linux.
Other Ctrl and Cmd shortcuts reach the application, except Vim's own Ctrl keys
such as Ctrl-R and Ctrl-W.

While a command line is typed, the mode label shows it, such as `:s/a/b/`.
`:w` dispatches `VimWrite`, `:q` dispatches `VimQuit`, and `:wq` and `:x` both.
Any other command arrives as `VimCommand` with its text. Handle them around the
editor:

```rust
use gpui_kit::component::input::{VimCommand, VimQuit, VimWrite};

div()
    .on_action(cx.listener(|this, _: &VimWrite, window, cx| this.save(window, cx)))
    .on_action(cx.listener(|this, _: &VimQuit, window, cx| this.close(window, cx)))
    .on_action(cx.listener(|this, action: &VimCommand, window, cx| {
        this.run(&action.command, window, cx)
    }))
    .child(TextEditor::new(&document))
```

