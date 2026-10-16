//! Small text helpers the spell checker and the word suggestions share.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Hashes a string with the standard library's default hasher.
pub(crate) fn hash_str(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Clamps an index down to the nearest UTF-8 character boundary.
pub(crate) fn floor_char_boundary(text: &str, mut idx: usize) -> usize {
    if idx >= text.len() {
        return text.len();
    }

    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }

    idx
}

/// The start of the line holding byte `offset` of `text`.
pub(crate) fn line_start(text: &str, offset: usize) -> usize {
    text[..offset]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0)
}
