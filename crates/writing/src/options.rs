use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The default name of the vocabulary file in the data folder.
const VOCABULARY_FILE: &str = "vocabulary.json";

/// The default name of the user's word list in the data folder.
const USER_DICTIONARY_FILE: &str = "user-dictionary.txt";

/// Where the writing aids keep their files, and which words the dictionary
/// accepts.
///
/// Without a data folder nothing is read or saved: the vocabulary starts
/// empty and lasts the session, and **Add to Dictionary** lasts the session
/// too.
///
/// ```
/// use gpui_kit_writing::{WordList, WritingOptions};
///
/// let options = WritingOptions::new()
///     .with_data_dir("/home/me/.config/my-app")
///     .with_word_lists([WordList::English, WordList::File("/usr/share/dict/words".into())]);
/// assert_eq!(
///     options.vocabulary_path().unwrap(),
///     std::path::Path::new("/home/me/.config/my-app/vocabulary.json"),
/// );
/// ```
#[derive(Clone, Debug)]
pub struct WritingOptions {
    data_dir: Option<PathBuf>,
    vocabulary_file: String,
    user_dictionary_file: String,
    word_lists: Vec<WordList>,
    vocabulary_cap: u64,
}

impl Default for WritingOptions {
    fn default() -> Self {
        Self {
            data_dir: None,
            vocabulary_file: VOCABULARY_FILE.into(),
            user_dictionary_file: USER_DICTIONARY_FILE.into(),
            word_lists: WordList::defaults(),
            vocabulary_cap: crate::completion::VOCAB_MAX_BYTES,
        }
    }
}

impl WritingOptions {
    /// No data folder, `vocabulary.json` and `user-dictionary.txt` as the
    /// file names, the bundled English word list and a 16 MiB vocabulary.
    pub fn new() -> Self {
        Self::default()
    }

    /// The folder the vocabulary and the user's word list live in, such as
    /// the application's configuration folder. It is created at the first
    /// save.
    pub fn with_data_dir(mut self, data_dir: impl Into<PathBuf>) -> Self {
        self.data_dir = Some(data_dir.into());
        self
    }

    /// The name of the file in the data folder that holds what the word
    /// suggestions learned. Default: `vocabulary.json`. A lock file with the
    /// same stem and the extension `lock` sits beside it.
    pub fn with_vocabulary_file(mut self, vocabulary_file: impl Into<String>) -> Self {
        self.vocabulary_file = vocabulary_file.into();
        self
    }

    /// The name of the file in the data folder that **Add to Dictionary**
    /// appends to, one word per line. It is read with the word lists.
    /// Default: `user-dictionary.txt`.
    pub fn with_user_dictionary_file(mut self, user_dictionary_file: impl Into<String>) -> Self {
        self.user_dictionary_file = user_dictionary_file.into();
        self
    }

    /// The word lists the dictionary accepts, in place of the default
    /// [`WordList::English`].
    pub fn with_word_lists(mut self, word_lists: impl IntoIterator<Item = WordList>) -> Self {
        self.word_lists = word_lists.into_iter().collect();
        self
    }

    /// The most bytes the vocabulary file may take. A save that would make
    /// it larger drops the rarest entries until it takes seven eighths of
    /// this. Default: 16 MiB.
    ///
    /// The vocabulary in memory takes about six and a half times its file:
    /// about 100 MB at the default cap. A save in the background briefly
    /// needs that much again, to read the file another process may have
    /// added to. A smaller cap keeps less of what the user types.
    pub fn with_vocabulary_cap(mut self, bytes: u64) -> Self {
        self.vocabulary_cap = bytes;
        self
    }

    pub fn data_dir(&self) -> Option<&Path> {
        self.data_dir.as_deref()
    }

    pub fn vocabulary_file(&self) -> &str {
        &self.vocabulary_file
    }

    pub fn user_dictionary_file(&self) -> &str {
        &self.user_dictionary_file
    }

    pub fn word_lists(&self) -> &[WordList] {
        &self.word_lists
    }

    pub fn vocabulary_cap(&self) -> u64 {
        self.vocabulary_cap
    }

    /// Where the vocabulary is saved, when there is a data folder.
    pub fn vocabulary_path(&self) -> Option<PathBuf> {
        Some(self.data_dir.as_ref()?.join(&self.vocabulary_file))
    }

    /// Where **Add to Dictionary** writes, when there is a data folder.
    pub fn user_dictionary_path(&self) -> Option<PathBuf> {
        Some(self.data_dir.as_ref()?.join(&self.user_dictionary_file))
    }
}

/// Options that create a file only its owner can read and write, on Unix:
/// the vocabulary and the user's word list hold what the user wrote.
pub(crate) fn private_file() -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    owner_only(&mut options);
    options
}

#[cfg(unix)]
fn owner_only(options: &mut std::fs::OpenOptions) {
    std::os::unix::fs::OpenOptionsExt::mode(options, 0o600);
}

/// Elsewhere a new file takes the folder's permissions.
#[cfg(not(unix))]
fn owner_only(_: &mut std::fs::OpenOptions) {}

/// Words a dictionary accepts: one per line, with blank lines and lines
/// starting with `#` skipped. Hunspell affix flags (`word/MS`) are stripped
/// but not expanded, so a Hunspell `.dic` gives only its stems.
///
/// A dictionary needs at least 10,000 words in all to check spelling. With
/// fewer, such as a user's word list alone, it accepts every word.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WordList {
    /// American English up to SCOWL size 70, about 170,000 words, with each
    /// accented word also listed without its accents. Bundled by the
    /// `english` feature, on by default.
    #[cfg(feature = "english")]
    English,
    /// A file, read when the dictionary loads. A missing or unreadable file
    /// is skipped.
    File(PathBuf),
    /// Words given as text.
    Text(Arc<str>),
}

impl WordList {
    fn defaults() -> Vec<Self> {
        #[cfg(feature = "english")]
        return vec![Self::English];
        #[cfg(not(feature = "english"))]
        return Vec::new();
    }

    /// The list's words as text, or `None` for a file that can't be read.
    pub(crate) fn read(&self) -> Option<std::borrow::Cow<'static, str>> {
        match self {
            #[cfg(feature = "english")]
            Self::English => Some(std::borrow::Cow::Borrowed(ENGLISH)),
            Self::File(path) => std::fs::read_to_string(path)
                .ok()
                .map(std::borrow::Cow::Owned),
            Self::Text(text) => Some(std::borrow::Cow::Owned(text.to_string())),
        }
    }
}

/// The bundled English word list. See the crate's NOTICE file.
#[cfg(feature = "english")]
const ENGLISH: &str = include_str!("../assets/en.txt");
