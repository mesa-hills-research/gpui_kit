use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, ClickEvent, ClipboardEntry, Context, Entity, Focusable,
    InteractiveElement, IntoElement, KeyBinding, ParentElement as _, Render, SharedString, Styled,
    Subscription, Task, Window, div, px,
};

use crate::{ChangeStorySize, section, story_toolbar};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable, Size,
    attachment::{
        Attachment, AttachmentActions, AttachmentContent, AttachmentDescription, AttachmentGroup,
        AttachmentMedia, AttachmentTitle,
    },
    button::{Button, ButtonVariants as _},
    h_flex,
    hover_card::HoverCard,
    input::{
        AcceptSuggestionWord, InputEvent, RopeExt as _, ShowSuggestions, Suggestion,
        SuggestionEvent, SuggestionOptions, SuggestionProvider, SuggestionRequest, Textarea,
        TextareaState,
    },
    v_flex,
};

pub fn init(cx: &mut App) {
    // Suggestion keys of the story's own. Each falls through to the key's
    // ordinary binding while there is nothing to act on.
    cx.bind_keys([
        KeyBinding::new("ctrl-space", ShowSuggestions, Some("Input")),
        KeyBinding::new("alt-right", AcceptSuggestionWord, Some("Input")),
    ]);
}

/// Words the suggestion examples complete from.
const VOCABULARY: &[&str] = &[
    "accessibility",
    "application",
    "autocomplete",
    "component",
    "composition",
    "comprehensive",
    "configuration",
    "desktop",
    "document",
    "documentation",
    "framework",
    "interaction",
    "interface",
    "keyboard",
    "suggestion",
    "suggestions",
    "textarea",
    "the quick brown fox jumps over the lazy dog",
];

/// Completes the word before the caret from [`VOCABULARY`].
///
/// An application ranks with its own model, a dictionary or what the user
/// typed before; the textarea shows the answer in the order given.
struct VocabularyProvider;

impl SuggestionProvider for VocabularyProvider {
    fn suggestions(
        &self,
        request: &SuggestionRequest,
        _: &mut Window,
        _: &mut App,
    ) -> Task<anyhow::Result<Vec<Suggestion>>> {
        // Read the caret's line only, not the whole text.
        let text = request.text();
        let offset = request.offset();
        let line_start = text.line_start_offset(text.offset_to_point(offset).row);
        let before = text.slice(line_start..offset).to_string();
        let word = before
            .rsplit(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap_or_default();
        let start = offset - word.len();
        let prefix = word.to_lowercase();
        if prefix.is_empty() {
            return Task::ready(Ok(Vec::new()));
        }
        let suggestions = VOCABULARY
            .iter()
            .filter(|word| word.starts_with(&prefix) && word.len() > prefix.len())
            .take(8)
            .map(|word| {
                Suggestion::new(*word)
                    .with_range(start..offset)
                    .with_detail(if word.contains(' ') { "phrase" } else { "word" })
            })
            .collect();
        Task::ready(Ok(suggestions))
    }
}

struct ComposerAttachment {
    id: u64,
    title: String,
    detail: String,
}

pub struct TextareaStory {
    tokens: Entity<super::input_tokens::TokenExample>,
    textarea: Entity<TextareaState>,
    textarea_auto_grow: Entity<TextareaState>,
    textarea_no_wrap: Entity<TextareaState>,
    textarea_auto_grow_no_wrap: Entity<TextareaState>,
    chat_input: Entity<TextareaState>,
    chat_messages: Vec<String>,
    composer: Entity<TextareaState>,
    suggestions_menu: Entity<TextareaState>,
    suggestions_inline: Entity<TextareaState>,
    last_suggestion_event: Option<SharedString>,
    attachments: Vec<ComposerAttachment>,
    /// Counter for attachment ids; `Image::id` is a content hash, so pasting
    /// the same image twice would collide.
    next_attachment_id: u64,
    size: Size,
    _subscriptions: Vec<Subscription>,
}

impl super::Story for TextareaStory {
    fn title() -> &'static str {
        "Textarea"
    }

    fn description() -> &'static str {
        "Input with multi-line mode."
    }

    fn closable() -> bool {
        false
    }

    fn new_view(window: &mut Window, cx: &mut App) -> Entity<impl Render> {
        Self::view(window, cx)
    }
}

impl TextareaStory {
    pub fn view(window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(window, cx))
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let textarea = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(10)
                .placeholder("Enter text here...")
                .searchable(true)
                .default_value(
                    unindent::unindent(
                        r#"Hello 世界，this is GPUI component.

                    The GPUI Component is a collection of UI components for GPUI framework, including.

                    Button, Input, Checkbox, Radio, Dropdown, Tab, and more...

                    Here is an application that is built by using GPUI Component.

                    > This application is still under development, not published yet.

                    ![image](https://github.com/user-attachments/assets/559a648d-19df-4b5a-b563-b78cc79c8894)

                    ![image](https://github.com/user-attachments/assets/5e06ad5d-7ea0-43db-8d13-86a240da4c8d)

                    ## Demo

                    If you want to see the demo, here is a some demo applications.
                    "#,
                    )
                )
        });

        let textarea_no_wrap = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(6)
                .soft_wrap(false)
                .default_value("This is a very long line of text to test if the horizontal scrolling function is working properly, and it should not wrap automatically but display a horizontal scrollbar.\nThe second line is also very long text, used to test the horizontal scrolling effect under multiple lines, and you can input more content to test.\nThe third line: Here you can input other long text content that requires horizontal scrolling.\n")
        });

        let textarea_auto_grow = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 5)
                .placeholder("Enter text here...")
                .default_value(
                    "Hello 世界 this is a very long line of text \
                    to test if the horizontal scrolling function is working \
                    properly, and it should not wrap automatically but display \
                    a horizontal scrollbar.\n\
                    The second line is also very long text, used to test the \
                    horizontal scrolling effect under multiple lines, and you \
                    can input more content to test.\nThe third line: Here you \
                    can input other long text content that requires \
                    horizontal scrolling.\n",
                )
        });

        let textarea_auto_grow_no_wrap = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 5)
                .soft_wrap(false)
                .placeholder("Enter text here...")
                .default_value("Hello 世界，this is GPUI component.")
        });

        let chat_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 5)
                .submit_on_enter(true)
                .placeholder("Type a message, Enter to send, Shift+Enter for newline")
        });

        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 5)
                .placeholder("Paste a screenshot here, it becomes an attachment above")
        });

        let provider: Rc<dyn SuggestionProvider> = Rc::new(VocabularyProvider);
        let suggestions_menu = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 6)
                .placeholder("Type “com” or “doc”, or press Ctrl-Space")
                .suggestion_provider(provider.clone())
                .suggestion_options(SuggestionOptions::default().inline(true))
        });
        let suggestions_inline = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 6)
                .placeholder("Type “the”: Tab accepts, Alt-Right takes a word")
                .suggestion_provider(provider)
                .suggestion_options(SuggestionOptions::default().menu(false).inline(true))
        });

        let on_suggestion = |this: &mut Self,
                             _: Entity<TextareaState>,
                             event: &SuggestionEvent,
                             cx: &mut Context<Self>| {
            this.last_suggestion_event = Some(match event {
                SuggestionEvent::Accepted {
                    suggestion,
                    partial,
                } => {
                    let how = if *partial {
                        "Accepted a word of"
                    } else {
                        "Accepted"
                    };
                    format!("{how} \u{201c}{}\u{201d}", suggestion.text()).into()
                }
                SuggestionEvent::Dismissed { suggestion } => {
                    format!("Dismissed \u{201c}{}\u{201d}", suggestion.text()).into()
                }
                _ => return,
            });
            cx.notify();
        };

        let _subscriptions = vec![
            cx.subscribe(&suggestions_menu, on_suggestion),
            cx.subscribe(&suggestions_inline, on_suggestion),
            cx.subscribe_in(
                &chat_input,
                window,
                |this: &mut Self, input, event, window, cx| match event {
                    InputEvent::PressEnter { shift, .. } if !shift => {
                        let text = input.read(cx).value().trim().to_string();
                        if !text.is_empty() {
                            this.chat_messages.push(text);
                            input.update(cx, |state, cx| {
                                state.set_value("", window, cx);
                            });
                            cx.notify();
                        }
                    }
                    _ => {}
                },
            ),
        ];

        Self {
            tokens: super::input_tokens::TokenExample::new(true, window, cx),
            textarea,
            textarea_auto_grow,
            textarea_no_wrap,
            textarea_auto_grow_no_wrap,
            chat_input,
            chat_messages: Vec::new(),
            composer,
            suggestions_menu,
            suggestions_inline,
            last_suggestion_event: None,
            attachments: Vec::new(),
            next_attachment_id: 0,
            size: Size::Medium,
            _subscriptions,
        }
    }

    fn on_insert_text_to_textarea(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.textarea.update(cx, |input, cx| {
            input.insert("Hello 你好", window, cx);
        });
    }

    fn on_replace_text_to_textarea(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.textarea.update(cx, |input, cx| {
            input.replace("Hello 你好", window, cx);
        });
    }
}

impl Focusable for TextareaStory {
    fn focus_handle(&self, cx: &gpui_kit::App) -> gpui_kit::FocusHandle {
        self.textarea.focus_handle(cx)
    }
}

impl Render for TextareaStory {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let loc = self.textarea.read(cx).cursor_position();

        v_flex()
            .w_full()
            .gap_3()
            .on_action(cx.listener(|this, action: &ChangeStorySize, _, cx| {
                this.size = action.0;
                cx.notify();
            }))
            .child(story_toolbar(self.size))
            .child(
                section("Textarea").w(px(560.)).child(
                    v_flex()
                        .gap_2()
                        .w_full()
                        .child(Textarea::new(&self.textarea).with_size(self.size).h(px(320.)))
                        .child(
                            h_flex()
                                .justify_between()
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            Button::new("btn-insert-text")
                                                .outline()
                                                .xsmall()
                                                .label("Insert Text")
                                                .on_click(
                                                    cx.listener(Self::on_insert_text_to_textarea),
                                                ),
                                        )
                                        .child(
                                            Button::new("btn-replace-text")
                                                .outline()
                                                .xsmall()
                                                .label("Replace Text")
                                                .on_click(
                                                    cx.listener(Self::on_replace_text_to_textarea),
                                                ),
                                        ),
                                )
                                .child(format!("{}:{}", loc.line, loc.character)),
                        ),
                ),
            )
            .child(
                section("No Wrap")
                    .w(px(560.))
                    .child(Textarea::new(&self.textarea_no_wrap).with_size(self.size).h(px(200.))),
            )
            .child(
                section("Auto Grow")
                    .w(px(560.))
                    .child(Textarea::new(&self.textarea_auto_grow).with_size(self.size)),
            )
            .child(
                section("Auto Grow with No Wrap")
                    .w(px(560.))
                    .child(Textarea::new(&self.textarea_auto_grow_no_wrap).with_size(self.size)),
            )
            .child(
                section("Suggestions")
                    .description(
                        "Suggestions from the application's provider, in a menu with a preview, \
                        or inline only. Up and Down choose, Enter or Tab accept, Escape dismisses.",
                    )
                    .w(px(560.))
                    .child(
                        v_flex()
                            .gap_2()
                            .w_full()
                            .child(Textarea::new(&self.suggestions_menu).with_size(self.size))
                            .child(Textarea::new(&self.suggestions_inline).with_size(self.size))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        self.last_suggestion_event
                                            .clone()
                                            .unwrap_or_else(|| "No suggestion accepted yet".into()),
                                    ),
                            ),
                    ),
            )
            .child(
                section("Submit on Enter (Chat)").w(px(560.)).child(
                    v_flex()
                        .gap_2()
                        .w_full()
                        .child(v_flex().gap_1().children(
                            self.chat_messages.iter().enumerate().map(|(i, msg)| {
                                div()
                                    .id(("chat-msg", i))
                                    .px_2()
                                    .py_1()
                                    .rounded(cx.theme().radius)
                                    .bg(cx.theme().muted)
                                    .child(msg.clone())
                            }),
                        ))
                        .child(Textarea::new(&self.chat_input).with_size(self.size)),
                ),
            )
            .child(
                section("Paste Images (Composer)")
                    .description("Paste a screenshot, it lands as an attachment pill above.")
                    .w(px(560.))
                    .child(
                        v_flex()
                            .gap_2()
                            .w_full()
                            .when(!self.attachments.is_empty(), |this| {
                                this.child(AttachmentGroup::new("composer-attachments").children(
                                    self.attachments.iter().map(|attachment| {
                                        let id = attachment.id;
                                        HoverCard::new(("pasted-preview", id))
                                            .trigger(
                                                Attachment::new()
                                                    .media(AttachmentMedia::new().child(
                                                        Icon::new(IconName::FileText).small(),
                                                    ))
                                                    .content(
                                                        AttachmentContent::new()
                                                            .title(AttachmentTitle::new(
                                                                attachment.title.clone(),
                                                            ))
                                                            .description(
                                                                AttachmentDescription::new(
                                                                    attachment.detail.clone(),
                                                                ),
                                                            ),
                                                    )
                                                    .actions(
                                                        AttachmentActions::new().child(
                                                            Button::new(("remove-pasted", id))
                                                                .ghost()
                                                                .xsmall()
                                                                .icon(IconName::Close)
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        this.attachments.retain(
                                                                            |item| item.id != id,
                                                                        );
                                                                        cx.notify();
                                                                    },
                                                                )),
                                                        ),
                                                    ),
                                            )
                                            .child(div().text_sm().child(attachment.detail.clone()))
                                            .into_any_element()
                                    }),
                                ))
                            })
                            .child({
                                let view = cx.entity().downgrade();
                                Textarea::new(&self.composer)
                                    .with_size(self.size)
                                    .on_paste(move |item, _, cx| {
                                    let images: Vec<_> = item
                                        .entries()
                                        .iter()
                                        .filter_map(|entry| match entry {
                                            ClipboardEntry::Image(image) => Some(image.clone()),
                                            _ => None,
                                        })
                                        .collect();
                                    if images.is_empty() {
                                        return false;
                                    }
                                    view.update(cx, |this: &mut Self, cx| {
                                        for image in images {
                                            let id = this.next_attachment_id;
                                            this.next_attachment_id += 1;
                                            this.attachments.push(ComposerAttachment {
                                                id,
                                                title: format!("pasted-image-{id}.png"),
                                                detail: format!(
                                                    "{:?} - {} bytes",
                                                    image.format,
                                                    image.bytes.len()
                                                ),
                                            });
                                        }
                                        cx.notify();
                                    })
                                    .ok();
                                    true
                                })
                            }),
                    ),
            )
            .child(section("Atomic inline tokens")
                .description("References keep their identity through selection, deletion and undo. Copy returns the underlying text.")
                .w_full().child(self.tokens.clone()))
    }
}
