# mhr-gpui-kit-writing

Spell checking and learned word suggestions for GPUI Kit's `TextareaState`.

- `DictionaryChecker` is a `SpellChecker` against a `Dictionary`: the bundled
  English word list (SCOWL size 70, feature `english`, on by default), any
  other word list, and the words the user added. It skips code, identifiers,
  paths and URLs, and ranks corrections for typing mistakes.
- `WordSuggestions` is a `SuggestionProvider` that completes the word being
  typed and predicts the next one, from a vocabulary learned from what the
  user types on their computer, paced by `SuggestionMode` (Off, Quiet, Eager).
- `Writing` holds what an application's textareas share, and
  `WritingOptions` says where its files live.

Applications usually enable it through `mhr-gpui-kit`'s `writing` feature and
reach it as `gpui_kit::writing`. See
[Spell Checking and Word Suggestions](../../website/docs/writing.md).

The bundled word list comes from SCOWL by Kevin Atkinson, under its own
permissive license: see [NOTICE](NOTICE).
