use std::rc::Rc;

use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement as _,
    Render, Styled as _, Subscription, Window, div,
};

use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _,
    button::Button,
    h_flex,
    input::{SuggestionOptions, TextEditor, TextareaState},
    switch::Switch,
    v_flex,
};
use gpui_kit::writing::{SuggestionMode, WordSuggestions, Writing, WritingOptions};

const DOCUMENT: &str = "\
Spell checking and word suggestions from GPUI Kit Writing

This text is checked against the bundled English word list, so a typo like recieve or seperate \
gets a wavy underline. Right-click it for corrections.

Word suggestions learn from what you type here and nowhere else. Finish a sentence with a period, \
then start it again: the rest of it shows after the caret, and Tab takes it.
";

pub struct WritingStory {
    writing: Writing,
    document: Entity<TextareaState>,
    spell_check: bool,
    _subscriptions: Vec<Subscription>,
}

impl super::Story for WritingStory {
    fn title() -> &'static str {
        "Writing"
    }

    fn description() -> &'static str {
        "Spell checking against a real dictionary, and word suggestions learned from what you type."
    }

    fn closable() -> bool {
        false
    }

    fn new_view(window: &mut Window, cx: &mut App) -> Entity<impl Render> {
        Self::view(window, cx)
    }
}

impl WritingStory {
    pub fn view(window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(window, cx))
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // No data folder: what the story learns lasts until it closes.
        let writing = Writing::load(WritingOptions::new(), cx);
        let suggestions: Rc<WordSuggestions> = writing.word_suggestions();
        suggestions.opened_with(DOCUMENT);
        let document = cx.new(|cx| {
            TextareaState::new(window, cx)
                .text_editor()
                .spell_checker(writing.spell_checker())
                .suggestion_provider(suggestions.clone())
                .suggestion_options(SuggestionOptions::default().menu(false).inline(true))
                .default_value(DOCUMENT)
        });
        let pacing = suggestions.follow(&document, cx);
        Self {
            writing,
            document,
            spell_check: true,
            _subscriptions: vec![pacing],
        }
    }
}

impl Focusable for WritingStory {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.document.focus_handle(cx)
    }
}

impl Render for WritingStory {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = self.writing.suggestion_mode();
        v_flex()
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .gap_4()
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
                    .child(h_flex().gap_1().children(
                        [
                            (SuggestionMode::Off, "Off"),
                            (SuggestionMode::Quiet, "Quiet"),
                            (SuggestionMode::Eager, "Eager"),
                        ]
                        .map(|(choice, label)| {
                            Button::new(label)
                                .xsmall()
                                .outline()
                                .label(label)
                                .selected(mode == choice)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.writing.set_suggestion_mode(choice);
                                    this.document
                                        .update(cx, |document, cx| document.focus(window, cx));
                                    cx.notify();
                                }))
                        }),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(match mode {
                                SuggestionMode::Off => "No word suggestions",
                                SuggestionMode::Quiet => {
                                    "Suggestions show when typing pauses, less often for words you type past"
                                }
                                _ => "A suggestion after every keystroke",
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
