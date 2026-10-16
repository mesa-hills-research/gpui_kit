//! Spell checking and word suggestions for GPUI Kit's textareas.
//!
//! A `TextareaState` from GPUI Kit asks a `SpellChecker` which words are
//! misspelled and a `SuggestionProvider` what could complete the text at the
//! caret, and draws the underlines, the fixes and the ghost text itself. This
//! crate supplies both:
//!
//! - [`DictionaryChecker`] checks against a [`Dictionary`]: a bundled
//!   English word list (feature `english`, on by default), any other
//!   [`WordList`], and the words the user added. It skips code, identifiers,
//!   paths and other text that isn't prose, and ranks corrections for typing
//!   mistakes.
//! - [`WordSuggestions`] completes the word being typed, and predicts the
//!   next one after a space, from a vocabulary learned on the user's
//!   computer from what they type, never from pasted or program text. It
//!   offers a suggestion only when it is more likely right than wrong, backs
//!   off from words the user types past, and paces itself by
//!   [`SuggestionMode`].
//!
//! [`Writing`] holds what every textarea shares: the dictionary, the
//! vocabulary and the pacing. [`WritingOptions`] says where its files live.
//!
//! The crate is English-centric: the spell checker skips words with letters
//! outside ASCII, and the code/prose classifier counts English function
//! words.

mod classify;
mod completion;
mod options;
mod spelling;
mod text;
mod writing;

pub use completion::{SuggestionMode, WordSuggestions, accept_word_bindings};
pub use options::{WordList, WritingOptions};
pub use spelling::{Dictionary, DictionaryChecker};
pub use writing::Writing;
