use crate::WritingOptions;
use crate::completion::vocabulary::{LoadedVocabulary, Vocabulary};
use crate::completion::{SuggestionMode, SuggestionPacing, WordSuggestions};
use crate::spelling::{Dictionary, DictionaryChecker, Words};
use gpui::{App, Task};
use gpui_base::input::refresh_spelling;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The writing aids an application shares between its textareas: the
/// spelling dictionary, the vocabulary the word suggestions learn from what
/// the user types, and how eagerly suggestions show.
///
/// Load one when the application starts, keep it, and hand each textarea
/// its own [`DictionaryChecker`] and [`WordSuggestions`]. Clones share
/// everything.
///
/// ```ignore
/// let writing = Writing::load(WritingOptions::new().with_data_dir(data_dir), cx);
///
/// let suggestions = writing.word_suggestions();
/// let textarea = cx.new(|cx| {
///     TextareaState::new(window, cx)
///         .spell_checker(writing.spell_checker())
///         .suggestion_provider(suggestions.clone())
///         .suggestion_options(SuggestionOptions::default().menu(false).inline(true))
/// });
/// let pacing = suggestions.follow(&textarea, cx);
///
/// // Learning is saved when the application says so.
/// writing.save_in_background(cx).detach();
/// ```
#[derive(Clone)]
pub struct Writing {
    dictionary: Rc<Dictionary>,
    vocabulary: Rc<RefCell<Vocabulary>>,
    pacing: Rc<SuggestionPacing>,
    loaded: Rc<Cell<bool>>,
}

/// What loading reads, on a background thread.
struct Read {
    words: Words,
    vocabulary: Option<LoadedVocabulary>,
}

impl Read {
    fn read(options: &WritingOptions) -> Self {
        Self {
            words: Words::read(
                options.word_lists(),
                options.user_dictionary_path().as_deref(),
            ),
            vocabulary: options.vocabulary_path().map(LoadedVocabulary::read),
        }
    }
}

impl Writing {
    /// Starts loading the dictionary and the vocabulary `options` name, and
    /// returns at once. They are read on a background thread. Until they
    /// arrive, the spell checkers find no word misspelled and suggestions
    /// draw on what the user types and the document's own words; then every
    /// textarea's spelling is checked again.
    ///
    /// Suggestions start in [`SuggestionMode::Quiet`].
    pub fn load(options: WritingOptions, cx: &mut App) -> Self {
        let writing = Self::unloaded(&options);
        let read = cx
            .background_executor()
            .spawn(async move { Read::read(&options) });
        let loading = writing.clone();
        cx.spawn(async move |cx| {
            let read = read.await;
            cx.update(|cx| {
                loading.install(read);
                refresh_spelling(cx);
            });
        })
        .detach();
        writing
    }

    /// Loads the dictionary and the vocabulary `options` name on this
    /// thread, as [`Self::load`] does on another. For tests and tools.
    pub fn load_blocking(options: WritingOptions) -> Self {
        let writing = Self::unloaded(&options);
        writing.install(Read::read(&options));
        writing
    }

    /// Whether the dictionary and the vocabulary have arrived.
    pub fn is_loaded(&self) -> bool {
        self.loaded.get()
    }

    fn unloaded(options: &WritingOptions) -> Self {
        let dictionary = Rc::new(Dictionary::unloaded(options.user_dictionary_path()));
        let mut vocabulary = Vocabulary::with_dictionary(dictionary.clone());
        vocabulary.set_cap(options.vocabulary_cap());
        Self {
            loaded: Rc::new(Cell::new(false)),
            ..Self::from_parts(dictionary, vocabulary)
        }
    }

    /// Takes what loading read, with what was learned and added meanwhile.
    fn install(&self, read: Read) {
        self.dictionary.install(read.words);
        if let Some(vocabulary) = read.vocabulary {
            self.vocabulary.borrow_mut().install(vocabulary);
        }
        self.loaded.set(true);
    }

    /// Writing aids that read and save nothing: a dictionary that accepts
    /// every word and an empty vocabulary that lasts the session. For tests,
    /// and for applications that don't check spelling.
    pub fn in_memory() -> Self {
        Self::from_parts(Rc::new(Dictionary::empty()), Vocabulary::new())
    }

    pub(crate) fn from_parts(dictionary: Rc<Dictionary>, vocabulary: Vocabulary) -> Self {
        Self {
            dictionary,
            vocabulary: Rc::new(RefCell::new(vocabulary)),
            pacing: Rc::new(SuggestionPacing::new(SuggestionMode::default())),
            loaded: Rc::new(Cell::new(true)),
        }
    }

    /// The spelling dictionary every textarea's checker shares.
    pub fn dictionary(&self) -> &Rc<Dictionary> {
        &self.dictionary
    }

    /// A spell checker for one textarea. Give each textarea its own.
    pub fn spell_checker(&self) -> Rc<DictionaryChecker> {
        Rc::new(DictionaryChecker::new(self.dictionary.clone()))
    }

    /// Word suggestions for one textarea. Give each textarea its own.
    pub fn word_suggestions(&self) -> Rc<WordSuggestions> {
        Rc::new(WordSuggestions::new(
            self.vocabulary.clone(),
            self.pacing.clone(),
        ))
    }

    /// How eagerly suggestions show in every textarea.
    pub fn suggestion_mode(&self) -> SuggestionMode {
        self.pacing.mode()
    }

    /// Switches every textarea to `mode` from its next keystroke.
    pub fn set_suggestion_mode(&self, mode: SuggestionMode) {
        self.pacing.set_mode(mode);
    }

    /// Adds what the suggestions learned since the last save to the
    /// vocabulary file, on this thread, after waiting for a background save
    /// under way, and appends the words added to the dictionary that its
    /// background thread hasn't yet. For quitting, where nothing can wait for
    /// [`Self::save_in_background`].
    ///
    /// It waits at most a second for a background save, and half a second
    /// for another process's save; past either, the learning since the last
    /// save stays unsaved. Saving a vocabulary near the default 16 MiB cap
    /// takes a few hundred milliseconds. Where the application can, it closes
    /// its windows, awaits [`Self::save_in_background`] and then quits.
    pub fn save(&self) {
        self.dictionary.save_added_words();
        self.vocabulary.borrow_mut().save();
    }

    /// Saves as [`Self::save`] does, with the file's work on a background
    /// thread. The task finishes when the save does; detach it, or keep it.
    /// Saves asked for while one runs make one more save after it.
    pub fn save_in_background(&self, cx: &App) -> Task<()> {
        Vocabulary::save_in_background(&self.vocabulary, cx)
    }

    #[cfg(test)]
    pub(crate) fn vocabulary(&self) -> &Rc<RefCell<Vocabulary>> {
        &self.vocabulary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WordList;
    use crate::spelling::tests::Scratch;
    use gpui::{TestAppContext, VisualTestContext, px, size};
    use gpui_base::input::{SpellChecker as _, TextareaState};

    /// The files go where the options say, under the names they give.
    #[gpui::test]
    fn files_live_in_the_data_folder_under_their_names(cx: &mut TestAppContext) {
        let scratch = Scratch::new("data-folder");
        let options = WritingOptions::new()
            .with_data_dir(scratch.0.join("app"))
            .with_vocabulary_file("words.json")
            .with_user_dictionary_file("mine.txt")
            .with_word_lists([WordList::English]);
        let writing = cx.update(|cx| Writing::load(options.clone(), cx));
        cx.run_until_parked();
        assert!(writing.is_loaded());
        cx.update(|cx| writing.dictionary().add_word("Wojciech", cx));
        writing
            .vocabulary()
            .borrow_mut()
            .learn_sentence("Ferns grow slowly");
        writing.save();

        let mut names: Vec<String> = std::fs::read_dir(scratch.0.join("app"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["mine.txt", "words.json", "words.lock"]);

        let reloaded = Writing::load_blocking(options);
        assert!(reloaded.dictionary().is_correct("Wojciech"));
        assert_eq!(
            reloaded.vocabulary().borrow().counts().word_count("ferns"),
            1
        );
    }

    /// **Add to Dictionary** accepts the word at once and appends it to the
    /// user's word list on a background thread. A save at quit appends what
    /// that thread hasn't yet, once.
    #[gpui::test]
    fn added_words_are_written_off_the_ui_thread(cx: &mut TestAppContext) {
        let scratch = Scratch::new("added-words");
        let options = WritingOptions::new().with_data_dir(scratch.0.join("app"));
        let writing = Writing::load_blocking(options.clone());
        let list = options.user_dictionary_path().unwrap();
        let read = || std::fs::read_to_string(&list).unwrap();
        let checker = writing.spell_checker();
        assert!(!writing.dictionary().is_correct("Zbigniewski"));

        cx.update(|cx| checker.add_to_dictionary("Zbigniewski", cx));
        assert!(writing.dictionary().is_correct("Zbigniewski"));
        assert!(!list.exists(), "written on the UI thread");
        cx.run_until_parked();
        assert_eq!(read(), "Zbigniewski\n");

        cx.update(|cx| {
            writing.dictionary().add_word("Wojciech", cx);
            writing.dictionary().add_word("Przybylski", cx);
        });
        writing.save();
        assert_eq!(read(), "Zbigniewski\nWojciech\nPrzybylski\n");
        cx.run_until_parked();
        assert_eq!(read(), "Zbigniewski\nWojciech\nPrzybylski\n");
        assert!(
            Writing::load_blocking(options)
                .dictionary()
                .is_correct("Wojciech")
        );
    }

    /// The vocabulary file stays within the cap the options give.
    #[test]
    fn the_vocabulary_keeps_within_its_cap() {
        let scratch = Scratch::new("vocabulary-cap");
        let cap = 4096;
        let options = WritingOptions::new()
            .with_data_dir(scratch.0.clone())
            .with_vocabulary_cap(cap);
        let writing = Writing::load_blocking(options.clone());
        for number in 0..400 {
            writing
                .vocabulary()
                .borrow_mut()
                .learn_sentence(&format!("Notes on topic{number} today"));
        }
        writing.save();
        let saved = std::fs::metadata(options.vocabulary_path().unwrap())
            .unwrap()
            .len();
        assert!(saved <= cap / 8 * 7, "{saved} bytes, cap {cap}");
        let reloaded = Writing::load_blocking(options);
        assert_eq!(
            reloaded.vocabulary().borrow().counts().word_count("notes"),
            400
        );
    }

    /// Loading happens on another thread. What the user adds and teaches
    /// meanwhile is kept, and saved with what was read.
    #[gpui::test]
    fn what_is_learned_while_loading_is_kept(cx: &mut TestAppContext) {
        let scratch = Scratch::new("loading");
        let options = WritingOptions::new().with_data_dir(scratch.0.clone());
        let earlier = Writing::load_blocking(options.clone());
        earlier
            .vocabulary()
            .borrow_mut()
            .learn_sentence("Ferns grow slowly");
        earlier.save();

        let writing = cx.update(|cx| Writing::load(options.clone(), cx));
        assert!(!writing.is_loaded());
        assert!(writing.dictionary().is_correct("mispeled"));
        cx.update(|cx| writing.dictionary().add_word("Wojciech", cx));
        writing
            .vocabulary()
            .borrow_mut()
            .learn_sentence("Ferns need shade");
        cx.run_until_parked();

        assert!(writing.is_loaded());
        assert!(!writing.dictionary().is_correct("mispeled"));
        assert!(writing.dictionary().is_correct("Wojciech"));
        let ferns = |writing: &Writing| writing.vocabulary().borrow().counts().word_count("ferns");
        assert_eq!(ferns(&writing), 2);
        writing.save();
        assert_eq!(ferns(&Writing::load_blocking(options)), 2);
    }

    /// A textarea checked by the writing aids underlines what the dictionary
    /// doesn't know, and offers its corrections.
    #[gpui::test]
    fn a_textarea_underlines_misspellings(cx: &mut TestAppContext) {
        cx.update(gpui_base::init);
        let writing = cx.update(|cx| Writing::load(WritingOptions::new(), cx));
        let window = cx.open_window(size(px(600.), px(200.)), |window, cx| {
            TextareaState::new(window, cx)
                .spell_checker(writing.spell_checker())
                .default_value("This line is mispeled here.")
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        let textarea = window.root(&mut cx).unwrap();
        let word = textarea.update(&mut cx, |state, cx| {
            let ranges = state.misspellings();
            assert_eq!(ranges, [13..21]);
            let misspelling = state.misspelling_at(15).unwrap();
            (misspelling.word().clone(), misspelling.suggestions(cx))
        });
        assert_eq!(word.0, "mispeled");
        assert_eq!(word.1.first().map(|word| word.as_ref()), Some("misspelled"));
    }

    /// In a text editor, a word paused in isn't underlined while the caret
    /// is in it, and a finished word is checked at once, as the textarea
    /// does for any checker.
    #[gpui::test]
    fn a_word_is_underlined_once_it_is_finished(cx: &mut TestAppContext) {
        cx.update(gpui_base::init);
        let writing = Writing::load_blocking(WritingOptions::new());
        let window = cx.open_window(size(px(600.), px(200.)), |window, cx| {
            TextareaState::new(window, cx)
                .text_editor()
                .spell_checker(writing.spell_checker())
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let textarea = window.root(&mut cx).unwrap();
        cx.update(|window, cx| {
            textarea.update(cx, |state, cx| state.focus(window, cx));
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
        let marked = |cx: &mut VisualTestContext| {
            textarea.read_with(cx, |state, _| {
                state
                    .misspellings()
                    .into_iter()
                    .map(|range| state.text().slice(range).to_string())
                    .collect::<Vec<_>>()
            })
        };

        cx.simulate_input("The methodol");
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        assert_eq!(marked(&mut cx), Vec::<String>::new(), "paused in the word");
        cx.simulate_input("ogy ");
        assert_eq!(marked(&mut cx), Vec::<String>::new());
        cx.simulate_input("is mispeled ");
        assert_eq!(marked(&mut cx), ["mispeled"], "without a pause");
    }
}
