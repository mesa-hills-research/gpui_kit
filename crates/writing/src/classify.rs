//! Whether text is code or prose.
//!
//! A language-agnostic classifier scores lines as code or prose from their
//! structure (symbols, identifier-shaped tokens, a line ending in `;` or a
//! brace) against the strongest prose signal, the share of English function
//! words. The spell checker skips lines that score as code, and the word
//! suggestions draw on the document alone in code and never learn from it.

use crate::text::{floor_char_boundary, line_start};

/// Backward scan bound for Markdown fence detection.
const FENCE_SCAN_BYTES: usize = 4096;

/// Byte budget for classifying lines above the cursor.
const CLASSIFY_SCAN_BYTES: usize = 2048;

/// Byte budget for classifying lines below the cursor.
const CLASSIFY_FORWARD_BYTES: usize = 1024;

/// Mean weighted line score above which context is classified as code.
const CODE_SCORE_THRESHOLD: f64 = 0.2;

/// Maximum length of a candidate offered in prose context.
const PROSE_MAX_CANDIDATE_LEN: usize = 24;

/// Common English closed-class words, the prose signal of the code/prose
/// classifier.
const FUNCTION_WORDS: &[&str] = &[
    "the", "be", "to", "of", "and", "a", "an", "in", "that", "have", "it", "for", "not", "on",
    "with", "he", "as", "you", "do", "at", "this", "but", "his", "by", "from", "they", "we", "her",
    "she", "or", "will", "my", "one", "all", "would", "there", "their", "what", "so", "if", "is",
    "was", "are", "were", "been", "has", "had", "can", "could", "should", "i", "another", "each",
    "every", "these", "those", "many", "several", "few", "both",
];

/// Classification of the text surrounding the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextContext {
    Prose,
    Code,
}

/// Detects whether the cursor sits inside a Markdown code fence.
fn inside_code_fence(text: &str, offset: usize) -> bool {
    let floor = floor_char_boundary(text, offset.saturating_sub(FENCE_SCAN_BYTES));

    let region_start = if floor == 0 {
        0
    } else {
        match text[floor..offset].find('\n') {
            Some(index) => floor + index + 1,
            None => return false,
        }
    };

    let region = &text[region_start..line_start(text, offset)];
    let fences = region
        .lines()
        .filter(|line| line.trim_start().starts_with("```"))
        .count();

    fences % 2 == 1
}

/// Strips Markdown block markers from the start of a line.
fn strip_markup_prefix(text: &str) -> &str {
    let mut text = text;

    loop {
        let trimmed = text.trim_start();
        let mut characters = trimmed.chars();

        match characters.next() {
            Some('#') | Some('>') => {
                text = &trimmed[1..];
            }
            Some('-') | Some('*') | Some('+') if characters.next() == Some(' ') => {
                text = &trimmed[2..];
            }
            Some(character) if character.is_ascii_digit() => {
                let after_digits =
                    trimmed.trim_start_matches(|candidate: char| candidate.is_ascii_digit());

                match after_digits.strip_prefix(". ") {
                    Some(rest) => text = rest,
                    None => return trimmed,
                }
            }
            _ => return trimmed,
        }
    }
}

fn is_code_symbol(character: char) -> bool {
    matches!(
        character,
        '{' | '}'
            | '['
            | ']'
            | '('
            | ')'
            | '<'
            | '>'
            | '='
            | ';'
            | ':'
            | '&'
            | '|'
            | '+'
            | '*'
            | '/'
            | '\\'
            | '^'
            | '%'
            | '~'
            | '`'
            | '@'
            | '$'
            | '#'
    )
}

fn is_function_word(word: &str) -> bool {
    FUNCTION_WORDS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(word))
}

/// Tests for identifier-shaped tokens.
fn looks_like_identifier(token: &str) -> bool {
    if token.len() < 2 {
        return false;
    }

    if token.contains("::") || token.contains("->") || token.contains("=>") || token.contains("()")
    {
        return true;
    }

    let mut has_alpha = false;
    let mut has_digit = false;
    let mut has_underscore = false;
    let mut has_lower = false;
    let mut internal_upper = false;

    for (index, character) in token.chars().enumerate() {
        if character == '_' {
            has_underscore = true;
        }

        if character.is_ascii_digit() {
            has_digit = true;
        }

        if character.is_alphabetic() {
            has_alpha = true;

            if character.is_lowercase() {
                has_lower = true;
            } else if index > 0 {
                internal_upper = true;
            }
        }
    }

    (has_underscore && has_alpha) || (has_digit && has_alpha) || (internal_upper && has_lower)
}

/// Scores a line on a code-versus-prose axis in `[-1, 1]`.
pub(crate) fn line_code_score(line: &str) -> f64 {
    let indented = line.starts_with('\t') || line.starts_with("    ");
    let stripped = strip_markup_prefix(line).trim_end();

    if stripped.chars().count() < 3 {
        return 0.0;
    }

    let mut symbols = 0;
    let mut nonspace = 0;

    let mut characters = stripped.chars().peekable();
    while let Some(character) = characters.next() {
        if character.is_whitespace() {
            continue;
        }

        nonspace += 1;

        // A colon before a space is prose punctuation, as in "Agenda: budget,
        // hiring". Code puts one before a space too, in a type or a key, but
        // has other symbols to tell it by.
        let prose_colon =
            character == ':' && characters.peek().is_some_and(|next| next.is_whitespace());
        if is_code_symbol(character) && !prose_colon {
            symbols += 1;
        }
    }

    let mut tokens = 0;
    let mut identifier_tokens = 0;
    let mut function_words = 0;

    for token in stripped.split_whitespace() {
        tokens += 1;

        if looks_like_identifier(token) {
            identifier_tokens += 1;
        }

        let core = token.trim_matches(|character: char| !character.is_alphabetic());

        if !core.is_empty() && is_function_word(core) {
            function_words += 1;
        }
    }

    if tokens == 0 || nonspace == 0 {
        return 0.0;
    }

    let symbol_ratio = symbols as f64 / nonspace as f64;
    let identifier_ratio = identifier_tokens as f64 / tokens as f64;
    let function_word_ratio = function_words as f64 / tokens as f64;

    let mut score = 0.0;
    score += 2.0 * (symbol_ratio / 0.15).min(1.0);
    score += 1.5 * identifier_ratio;
    score -= 1.6 * (function_word_ratio / 0.35).min(1.0);

    if matches!(stripped.chars().last(), Some(';' | '{' | '}')) {
        score += 0.6;
    }

    if indented {
        score += 0.25;
    }

    score.clamp(-1.0, 1.0)
}

/// Classifies the cursor surroundings as code or prose.
pub(crate) fn classify_context(text: &str, offset: usize) -> TextContext {
    if inside_code_fence(text, offset) {
        return TextContext::Code;
    }

    let mut score = 0.0;
    let mut weight = 0.0;

    let current_start = line_start(text, offset);
    let current = &text[current_start..offset];

    if !current.trim().is_empty() {
        score += 3.0 * line_code_score(current);
        weight += 3.0;
    }

    let above_weights = [2.0, 1.5, 1.0, 0.75, 0.5, 0.5];
    let mut taken = 0;
    let mut scanned = 0;
    let mut end = current_start;

    while end > 0 && taken < above_weights.len() && scanned < CLASSIFY_SCAN_BYTES {
        let start = line_start(text, end - 1);
        let line = &text[start..end - 1];
        scanned += end - start;

        if !line.trim().is_empty() {
            score += above_weights[taken] * line_code_score(line);
            weight += above_weights[taken];
            taken += 1;
        }

        end = start;
    }

    let below_weights = [1.0, 0.75];
    let mut taken = 0;
    let mut scanned = 0;
    let mut position = text[offset..].find('\n').map(|index| offset + index + 1);

    while let Some(start) = position {
        if taken >= below_weights.len() || scanned > CLASSIFY_FORWARD_BYTES || start >= text.len() {
            break;
        }

        let end = text[start..]
            .find('\n')
            .map(|index| start + index)
            .unwrap_or(text.len());

        let line = &text[start..end];
        scanned += end - start;

        if !line.trim().is_empty() {
            score += below_weights[taken] * line_code_score(line);
            weight += below_weights[taken];
            taken += 1;
        }

        position = if end < text.len() {
            Some(end + 1)
        } else {
            None
        };
    }

    if weight == 0.0 {
        return TextContext::Prose;
    }

    if score / weight > CODE_SCORE_THRESHOLD {
        TextContext::Code
    } else {
        TextContext::Prose
    }
}

/// Rejects code-shaped candidates in prose context.
pub(crate) fn is_prose_shaped(word: &str) -> bool {
    if word.chars().count() > PROSE_MAX_CANDIDATE_LEN {
        return false;
    }

    let mut has_lower = false;
    let mut internal_upper = false;

    for (index, character) in word.chars().enumerate() {
        if character == '_' || character.is_ascii_digit() {
            return false;
        }

        if character.is_uppercase() {
            if index > 0 {
                internal_upper = true;
            }
        } else if character.is_lowercase() {
            has_lower = true;
        }
    }

    !(internal_upper && has_lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A note with a colon in it reads as prose, while code with colons in
    /// it stays code.
    #[test]
    fn a_colon_before_a_space_reads_as_prose() {
        for line in [
            "Pick up groceries: eggs, milk, bread",
            "Call Mom re: Thanksgiving plans",
            "Notes 10/12: ask about PTO, send the invoice",
        ] {
            assert_eq!(
                classify_context(line, line.len()),
                TextContext::Prose,
                "{line}"
            );
        }
        for code in [
            "fn shift(range: Range<usize>, new_len: usize) -> usize {\n    let end: usize = range.end;",
            "def area(width: int, height: int) -> int:\n    return {\"width\": width}",
        ] {
            assert_eq!(
                classify_context(code, code.len()),
                TextContext::Code,
                "{code}"
            );
        }
    }
}
