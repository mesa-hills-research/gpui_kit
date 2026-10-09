use std::{cell::RefCell, collections::HashSet, rc::Rc};

use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, Window, div,
};

use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _, WindowExt as _,
    button::Button,
    h_flex,
    input::{
        Keymap, RopeExt as _, SaveBuffer, SpellCheck, SpellCheckRequest, SpellChecker, SpellEvent,
        Suggestion, SuggestionOptions, SuggestionProvider, SuggestionRequest, TextEditor,
        TextareaState, VimQuit, VimWrite,
    },
    switch::Switch,
    v_flex,
};

const DOCUMENT: &str = "\
A text editor for prose

The editor numbers its lines, wraps long ones at the edge of the view and has a find panel. \
Words the spell checker doesn't know get a wavy underline, like teh and recieve here.

Right-click an underlined word to replace it, add it to the dictionary or ignore it. \
Shift-F10 or the Menu key opens the same menu at the caret.

As you type, the rest of a word shows after the caret. Press Tab to take it: try typing \"docu\" or \"sugg\".

Some more misspellings to try: seperate, definately, occured and untill.";

/// Misspellings the story's checker knows, each with its corrections.
const CORRECTIONS: &[(&str, &[&str])] = &[
    ("teh", &["the", "tech", "ten"]),
    ("recieve", &["receive", "relieve", "deceive"]),
    ("seperate", &["separate", "desperate"]),
    ("definately", &["definitely", "defiantly"]),
    ("occured", &["occurred", "occurs"]),
    ("untill", &["until", "untie"]),
    ("wich", &["which", "wish", "witch"]),
    ("beleive", &["believe", "belie"]),
    ("adress", &["address", "dress"]),
    ("tommorow", &["tomorrow"]),
];

/// Words the story completes as you type.
const VOCABULARY: &[&str] = &[
    "dictionary",
    "document",
    "documentation",
    "editor",
    "misspelling",
    "paragraph",
    "replacement",
    "suggestion",
    "suggestions",
    "underline",
];

/// A stand-in for a real dictionary: it marks the common misspellings in
/// [`CORRECTIONS`] and suggests their corrections. An application checks
/// against its own word list and ranks suggestions itself.
#[derive(Default)]
struct CommonMisspellings {
    added: RefCell<HashSet<String>>,
}

impl SpellChecker for CommonMisspellings {
    fn check(&self, request: &SpellCheckRequest, _: &mut App) -> Task<anyhow::Result<SpellCheck>> {
        let mut misspelled = Vec::new();
        for range in request.ranges() {
            let text = request.text().slice(range.clone()).to_string();
            let mut offset = range.start;
            for word in text.split(|c: char| !c.is_alphabetic()) {
                let lower = word.to_lowercase();
                if CORRECTIONS.iter().any(|(wrong, _)| *wrong == lower)
                    && !self.added.borrow().contains(&lower)
                {
                    misspelled.push(offset..offset + word.len());
                }
                offset += word.len() + 1;
            }
        }
        Task::ready(Ok(SpellCheck {
            misspelled,
            ..Default::default()
        }))
    }

    fn suggestions(&self, word: &str, _: &mut App) -> Vec<SharedString> {
        let lower = word.to_lowercase();
        CORRECTIONS
            .iter()
            .find(|(wrong, _)| *wrong == lower)
            .map(|(_, right)| right.iter().map(|word| SharedString::from(*word)).collect())
            .unwrap_or_default()
    }

    fn add_to_dictionary(&self, word: &str, _: &mut App) {
        self.added.borrow_mut().insert(word.to_lowercase());
    }
}

/// Completes the word before the caret from [`VOCABULARY`].
struct Vocabulary;

impl SuggestionProvider for Vocabulary {
    fn suggestions(
        &self,
        request: &SuggestionRequest,
        _: &mut Window,
        _: &mut App,
    ) -> Task<anyhow::Result<Vec<Suggestion>>> {
        let (text, offset) = (request.text(), request.offset());
        let line_start = text.line_start_offset(text.offset_to_point(offset).row);
        let before = text.slice(line_start..offset).to_string();
        let typed = before
            .rsplit(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap_or_default();
        if typed.len() < 2 {
            return Task::ready(Ok(Vec::new()));
        }
        let start = offset - typed.len();
        let suggestions = VOCABULARY
            .iter()
            .filter(|word| word.starts_with(typed) && word.len() > typed.len())
            .map(|word| Suggestion::new(*word).with_range(start..offset))
            .collect();
        Task::ready(Ok(suggestions))
    }
}

pub struct TextEditorStory {
    document: Entity<TextareaState>,
    line_numbers: bool,
    spell_check: bool,
    last_event: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl super::Story for TextEditorStory {
    fn title() -> &'static str {
        "Text Editor"
    }

    fn description() -> &'static str {
        "A textarea set up for prose: line numbers, spell checking, completions and CUA, Emacs or Vim keys."
    }

    fn closable() -> bool {
        false
    }

    fn new_view(window: &mut Window, cx: &mut App) -> Entity<impl Render> {
        Self::view(window, cx)
    }
}

impl TextEditorStory {
    pub fn view(window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(window, cx))
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let document = cx.new(|cx| {
            TextareaState::new(window, cx)
                .text_editor()
                .spell_checker(Rc::new(CommonMisspellings::default()))
                .suggestion_provider(Rc::new(Vocabulary))
                .suggestion_options(SuggestionOptions::default().menu(false).inline(true))
                .default_value(DOCUMENT)
        });
        let on_spelling = cx.subscribe(&document, |this, _, event: &SpellEvent, cx| {
            this.last_event = Some(
                match event {
                    SpellEvent::Replaced { word, with } => {
                        format!("Replaced \u{201c}{word}\u{201d} with \u{201c}{with}\u{201d}")
                    }
                    SpellEvent::AddedToDictionary { word } => {
                        format!("Added \u{201c}{word}\u{201d} to the dictionary")
                    }
                    SpellEvent::Ignored { word } => format!("Ignoring \u{201c}{word}\u{201d}"),
                    _ => return,
                }
                .into(),
            );
            cx.notify();
        });
        // The caret position and the keymap's mode label follow the document.
        let on_change = cx.observe(&document, |_, _, cx| cx.notify());
        Self {
            document,
            line_numbers: true,
            spell_check: true,
            last_event: None,
            _subscriptions: vec![on_spelling, on_change],
        }
    }
}

impl gpui_kit::Focusable for TextEditorStory {
    fn focus_handle(&self, cx: &App) -> gpui_kit::FocusHandle {
        self.document.focus_handle(cx)
    }
}

impl Render for TextEditorStory {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let document = self.document.read(cx);
        let position = document.cursor_position();
        let keymap = document.current_keymap();
        let mode = document.keymap_mode_label();
        v_flex()
            .size_full()
            .gap_3()
            // Emacs's C-x C-s and Vim's :w and :q leave saving and closing to
            // the application.
            .on_action(cx.listener(|_, _: &SaveBuffer, window, cx| {
                window.push_notification("C-x C-s: the application saves here", cx)
            }))
            .on_action(cx.listener(|_, _: &VimWrite, window, cx| {
                window.push_notification(":w: the application saves here", cx)
            }))
            .on_action(cx.listener(|_, _: &VimQuit, window, cx| {
                window.push_notification(":q: the application closes the document here", cx)
            }))
            .child(
                h_flex()
                    .gap_4()
                    .child(
                        Switch::new("line-numbers")
                            .small()
                            .label("Line numbers")
                            .checked(self.line_numbers)
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                this.line_numbers = *checked;
                                this.document.update(cx, |document, cx| {
                                    document.set_line_number(*checked, window, cx)
                                });
                                cx.notify();
                            })),
                    )
                    .child(
                        Switch::new("spell-check")
                            .small()
                            .label("Spell check")
                            .checked(self.spell_check)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.spell_check = *checked;
                                this.document.update(cx, |document, cx| {
                                    document.set_spell_checking(*checked, cx)
                                });
                                cx.notify();
                            })),
                    )
                    .child(h_flex().gap_1().children(Keymap::ALL.map(|scheme| {
                        let label = match scheme {
                            Keymap::Cua => "CUA",
                            Keymap::Emacs => "Emacs",
                            Keymap::Vim => "Vim",
                        };
                        Button::new(scheme.name())
                            .xsmall()
                            .outline()
                            .label(label)
                            .selected(keymap == scheme)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.document.update(cx, |document, cx| {
                                    document.set_keymap(scheme, cx);
                                    document.focus(window, cx);
                                });
                            }))
                    })))
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.last_event.clone().unwrap_or_else(|| {
                                "Right-click an underlined word to fix it".into()
                            })),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(match mode {
                                Some(mode) => format!(
                                    "{mode}  {}:{}",
                                    position.line + 1,
                                    position.character + 1
                                ),
                                None => format!("{}:{}", position.line + 1, position.character + 1),
                            }),
                    ),
            )
            .child(
                div()
                    .min_h_0()
                    .flex_1()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(TextEditor::new(&self.document)),
            )
    }
}
