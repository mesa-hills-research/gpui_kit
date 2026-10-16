---
title: Spell Checking and Word Suggestions
description: gpui-kit-writing gives a Textarea a real dictionary and word suggestions learned from what the user types.
order: -9.04
---

# Spell Checking and Word Suggestions

A [Textarea](../component/textarea) underlines misspellings it gets from a
`SpellChecker` and shows the completions a `SuggestionProvider` offers. The
textarea owns when to ask, the underlines, the fixes and the ghost text; the
application owns the dictionary and what to suggest. `gpui-kit-writing`
supplies both, so a writing surface such as a chat composer or a notes editor
gets them without writing its own:

- **Spell checking** against a dictionary: a bundled English word list
  (SCOWL size 70, about 170,000 words), any other word list, and the words the
  user added. Code, identifiers, paths, URLs, ALL-CAPS and camelCase words are
  left alone, and right-click corrections are ranked for typing mistakes, so
  "mispeled" offers "misspelled" first.
- **Word suggestions** that finish the word being typed, and predict the next
  word after a space, from a vocabulary learned on the user's computer from
  what they write: typing, suggestions taken with Tab and spelling
  corrections chosen from the menu. Pasted text, undo and redo and text the
  program inserts teach nothing, however short. A suggestion shows only when
  it is more likely right than wrong, and words the user keeps typing past
  are offered less. Punctuation
  between words, such as a comma, a colon or a slash, ends what a prediction
  follows: after "UI/UX, " nothing learned after "UI/UX" is offered.

Enable the `writing` feature of `gpui-kit`:

```toml
gpui-kit = { version = "...", features = ["writing"] }
```

## Setting it up

`Writing` holds what every textarea shares: the dictionary, the vocabulary and
how eagerly suggestions show. Load it once when the application starts. Each
textarea gets its own checker and its own suggestions:

```rust
use gpui_kit::component::input::{SuggestionOptions, TextEditor, TextareaState};
use gpui_kit::writing::{Writing, WritingOptions, accept_word_bindings};

// At startup, after gpui_kit::init.
let writing = Writing::load(
    WritingOptions::new().with_data_dir(config_dir.join("my-app")),
    cx,
);
cx.bind_keys(accept_word_bindings());

// For each textarea.
let suggestions = writing.word_suggestions();
let document = cx.new(|cx| {
    TextareaState::new(window, cx)
        .spell_checker(writing.spell_checker())
        .suggestion_provider(suggestions.clone())
        .suggestion_options(SuggestionOptions::default().menu(false).inline(true))
});
// Keep the subscription: Tab and Escape pace what comes next.
let pacing = suggestions.follow(&document, cx);
```

`Writing::load` returns at once and reads the word lists and the vocabulary
on a background thread. Until they arrive, which takes a fraction of a second
(longer for a large vocabulary), nothing is underlined and suggestions know
only what the user types; then every textarea's spelling is checked again.
`is_loaded` says whether they have arrived. `Writing::load_blocking` reads
them on the calling thread, for tests and tools.

Tab takes a whole suggestion. `accept_word_bindings` binds Ctrl+Right, or
Cmd+Right on macOS, to take its next word; with nothing offered the key moves
the caret as usual.

A textarea opened with text the user didn't type, such as a document read
from disk, tells its suggestions with `suggestions.opened_with(&text)`, so
that text is never learned.

A sentence is learned once the user types on after its `.`, `!`, `?` or line
break, so the last sentence of a chat message sent with Enter would never be.
A composer calls `suggestions.learn_rest(state.text())` as it sends, before it
clears the text: the last sentence is learned under the same rules, once.

## Files

`WritingOptions` says where the files live. Without a data folder nothing is
read or saved, and what the suggestions learn lasts the session. The files
hold what the user wrote, so on Unix they are created readable by the user
alone.

| Option | Default | Holds |
| --- | --- | --- |
| `with_data_dir(path)` | none | the folder of the two files below |
| `with_vocabulary_file(name)` | `vocabulary.json` | what the suggestions learned, with `vocabulary.lock` beside it |
| `with_user_dictionary_file(name)` | `user-dictionary.txt` | the words **Add to Dictionary** added, one per line |
| `with_word_lists(lists)` | `[WordList::English]` | the word lists the dictionary accepts |
| `with_vocabulary_cap(bytes)` | 16 MiB | the most bytes `vocabulary.json` may take |

A `WordList` is `English` (the `english` feature, on by default), a
`File(path)` read at loading, or `Text`. A list is one word per line, `#`
starts a comment, and Hunspell affix flags (`word/MS`) are stripped, though
not expanded. With fewer than 10,000 words in all the dictionary accepts
every word rather than mark everything.

The vocabulary saves when the application says so:

```rust
// When a document is saved, a window closes, or on a timer.
writing.save_in_background(cx).detach();

// To quit: close the windows, then quit once the save is done.
let saved = writing.save_in_background(cx);
cx.spawn(async move |cx| {
    saved.await;
    cx.update(|cx| cx.quit());
})
.detach();

// In a quit handler, where nothing can wait.
writing.save();
```

A background save never holds up the UI. `save` runs on the calling thread:
saving a vocabulary near the default cap takes a few hundred milliseconds, and
it waits at most a second for a background save under way and half a second
for another process's save, after which this session's latest learning stays
unsaved. **Add to Dictionary** appends its word to the user's word list on a
background thread too, and `save` appends any it hasn't yet.

Several processes can share one vocabulary file. Each save adds what this
process learned since its last save to what the file holds now, under a lock,
and writes the file whole, so a crash leaves the old file or the new one. A
file that doesn't parse is moved aside rather than written over.

A save that would take the file past its cap drops the rarest entries, those
seen once first, until it takes seven eighths of the cap. The vocabulary in
memory takes about six and a half times its file: about 100 MB at the default
16 MiB. A background save briefly needs about that much again, to read what
other processes added to the file; `save` skips the reading when the file is
as this process last left it. A smaller cap keeps memory down and remembers
less of what the user wrote.

## How eagerly suggestions show

`writing.set_suggestion_mode(mode)` switches every textarea from its next
keystroke:

| `SuggestionMode` | Behavior |
| --- | --- |
| `Off` | No suggestions. Nothing typed while Off is learned, then or later. |
| `Quiet` (default) | After typing pauses for 300 ms. Each suggestion typed past adds 150 ms, up to a second, and a word typed past is not offered again after the same start for ten minutes. A suggestion stays while the user types along it. |
| `Eager` | After every keystroke. |

`SuggestionMode` serializes as `"off"`, `"quiet"` or `"eager"`, and reads
`true` and `false` as Quiet and Off.

## Limits

- English only: the checker skips words with letters outside ASCII, and the
  code/prose classifier counts English function words.
- Spell checking and suggestions are a `TextareaState` feature. A single-line
  `Input` and the code `Editor` have neither.
