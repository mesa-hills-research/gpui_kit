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
