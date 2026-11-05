//! Whether text is code or prose.
//!
//! The classifier reads the text before the caret a word at a time, as it
//! was typed. Each word gets a code vote and a prose vote from its spelling.
//! An underscore, a call, an operator or a keyword rare in English is code,
//! and a function word or a plain English word is prose. Paths, URLs and
//! Markdown markup vote neither way. A line's score is the mean of its
//! votes and its density of code symbols, with a little for an indent, a
//! comment lead or a Markdown lead. The lines above count through a decayed
//! mean of their scores, which a closing fence resets, since prose follows a
//! block of code. At the caret the line counts as many times as it has
//! words, against two for the lines above. The text is code when the sum
//! passes zero, inside a fence, and inside an inline span of backticks.
//!
//! The spell checker skips lines that score as code, and the word
//! suggestions draw on the document alone in code and never learn from it.

use crate::text::{floor_char_boundary, line_start};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;

/// How far above the caret's line fences are counted.
const FENCE_SCAN_BYTES: usize = 12 * 1024;

/// How far above the caret's line the lines above are read.
const CONTEXT_SCAN_BYTES: usize = 2048;

/// Maximum length of a candidate offered in prose context.
const PROSE_MAX_CANDIDATE_LEN: usize = 24;

/// The weight of a line's mean vote in its score.
const VOTE_WEIGHT: f64 = 1.2;

/// The weight of a line's density of code symbols in its score.
const SYMBOL_WEIGHT: f64 = 0.6;

/// The density of code symbols at which their term is full.
const SYMBOL_FULL: f64 = 0.15;

/// Added for a line indented by a tab or four spaces.
const INDENT: f64 = 0.25;

/// Added for a line that starts with a Markdown heading, bullet, quote,
/// number or table row.
const MARKUP_LEAD: f64 = -0.2;

/// Added for a line that starts with `//` or `/*`, and for a `#` lead as far
/// as the lines above read as code, where it opens a comment rather than a
/// heading.
const COMMENT: f64 = 0.6;

/// The context at which a `#` lead counts fully as a comment.
const COMMENT_CONTEXT_FULL: f64 = 0.3;

/// How many of the caret line's words the lines above count as.
const CONTEXT_WORDS: f64 = 2.0;

/// The share of the context a finished line keeps.
const CONTEXT_DECAY: f64 = 0.5;

/// The share of the context a blank line keeps.
const BLANK_LINE_DECAY: f64 = 0.5;

/// The context at the start of a text and after a closing fence, which
/// leans to prose.
const START_CONTEXT: f64 = -0.3;

/// English function words that aren't keywords in common languages.
const PROSE_FUNCTION_WORDS: &str = "a about after again all already also although always am an \
    another any are aren't at be because been before being between both but by can can't cannot \
    could couldn't dear did didn't does doesn't don't during each even every few get got had has \
    hasn't have haven't he he's hello her hi him his how i i'd i'll i'm i've into isn't it it's \
    its just know like little made make many me more most much my need never no of often on one \
    only other our out over own please quite rather really same several she she's should \
    shouldn't since so some sometimes still such than thank thanks that that's the their them \
    then there there's these they they're think this those though through to too until up upon \
    us usually very want was wasn't we we're were weren't what when where whether which who why \
    will within without won't would wouldn't yes you you're your";

/// English function words that are keywords too, as in "for i in range" or
/// "if not x".
const SHARED_FUNCTION_WORDS: &str = "and as do else for from if import in is not or return try \
    use while with";

/// Keywords and names rare in English, code wherever they stand.
const STRONG_KEYWORDS: &str = "apt argc argv async await awk cargo chmod chown cin const cout \
    curl def elif elsif endforeach endl enum esac f32 f64 fi float32 float64 fn foreach func \
    grep gzip i32 i64 impl instanceof int16 int32 int64 int8 iostream isize journalctl kubectl \
    kwargs lambda malloc mkdir mut nil none npm npx nullptr pip pip3 printf println proc pub \
    rsync scp sed sizeof ssh std stderr stdin stdio stdlib stdout struct sudo systemctl tar tmux \
    typeof u32 u64 u8 uint uint32 uint64 uint8 undefined unsigned unzip usize utf8 var vec wget \
    yarn yield zsh";

/// Names that are code more often than not but turn up in prose.
const WEAK_KEYWORDS: &str = "abs arg args bash bin bool buf cd cfg char cls cmd cp css ctx dir \
    docker double dst echo env err float fmt git html idx int json len lib ls max min msg mv \
    node null num obj opt php pkg private ptr public python req res rm self sql src static str \
    string sum tmp usr val vim void xml";

/// Keywords that are English words. At the head of a line and in lowercase
/// they open code more often than a sentence, which would be capitalised.
const HEAD_KEYWORDS: &str = "as assert break case catch class continue delete do else export \
    finally for from function if import include let mod namespace new package print raise \
    require return switch this throw try use using while with";

/// Operators that are code when they stand alone, sorted.
const OPERATORS: &[&str] = &[
    "!=", "!==", "&&", "&=", "**", "*=", "++", "+=", "--", "-=", "->", "..<", "..=", "/=", "::",
    ":=", "<-", "<<", "<=", "=", "==", "===", "=>", ">=", ">>", "^=", "|=", "||",
];

/// Operators that are code between word characters, as in `a->b`.
const INFIX_OPERATORS: &[&str] = &[
    "==", "!=", "<=", ">=", "->", "=>", "::", "+=", "-=", "**", "&&", "||",
];

/// Top-level domains that make a word a web address.
const DOMAINS: &[&str] = &["com", "org", "net", "io", "dev", "edu", "gov", "co.uk"];

/// File extensions that make a word a path.
const EXTENSIONS: &[&str] = &[
    "py", "rs", "js", "ts", "tsx", "jsx", "md", "txt", "json", "yaml", "yml", "toml", "html",
    "css", "sh", "c", "cpp", "h", "hpp", "java", "go", "exe", "csv", "png", "jpg", "pdf", "lock",
    "cfg", "ini", "log", "sql", "xml", "rb", "php",
];

/// Units after a number, as in `5th`, `10px` or `3pm`.
const NUMBER_UNITS: &[&str] = &[
    "st", "nd", "rd", "th", "s", "k", "m", "b", "x", "px", "pt", "am", "pm", "ms", "mb", "gb", "kb",
];

const CODE_SYMBOLS: &str = "{}[]()<>=;:&|+*/\\^%~@$#";

/// Classification of the text surrounding the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextContext {
    Prose,
    Code,
}

/// What a word's spelling says about it.
#[derive(Clone, Copy)]
enum Flag {
    /// A Markdown heading, bullet, quote, number, table row, rule or `**`.
    Markup,
    /// A web or email address.
    Url,
    Path,
    /// An English function word that isn't a keyword.
    ProseWord,
    /// An English function word that is a keyword too.
    SharedWord,
    /// Lowercase letters with a vowel.
    EnglishWord,
    Capitalised,
    /// A lowercase letter followed by a capital.
    CamelCase,
    /// An underscore inside the word.
    Underscore,
    LettersAndDigits,
    Number,
    /// A call, `name(` or `name[`.
    Call,
    /// A member, `a.b`.
    Member,
    /// An operator on its own.
    Operator,
    /// An operator between word characters, as in `x=1`.
    Infix,
    /// An HTML or XML tag.
    Tag,
    /// Brackets and semicolons alone.
    Brace,
    EndsInSemicolon,
    EndsInBrace,
    /// Ends in `):` or `]:`.
    EndsInParenColon,
    StrongKeyword,
    WeakKeyword,
    /// A single letter other than `a` and `I`.
    LoneLetter,
    /// Lowercase letters with no vowel, as in `cfg`.
    NoVowel,
    /// Sentence punctuation after a letter.
    EndsSentence,
    /// A keyword in lowercase at the head of the line.
    HeadKeyword,
    /// `//`, `/*` or `*/` at the head of the line.
    CommentHead,
    /// A capitalised word at the head of the line.
    CapitalisedHead,
    /// A shared function word after a word that looked like code.
    SharedAfterCode,
    /// An English word after a word that looked like code.
    WordAfterCode,
    /// A Markdown heading's `#` at the head of the line.
    HashLead,
    /// A number with a unit, as in `5th` or `10px`.
    NumberUnit,
    /// Letters and digits with no lowercase letter, as in `PG16`.
    CapitalsAndDigits,
}

/// The code vote of each flag, in the order the votes add up.
const CODE_VOTES: [(Flag, f64); 24] = [
    (Flag::Markup, -3.0),
    (Flag::Url, -3.0),
    (Flag::Path, 0.7),
    (Flag::ProseWord, -0.5),
    (Flag::CamelCase, 0.5),
    (Flag::Underscore, 1.0),
    (Flag::LettersAndDigits, 0.5),
    (Flag::Call, 1.0),
    (Flag::Member, 0.8),
    (Flag::Operator, 1.0),
    (Flag::Infix, 1.0),
    (Flag::Tag, 1.0),
    (Flag::Brace, 1.0),
    (Flag::EndsInSemicolon, 0.4),
    (Flag::EndsInBrace, 1.0),
    (Flag::EndsInParenColon, 1.0),
    (Flag::StrongKeyword, 1.0),
    (Flag::WeakKeyword, 0.5),
    (Flag::LoneLetter, 0.5),
    (Flag::NoVowel, 0.3),
    (Flag::HeadKeyword, 0.5),
    (Flag::CommentHead, 1.0),
    (Flag::NumberUnit, -3.0),
    (Flag::CapitalsAndDigits, -0.3),
];

/// The prose vote of each flag, in the order the votes add up.
const PROSE_VOTES: [(Flag, f64); 27] = [
    (Flag::Markup, -3.0),
    (Flag::Url, -3.0),
    (Flag::Path, -3.0),
    (Flag::ProseWord, 1.0),
    (Flag::SharedWord, 0.6),
    (Flag::EnglishWord, 0.4),
    (Flag::Capitalised, 0.2),
    (Flag::CamelCase, -3.0),
    (Flag::Underscore, -3.0),
    (Flag::LettersAndDigits, -0.4),
    (Flag::Call, -3.0),
    (Flag::Member, -3.0),
    (Flag::Operator, -3.0),
    (Flag::Infix, -3.0),
    (Flag::Tag, -3.0),
    (Flag::Brace, -3.0),
    (Flag::EndsInSemicolon, -0.4),
    (Flag::EndsInBrace, -3.0),
    (Flag::EndsInParenColon, -3.0),
    (Flag::StrongKeyword, -3.0),
    (Flag::WeakKeyword, -0.4),
    (Flag::LoneLetter, -3.0),
    (Flag::EndsSentence, 0.3),
    (Flag::HeadKeyword, -3.0),
    (Flag::CapitalisedHead, 0.3),
    (Flag::SharedAfterCode, -0.6),
    (Flag::WordAfterCode, -0.2),
];

#[derive(Clone, Copy, Default)]
struct Flags(u64);

impl Flags {
    fn set(&mut self, flag: Flag) {
        self.0 |= 1 << flag as u32;
    }

    fn has(self, flag: Flag) -> bool {
        self.0 & (1 << flag as u32) != 0
    }

    /// The code and prose votes, each clamped to `[0, 1]`.
    fn votes(self) -> (f64, f64) {
        let vote = |weights: &[f64; FLAG_COUNT]| {
            let mut sum = 0.0;
            let mut bits = self.0;
            while bits != 0 {
                sum += weights[bits.trailing_zeros() as usize];
                bits &= bits - 1;
            }
            sum.clamp(0.0, 1.0)
        };
        (vote(&CODE_WEIGHTS), vote(&PROSE_WEIGHTS))
    }
}

const FLAG_COUNT: usize = Flag::CapitalsAndDigits as usize + 1;

/// The votes as a weight for every flag, 0 for the flags that don't vote.
const fn weights(votes: &[(Flag, f64)]) -> [f64; FLAG_COUNT] {
    let mut weights = [0.0; FLAG_COUNT];
    let mut index = 0;
    while index < votes.len() {
        weights[votes[index].0 as usize] = votes[index].1;
        index += 1;
    }
    weights
}

const CODE_WEIGHTS: [f64; FLAG_COUNT] = weights(&CODE_VOTES);
const PROSE_WEIGHTS: [f64; FLAG_COUNT] = weights(&PROSE_VOTES);

const IN_STRONG_KEYWORDS: u8 = 1;
const IN_PROSE_FUNCTION_WORDS: u8 = 1 << 1;
const IN_SHARED_FUNCTION_WORDS: u8 = 1 << 2;
const IN_WEAK_KEYWORDS: u8 = 1 << 3;
const IN_HEAD_KEYWORDS: u8 = 1 << 4;

/// The word lists `word`, in lowercase, is in, as `IN_` bits.
fn word_lists(word: &str) -> u8 {
    static LISTS: LazyLock<HashMap<&str, u8>> = LazyLock::new(|| {
        let mut lists = HashMap::new();
        for (words, bit) in [
            (STRONG_KEYWORDS, IN_STRONG_KEYWORDS),
            (PROSE_FUNCTION_WORDS, IN_PROSE_FUNCTION_WORDS),
            (SHARED_FUNCTION_WORDS, IN_SHARED_FUNCTION_WORDS),
            (WEAK_KEYWORDS, IN_WEAK_KEYWORDS),
            (HEAD_KEYWORDS, IN_HEAD_KEYWORDS),
        ] {
            for word in words.split_whitespace() {
                *lists.entry(word).or_default() |= bit;
            }
        }
        lists
    });
    LISTS.get(word).copied().unwrap_or(0)
}

/// A word character, as regular expressions have it.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// A digit in any script, superscripts included. Fractions and Roman
/// numerals are numbers but no digits, so "¾-inch" is a word.
fn is_digit(c: char) -> bool {
    c.is_numeric() && !matches!(c, '¼'..='¾' | '\u{2150}'..='\u{218f}')
}

/// The ASCII characters of a word, so that a test is skipped when the word
/// lacks every character the test looks for.
#[derive(Clone, Copy)]
struct Present(u128);

impl Present {
    fn of(word: &str) -> Self {
        Self(
            word.bytes()
                .filter(u8::is_ascii)
                .fold(0, |set, b| set | 1 << b),
        )
    }

    fn any(self, chars: u128) -> bool {
        self.0 & chars != 0
    }
}

const fn ascii_set(chars: &[u8]) -> u128 {
    let mut set = 0;
    let mut index = 0;
    while index < chars.len() {
        set |= 1 << chars[index];
        index += 1;
    }
    set
}

const URL_CHARS: u128 = ascii_set(b":.@");
const PATH_CHARS: u128 = ascii_set(b"/.\\");
const INFIX_CHARS: u128 = ascii_set(b"=!<>-:+*&|^%");
const DIGITS: u128 = ascii_set(b"0123456789");
const SYMBOLS: u128 = ascii_set(CODE_SYMBOLS.as_bytes());

/// `word` in ASCII lowercase, in `buffer`, when it could be in a word list.
fn lowercase_in<'a>(word: &str, buffer: &'a mut [u8; 16]) -> Option<&'a str> {
    if word.len() > buffer.len() || !word.is_ascii() {
        return None;
    }
    let lower = &mut buffer[..word.len()];
    lower.copy_from_slice(word.as_bytes());
    lower.make_ascii_lowercase();
    std::str::from_utf8(lower).ok()
}

/// A heading's `#`s, a bullet, a quote or a list number, at a line's head.
fn is_markup_lead(token: &str) -> bool {
    let bytes = token.as_bytes();
    match bytes {
        [b'-' | b'*' | b'+' | b'>'] => true,
        [b'#', ..] => bytes.len() <= 6 && bytes.iter().all(|&b| b == b'#'),
        [digits @ .., b'.' | b')'] => {
            (1..=3).contains(&digits.len()) && digits.iter().all(u8::is_ascii_digit)
        }
        _ => false,
    }
}

/// A table row's pipes, a rule, or emphasis markers alone.
fn is_markup(token: &str) -> bool {
    let bytes = token.as_bytes();
    match bytes {
        [b'|', rest @ ..] => rest.iter().all(|b| b"-:|".contains(b)),
        [first @ (b'*' | b'_' | b'-' | b'='), _, ..] => bytes.iter().all(|b| b == first),
        _ => false,
    }
}

fn is_url(token: &str) -> bool {
    token.contains("://") || token.starts_with("www.") || is_domain(token) || is_email(token)
}

/// A domain name, such as `example.com/path`, maybe in a Markdown link's
/// bracket.
fn is_domain(token: &str) -> bool {
    let rest = token.strip_prefix('[').unwrap_or(token);
    let name_end = rest
        .find(|c: char| !(is_word_char(c) || c == '.' || c == '-'))
        .unwrap_or(rest.len());
    rest[..name_end]
        .match_indices('.')
        .filter(|&(dot, _)| dot > 0)
        .any(|(dot, _)| {
            let after = &rest[dot + 1..];
            DOMAINS.iter().any(|domain| {
                after.strip_prefix(domain).is_some_and(|next| {
                    next.is_empty() || next.starts_with(['/', '?', '#', ')', ']'])
                })
            })
        })
}

fn is_email(token: &str) -> bool {
    let Some((name, domain)) = token.split_once('@') else {
        return false;
    };
    let label_end = domain
        .find(|c: char| !(is_word_char(c) || c == '-'))
        .unwrap_or(domain.len());
    !name.is_empty()
        && name
            .chars()
            .all(|c| is_word_char(c) || matches!(c, '.' | '+' | '-'))
        && label_end > 0
        && domain[label_end..]
            .strip_prefix('.')
            .and_then(|rest| rest.chars().next())
            .is_some_and(is_word_char)
}

fn is_path(token: &str) -> bool {
    let rest = token.strip_prefix(['[', '(', '"', '\'']).unwrap_or(token);
    let is_path_char = |c: char| is_word_char(c) || c == '.' || c == '-';
    // `/`, `~/`, `./` or `../` at the start.
    let rooted = ["/", "~/", "./", "../"]
        .iter()
        .any(|start| rest.starts_with(start));
    // Three names joined by slashes at the start, as in `docs/notes/plan.md`.
    let nested = || {
        let mut rest = rest;
        for part in 0..3 {
            if part > 0 {
                let Some(after) = rest.strip_prefix('/') else {
                    return false;
                };
                rest = after;
            }
            let name = rest.find(|c| !is_path_char(c)).unwrap_or(rest.len());
            if name == 0 {
                return false;
            }
            rest = &rest[name..];
        }
        true
    };
    let drive = || {
        let mut chars = rest.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.next() == Some(':')
            && chars.next() == Some('\\')
    };
    let extension = || {
        token.match_indices('.').any(|(dot, _)| {
            let after = &token[dot + 1..];
            EXTENSIONS.iter().any(|extension| {
                after.strip_prefix(extension).is_some_and(|rest| {
                    rest.chars()
                        .all(|c| matches!(c, '.' | ',' | ':' | ';' | ')' | ']' | '"' | '\''))
                })
            })
        })
    };
    rooted || nested() || drive() || extension()
}

/// An operator between word characters, as in `x=1`, `a->b` or `i++`.
fn has_infix(word: &str) -> bool {
    for (index, c) in word.char_indices() {
        if !is_word_char(c) {
            continue;
        }
        let after = &word[index + c.len_utf8()..];
        if INFIX_OPERATORS
            .iter()
            .any(|operator| after.starts_with(operator))
        {
            return true;
        }
        let mut next = after.chars();
        if next.next().is_some_and(|c| "=+*<>|&^%".contains(c))
            && next.next().is_some_and(is_word_char)
        {
            return true;
        }
        if after == "++" || after == "--" {
            return true;
        }
    }
    false
}

/// A call or an index, as in `name(` or `items[`.
fn has_call(word: &str) -> bool {
    let unquoted = word.trim_end_matches(['"', '\'']).as_bytes();
    let opens = unquoted.windows(2).any(|pair| {
        (pair[0].is_ascii_alphanumeric() || matches!(pair[0], b'_' | b']' | b')'))
            && matches!(pair[1], b'(' | b'[')
    });
    let called = word.as_bytes().windows(2).any(|pair| {
        let name = pair[0].is_ascii_alphabetic() || pair[0] == b'_';
        (name && pair[1] == b'(') || ((name || pair[0] == b']') && pair[1] == b'[')
    });
    opens && called
}

/// A member, as in `self.items` or `list().len`.
fn has_member(word: &str) -> bool {
    word.match_indices('.').any(|(dot, _)| {
        let next = word.as_bytes().get(dot + 1);
        if !next.is_some_and(|&b| b.is_ascii_lowercase() || b == b'_') {
            return false;
        }
        let mut before = word[..dot].chars().rev();
        match before.next() {
            Some(')' | ']') => true,
            Some(c) if is_word_char(c) => before.next().is_some_and(is_word_char),
            _ => false,
        }
    })
}

/// A number with a unit, as in `5th`, `10px` or `3pm`.
fn is_number_with_unit(core: &str) -> bool {
    let unit = core.trim_start_matches(|c: char| c.is_ascii_digit());
    unit.len() < core.len()
        && NUMBER_UNITS
            .iter()
            .any(|known| unit.eq_ignore_ascii_case(known))
}

/// Letters with an optional apostrophe part, as in "it's".
fn is_english_shaped(core: &str) -> bool {
    let (word, rest) = match core.find(['\'', '\u{2019}']) {
        Some(index) => (&core[..index], Some(&core[index..])),
        None => (core, None),
    };
    let letters = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphabetic());
    letters(word)
        && rest.is_none_or(|rest| {
            let apostrophe = rest.chars().next().map_or(0, char::len_utf8);
            letters(&rest[apostrophe..])
        })
}

/// What the spelling of `token`, a word or what is typed of it, says about
/// it. `head` says whether it is the first word of its line, and
/// `after_code` whether the word before it looked like code.
fn token_flags(token: &str, present: Present, head: bool, after_code: bool) -> Flags {
    let mut flags = Flags::default();
    if head && is_markup_lead(token) || is_markup(token) {
        flags.set(Flag::Markup);
        if head && token.starts_with('#') {
            flags.set(Flag::HashLead);
        }
        return flags;
    }
    // A table row written without spaces, "|17|Romania|312.49", stands for
    // its longest cell.
    let (token, present) = if present.any(const { ascii_set(b"|") }) {
        let mut longest = "";
        let mut longest_chars = 0;
        for cell in token.split('|') {
            let chars = cell.chars().count();
            if chars > longest_chars {
                (longest, longest_chars) = (cell, chars);
            }
        }
        if longest.is_empty() {
            flags.set(Flag::Markup);
            return flags;
        }
        (longest, Present::of(longest))
    } else {
        (token, present)
    };
    if present.any(URL_CHARS) && is_url(token) {
        flags.set(Flag::Url);
        return flags;
    }
    if present.any(PATH_CHARS) && is_path(token) {
        flags.set(Flag::Path);
        return flags;
    }
    let core = token.trim_matches(|c: char| !c.is_alphanumeric());
    let bare = token.trim_matches(['*', '_', '"', '\'', '“', '”', '‘', '’', '(', '[', '{']);
    let tail = token.trim_end_matches(['*', '_', '"', '\'', '“', '”', '‘', '’']);

    let mut chars = token.chars();
    if (chars.next() == Some('<') && chars.next().is_some_and(char::is_alphabetic))
        || ["</", "<?", "<!"]
            .iter()
            .any(|start| token.starts_with(start))
    {
        flags.set(Flag::Tag);
    }
    let operator = |token: &str| OPERATORS.binary_search(&token).is_ok();
    if token.starts_with([
        '!', '&', '*', '+', '-', '.', '/', ':', '<', '=', '>', '^', '|',
    ]) && (operator(token)
        || (token.chars().nth(1).is_some() && operator(token.trim_end_matches([',', ';']))))
    {
        flags.set(Flag::Operator);
    }
    let in_braces = token.trim_matches(['(', ')', ',', ';']);
    if (!in_braces.is_empty() && in_braces.chars().all(|c| "{}[]".contains(c)))
        || (token.chars().all(|c| "{}()[];,".contains(c)) && token.contains(['{', '}', ';']))
    {
        flags.set(Flag::Brace);
    }
    if head && (token.starts_with("//") || token.starts_with("/*") || token.starts_with("*/")) {
        flags.set(Flag::CommentHead);
    }
    if tail.ends_with(';') {
        flags.set(Flag::EndsInSemicolon);
    }
    if tail.ends_with(['{', '}']) || ["};", "});", "})"].iter().any(|end| tail.ends_with(end)) {
        flags.set(Flag::EndsInBrace);
    }
    if tail.ends_with("):") || tail.ends_with("]:") {
        flags.set(Flag::EndsInParenColon);
    }
    if core.is_empty() {
        return flags;
    }

    // Markdown emphasis around a word, as in "**Space**", is no operator.
    let inner = token.trim_matches(['*', '_']);
    if present.any(INFIX_CHARS) && has_infix(inner) {
        flags.set(Flag::Infix);
    }
    if present.any(const { ascii_set(b"([") }) && has_call(inner) {
        flags.set(Flag::Call);
    } else if present.any(const { ascii_set(b".") }) && has_member(inner) {
        flags.set(Flag::Member);
    }
    let digits = present.any(DIGITS);
    if digits && is_number_with_unit(core) {
        flags.set(Flag::NumberUnit);
        return flags;
    }
    if digits
        && core.chars().all(|c| {
            c.is_ascii_digit() || matches!(c, '.' | ',' | '%' | ':' | '/' | '-' | '–' | 'x')
        })
    {
        flags.set(Flag::Number);
        return flags;
    }
    if present.any(const { ascii_set(b"_") }) && bare.trim_matches('_').contains('_') {
        flags.set(Flag::Underscore);
        return flags;
    }
    if (digits || !core.is_ascii())
        && core.chars().any(is_digit)
        && core.chars().any(char::is_alphabetic)
    {
        flags.set(Flag::LettersAndDigits);
        // "PG16", "COVID19" and "MP3" are names. Code's are lowercase, as
        // in "u8" and "utf8".
        if !core.chars().any(char::is_lowercase) {
            flags.set(Flag::CapitalsAndDigits);
        }
        return flags;
    }
    let mut core_chars = core.chars();
    if let (Some(letter), None) = (core_chars.next(), core_chars.next())
        && letter.is_alphabetic()
    {
        if matches!(letter, 'a' | 'A' | 'i' | 'I') {
            flags.set(Flag::ProseWord);
        } else if letter.is_ascii() {
            // Code names its variables in ASCII. A letter of another
            // script alone is a word, such as Russian "в" or "и".
            flags.set(Flag::LoneLetter);
        }
        return flags;
    }
    if core
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0].is_ascii_lowercase() && pair[1].is_ascii_uppercase())
    {
        flags.set(Flag::CamelCase);
        return flags;
    }
    let mut buffer = [0; 16];
    let lists = lowercase_in(core, &mut buffer).map_or(0, word_lists);
    if lists & IN_STRONG_KEYWORDS != 0 {
        flags.set(Flag::StrongKeyword);
        return flags;
    }
    if head && lists & IN_HEAD_KEYWORDS != 0 && !core.bytes().any(|b| b.is_ascii_uppercase()) {
        flags.set(Flag::HeadKeyword);
    }
    if lists & IN_PROSE_FUNCTION_WORDS != 0 {
        flags.set(Flag::ProseWord);
    } else if lists & IN_SHARED_FUNCTION_WORDS != 0 {
        flags.set(Flag::SharedWord);
        if after_code {
            flags.set(Flag::SharedAfterCode);
        }
    } else if is_english_shaped(core) {
        if lists & IN_WEAK_KEYWORDS != 0 {
            flags.set(Flag::WeakKeyword);
        }
        if !core.bytes().any(|b| b.is_ascii_lowercase()) {
            // All capitals, an acronym or a shout, votes neither way. A
            // single letter was read above.
        } else if core.as_bytes()[0].is_ascii_uppercase() {
            flags.set(Flag::Capitalised);
            if head {
                flags.set(Flag::CapitalisedHead);
            }
            if after_code {
                flags.set(Flag::WordAfterCode);
            }
        } else {
            if !core.bytes().any(|b| b"aeiouyAEIOUY".contains(&b)) {
                flags.set(Flag::NoVowel);
            }
            flags.set(Flag::EnglishWord);
            if after_code {
                flags.set(Flag::WordAfterCode);
            }
        }
    }
    if tail.ends_with(['.', '?', '!']) && core.chars().next_back().is_some_and(char::is_alphabetic)
    {
        flags.set(Flag::EndsSentence);
    }
    flags
}

/// How many code symbols `token` has. It leaves out those Markdown and
/// prose use, which are a colon that ends it, brackets around a word,
/// emphasis, a currency sign, a percent sign and a heading's `#`.
fn code_symbols_in(token: &str, present: Present, flags: Flags) -> usize {
    // An operator alone voted already, and a lone "=" in a note is no code.
    if !present.any(SYMBOLS)
        || [
            Flag::Markup,
            Flag::Url,
            Flag::Path,
            Flag::Number,
            Flag::NumberUnit,
            Flag::Operator,
        ]
        .iter()
        .any(|&flag| flags.has(flag))
    {
        return 0;
    }
    // Where the closing punctuation that ends the token starts.
    let closing = token.trim_end_matches(|c| ")*_.,;:!?\"'".contains(c)).len();
    let mut count = 0;
    let mut previous = None;
    let mut chars = token.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let following = chars.peek().map(|&(_, c)| c);
        let first = index == 0;
        let last = following.is_none();
        let emphasis = |c: Option<char>| matches!(c, Some('*' | '_'));
        let symbol = c.is_ascii() && SYMBOLS & 1 << c as u32 != 0;
        let skipped = !symbol
            || c == ':' && following.is_none_or(char::is_whitespace)
            || c == '(' && first
            || c == ')' && index >= closing
            || c == '*' && (first || last || emphasis(previous) || emphasis(following))
            || c == '$' && following.is_some_and(is_digit)
            || c == '%' && previous.is_some_and(is_digit)
            || c == '#' && first;
        if !skipped {
            count += 1;
        }
        previous = Some(c);
    }
    count
}

/// The line being read, word by word.
#[derive(Clone, Copy)]
struct Line {
    indented: bool,
    /// How many words voted.
    words: usize,
    code_votes: f64,
    prose_votes: f64,
    symbols: usize,
    nonspace: usize,
    /// The length of the run of backticks that opened an inline span, or 0.
    open_run: usize,
    markup_lead: bool,
    hash_lead: bool,
    comment_head: bool,
    after_code: bool,
    head: bool,
}

impl Line {
    fn new(text: &str) -> Self {
        Self {
            indented: is_indented(text),
            words: 0,
            code_votes: 0.0,
            prose_votes: 0.0,
            symbols: 0,
            nonspace: 0,
            open_run: 0,
            markup_lead: false,
            hash_lead: false,
            comment_head: false,
            after_code: false,
            head: true,
        }
    }

    /// A line read whole.
    fn read(text: &str) -> Self {
        let mut line = Self::new(text);
        for token in text.split_whitespace() {
            line.add(token);
        }
        line
    }

    /// Adds a word, whole or what is typed of it.
    fn add(&mut self, token: &str) {
        let present = Present::of(token);
        let flags = token_flags(token, present, self.head, self.after_code);
        self.symbols += code_symbols_in(token, present, flags);
        self.nonspace += token.chars().count();
        // Inline code as CommonMark reads it, where a run of n backticks
        // opens a span that the next run of exactly n closes.
        if present.any(const { ascii_set(b"`") }) {
            for run in token.split(|c| c != '`').filter(|run| !run.is_empty()) {
                if self.open_run == 0 {
                    self.open_run = run.len();
                } else if self.open_run == run.len() {
                    self.open_run = 0;
                }
            }
        }
        if flags.has(Flag::Markup) {
            self.markup_lead |= self.head;
            self.hash_lead |= flags.has(Flag::HashLead);
            return;
        }
        let (code, prose) = flags.votes();
        self.words += 1;
        self.code_votes += code;
        self.prose_votes += prose;
        self.comment_head |= flags.has(Flag::CommentHead);
        self.after_code = code >= 0.8;
        self.head = false;
    }

    /// The line's score in `[-1, 1]`, given the context of the lines above.
    fn score(&self, context: f64) -> f64 {
        if self.words == 0 {
            return 0.0;
        }
        let mean_vote = (self.code_votes - self.prose_votes) / self.words as f64;
        let density = (self.symbols as f64 / self.nonspace.max(1) as f64 / SYMBOL_FULL).min(1.0);
        let term = |set: bool, weight: f64| if set { weight } else { 0.0 };
        let comment =
            term(self.hash_lead, COMMENT) * (context / COMMENT_CONTEXT_FULL).clamp(0.0, 1.0);
        let score = VOTE_WEIGHT * mean_vote
            + SYMBOL_WEIGHT * density
            + term(self.indented, INDENT)
            + term(self.markup_lead, MARKUP_LEAD)
            + comment
            + term(self.comment_head, COMMENT);
        score.clamp(-1.0, 1.0)
    }
}

/// The lines read so far.
struct Stream {
    fence_open: bool,
    /// A decayed mean of the finished lines' scores.
    context: f64,
}

impl Stream {
    fn new() -> Self {
        Self {
            fence_open: false,
            context: START_CONTEXT,
        }
    }

    /// The code score at the end of `line`, the caret's, without the fence
    /// and the backticks.
    fn total(&self, line: &Line) -> f64 {
        if line.words == 0 {
            return self.context;
        }
        let words = line.words as f64;
        (words * line.score(self.context) + CONTEXT_WORDS * self.context) / (words + CONTEXT_WORDS)
    }

    fn is_code(&self, line: &Line) -> bool {
        self.fence_open || line.open_run != 0 || self.total(line) > 0.0
    }

    /// Takes in `line`, read from `text`, which it ended.
    fn finish_line(&mut self, text: &str, line: &Line) {
        if is_fence(text) {
            self.fence_open = !self.fence_open;
            if !self.fence_open {
                self.context = START_CONTEXT;
            }
        } else if self.fence_open {
            // Code inside a fence leaves the context as the fence found it.
        } else if text.trim().is_empty() {
            self.context *= BLANK_LINE_DECAY;
        } else {
            self.context =
                CONTEXT_DECAY * self.context + (1.0 - CONTEXT_DECAY) * line.score(self.context);
        }
    }
}

fn is_indented(line: &str) -> bool {
    line.starts_with('\t') || line.starts_with("    ")
}

fn is_fence(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

/// The first line start at or after byte `floor` of `text`, before `end`.
fn first_line_from(text: &str, floor: usize, end: usize) -> usize {
    if floor == 0 {
        return 0;
    }
    match text[floor..end].find('\n') {
        Some(index) => floor + index + 1,
        None => end,
    }
}

/// Reads `above`, the whole lines above the caret's line from the first
/// whose fence counts. It counts the fences in all of them, and reads the
/// lines from the last closing fence, at most [`CONTEXT_SCAN_BYTES`] of
/// them, for the context.
fn read_above(above: &str) -> Stream {
    let mut stream = Stream::new();
    let mut after_fence = 0;
    let mut position = 0;
    for line in above.split_inclusive('\n') {
        position += line.len();
        if is_fence(line) {
            stream.fence_open = !stream.fence_open;
            after_fence = position;
        }
    }
    if stream.fence_open {
        return stream;
    }
    let floor = floor_char_boundary(above, above.len().saturating_sub(CONTEXT_SCAN_BYTES));
    let start = after_fence.max(first_line_from(above, floor, above.len()));
    for text in above[start..].split_terminator('\n') {
        stream.finish_line(text, &Line::read(text));
    }
    stream
}

/// The last classification on this thread, so that the next, usually a
/// keystroke later on the same line, reads only the words typed since.
struct LastRead {
    /// The lines above the caret's line that it read, and what it made of
    /// them.
    above: String,
    stream: Stream,
    /// The caret's line up to the last space before the caret, and its
    /// words.
    line_text: String,
    line: Line,
}

thread_local! {
    static LAST_READ: RefCell<Option<LastRead>> = const { RefCell::new(None) };
}

impl LastRead {
    fn new(above: &str) -> Self {
        Self {
            above: above.to_string(),
            stream: read_above(above),
            line_text: String::new(),
            line: Line::new(""),
        }
    }

    /// Whether the caret's line, `current` up to the caret, is code.
    fn is_code(&mut self, current: &str) -> bool {
        if self.stream.fence_open {
            return true;
        }
        if !current.starts_with(&self.line_text) || is_indented(current) != self.line.indented {
            self.line_text.clear();
            self.line = Line::new(current);
        }
        let rest = &current[self.line_text.len()..];
        // The words a space follows are whole.
        let whole = rest
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map_or(0, |(index, c)| index + c.len_utf8());
        for token in rest[..whole].split_whitespace() {
            self.line.add(token);
        }
        self.line_text.push_str(&rest[..whole]);
        let mut line = self.line;
        if whole < rest.len() {
            line.add(&rest[whole..]);
        }
        self.stream.is_code(&line)
    }
}

/// Common English closed-class words, the prose signal of the spell
/// checker's line score.
const FUNCTION_WORDS: &[&str] = &[
    "the", "be", "to", "of", "and", "a", "an", "in", "that", "have", "it", "for", "not", "on",
    "with", "he", "as", "you", "do", "at", "this", "but", "his", "by", "from", "they", "we", "her",
    "she", "or", "will", "my", "one", "all", "would", "there", "their", "what", "so", "if", "is",
    "was", "are", "were", "been", "has", "had", "can", "could", "should", "i", "another", "each",
    "every", "these", "those", "many", "several", "few", "both",
];

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

/// Scores a line on a code-versus-prose axis in `[-1, 1]`, for the spell
/// checker, from its symbols, its identifier-shaped tokens and its share of
/// function words.
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

/// Classifies the text before byte `offset` of `text`, the caret, as code
/// or prose. It reads the lines in the [`FENCE_SCAN_BYTES`] above the
/// caret's line, as [`read_above`] does, and the caret's line up to the
/// caret.
pub(crate) fn classify_context(text: &str, offset: usize) -> TextContext {
    let line_start = line_start(text, offset);
    let fence_floor = floor_char_boundary(text, line_start.saturating_sub(FENCE_SCAN_BYTES));
    let above = &text[first_line_from(text, fence_floor, line_start)..line_start];
    let current = &text[line_start..offset];
    let code = LAST_READ.with_borrow_mut(|last_read| {
        let mut last = match last_read.take() {
            Some(last) if last.above == above => last,
            _ => LastRead::new(above),
        };
        let code = last.is_code(current);
        *last_read = Some(last);
        code
    });
    if code {
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

    /// Whether the end of `text` reads as code.
    fn ends_in_code(text: &str) -> bool {
        classify_context(text, text.len()) == TextContext::Code
    }

    /// The first line after a block of code is prose from its first word.
    #[test]
    fn the_line_after_a_fence_is_prose() {
        let code =
            "fn total(items: &[Item]) -> u32 {\n    items.iter().map(|item| item.price).sum()\n}\n";
        let text = format!("Here is the fix:\n```rust\n{code}```\nThanks, that works");
        for (end, _) in text.match_indices([',', ' ']).chain([(text.len(), "")]) {
            let line = &text[..end];
            if line.ends_with("```") || !line.contains("```\n") {
                continue;
            }
            assert!(!ends_in_code(line), "{line}");
        }
        assert!(ends_in_code(&format!(
            "Here is the fix:\n```rust\n{code}    let"
        )));
    }

    /// A word in an open backtick span is code, and the prose after the
    /// span closes is prose, as CommonMark pairs the runs of backticks.
    #[test]
    fn inline_code_is_code_until_its_backtick_closes() {
        assert!(ends_in_code(
            "If you want the old behaviour back, you can set the `strict"
        ));
        assert!(!ends_in_code("Rename `parse_line` before the"));
        assert!(ends_in_code("Set ``font: bo"));
        assert!(!ends_in_code("Set ``font: bold`` on the heading and"));
    }

    /// Markdown's own symbols, the pipes of a table, a heading's `#`, bold
    /// markers and the brackets of a link, are no sign of code.
    #[test]
    fn markdown_reads_as_prose() {
        for text in [
            "| Plan | Price | Seats |\n|---|---|---|\n| Team | $12 a month | 5",
            "|Basic|Monthly billing|Email support",
            "### Q3 Roadmap",
            "## Budget (Draft)",
            "1. **Back up the photos:** copy them to the drive",
            "- [Field guide](https://example.com/guide) (birds of the coast)",
        ] {
            assert!(!ends_in_code(text), "{text}");
        }
    }

    /// A path or a web address in a sentence leaves the sentence prose.
    #[test]
    fn a_path_or_a_url_in_a_sentence_is_prose() {
        for text in [
            "Logs go to /var/log/app/server.log now",
            "Read https://example.com/guide/start before you",
            "The notes are in docs/release/plan.md if you need",
            "Ask ada@example.com about the",
        ] {
            assert!(!ends_in_code(text), "{text}");
        }
    }

    /// Code pasted without a fence reads as code, keywords that are English
    /// words and all.
    #[test]
    fn pasted_code_without_a_fence_is_code() {
        for text in [
            "Here is what I ran:\nfor name in names:\n    print(name",
            "for item in items:\n    if item is not None:\n        total += item",
            "Sure! Try this:\nimport os\nfor f in os.listdir():\n    if f.endswith",
        ] {
            assert!(ends_in_code(text), "{text}");
        }
    }

    /// The last read speeds up the next classification and changes none.
    /// At every caret, visited forwards, backwards and at random, the
    /// answer is the one read afresh.
    #[test]
    fn the_last_read_changes_no_answer() {
        let text = "Some notes first.\n\n```\nfn main() {\n    let x = 1;\n}\n```\nAnd prose \
                    with `code` in it.\nfoo(bar);\n    indented\n| a | b |\n# Heading\n\
                    import os\nfrom sys import argv\nThanks for the help";
        let carets: Vec<usize> = (0..=text.len())
            .filter(|&caret| text.is_char_boundary(caret))
            .collect();
        let afresh: Vec<TextContext> = carets
            .iter()
            .map(|&caret| {
                LAST_READ.set(None);
                classify_context(text, caret)
            })
            .collect();
        let mut order: Vec<usize> = (0..carets.len()).collect();
        let check = |order: &[usize]| {
            for &index in order {
                assert_eq!(
                    classify_context(text, carets[index]),
                    afresh[index],
                    "at {}",
                    carets[index]
                );
            }
        };
        check(&order);
        order.reverse();
        check(&order);
        let mut state = 7u64;
        for index in (1..order.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            order.swap(index, (state >> 33) as usize % (index + 1));
        }
        check(&order);
    }

    /// Typing, deleting and editing anywhere, a change of indent and wide
    /// spaces included, leave the last read giving the answer read afresh.
    #[test]
    fn the_last_read_follows_edits() {
        // Two more spaces before the first word make the line indented.
        classify_context("  ", 2);
        assert_eq!(classify_context("    x", 5), TextContext::Code);
        let pieces = [
            "fn", "the", "x", "=", "(", ")", "`", "```", "\n", "\n", "\r\n", " ", " ", "  ", "\t",
            "\u{3000}", "\u{a0}", "é", "я", "我", "foo.bar", "#", "|", "{", ";", "we", "and",
            "print(", "//",
        ];
        let mut text = String::new();
        let mut caret = 0;
        let mut state = 7u64;
        let mut next = |below: usize| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 33) as usize % below
        };
        for step in 0..20_000 {
            let piece = pieces[next(pieces.len())];
            match next(8) {
                // Type at the caret.
                0..=3 => {
                    text.insert_str(caret, piece);
                    caret += piece.len();
                }
                // Delete the character before the caret.
                4 => {
                    if let Some(c) = text[..caret].chars().next_back() {
                        caret -= c.len_utf8();
                        text.remove(caret);
                    }
                }
                // Insert or replace somewhere else, above the caret too.
                5 | 6 => {
                    let at = floor_char_boundary(&text, next(text.len() + 1));
                    if let Some(c) = text[at..].chars().next().filter(|_| next(2) == 0) {
                        text.replace_range(at..at + c.len_utf8(), piece);
                        if at < caret {
                            caret = caret + piece.len() - c.len_utf8();
                        }
                    } else {
                        text.insert_str(at, piece);
                        if at <= caret {
                            caret += piece.len();
                        }
                    }
                }
                _ => caret = floor_char_boundary(&text, next(text.len() + 1)),
            }
            if text.len() > 600 {
                let cut = floor_char_boundary(&text, text.len() - 400);
                text.drain(..cut);
                caret = text.len();
            }
            let remembered = classify_context(&text, caret);
            let last = LAST_READ.take();
            let afresh = classify_context(&text, caret);
            LAST_READ.set(last);
            assert_eq!(remembered, afresh, "step {step}, {text:?} at {caret}");
        }
    }

    /// Russian and Ukrainian prose, with its one-letter words, is prose.
    #[test]
    fn a_letter_of_another_script_alone_is_a_word() {
        for text in [
            "Я думаю, что в этом году мы поедем к морю и в горы",
            "Я була в Києві з друзями і в Львові",
        ] {
            for (end, _) in text.match_indices(' ').chain([(text.len(), "")]) {
                assert!(!ends_in_code(&text[..end]), "{}", &text[..end]);
            }
        }
    }

    #[test]
    fn the_operators_are_sorted() {
        assert!(OPERATORS.is_sorted());
    }
}
