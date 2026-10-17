use super::*;
use gpui_kit::component::setting::{
    SelectIndex, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};

struct SettingsPreview {
    instructions: Entity<InputState>,
}

impl Render for SettingsPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let instructions = self.instructions.clone();
        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                Settings::new("settings-preview")
                    .sidebar_width(px(224.))
                    .default_selected_index(SelectIndex {
                        page_ix: 1,
                        group_ix: None,
                    })
                    .page(SettingPage::new("General").group(SettingGroup::new().item(
                        SettingItem::new("Language", SettingField::render(|_, _, _| "English")),
                    )))
                    .page(
                        SettingPage::new("Editing")
                            .icon(IconName::Settings2)
                            .default_open(true)
                            .description(
                                "Choose how new documents behave. Changes apply immediately.",
                            )
                            .resettable(false)
                            .group(
                                SettingGroup::new()
                                    .key(0)
                                    .title("Writing")
                                    .item(
                                        SettingItem::new(
                                            "Instructions",
                                            SettingField::render(move |options, _, _| {
                                                Input::new(&instructions).w_64().when(
                                                    options.layout() == gpui_kit::Axis::Vertical,
                                                    |this| this.w_full(),
                                                )
                                            }),
                                        )
                                        .description(
                                            "Add instructions to use when starting a document.",
                                        ),
                                    )
                                    .item(
                                        SettingItem::new(
                                            "Spell check",
                                            SettingField::switch(|_| true, |_, _| {}),
                                        )
                                        .description("Underline misspelled words while you type."),
                                    )
                                    .item(
                                        SettingItem::new(
                                            "Suggestions",
                                            SettingField::switch(|_| false, |_, _| {}),
                                        )
                                        .description("Suggest words after a short pause."),
                                    ),
                            )
                            .group(
                                SettingGroup::new()
                                    .key(0)
                                    .title("Display")
                                    .item(SettingItem::new(
                                        "Line numbers",
                                        SettingField::switch(|_| true, |_, _| {}),
                                    ))
                                    .item(
                                        SettingItem::new(
                                            "Wrap lines",
                                            SettingField::switch(|_| true, |_, _| {}),
                                        )
                                        .description("Keep long lines within the editor's width."),
                                    ),
                            ),
                    )
                    .page(SettingPage::new("Updates").group(SettingGroup::new().item(
                        SettingItem::new(
                            "Automatic updates",
                            SettingField::switch(|_| true, |_, _| {}),
                        ),
                    ))),
            )
    }
}

/// Rows whose fields are too wide to sit beside their titles.
struct LongChoicesPreview;

impl Render for LongChoicesPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let choice = |label: &'static str| {
            SettingField::dropdown(
                vec![(label.into(), label.into())],
                move |_| label.into(),
                |_, _| {},
            )
        };
        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                Settings::new("long-choices").sidebar_width(px(224.)).page(
                    SettingPage::new("Composer").resettable(false).group(
                        SettingGroup::new()
                            .title("Sending")
                            .item(
                                SettingItem::new(
                                    "Send key",
                                    choice("Enter sends, Shift-Enter starts a new line"),
                                )
                                .description("Choose the key that sends a message."),
                            )
                            .item(
                                SettingItem::new(
                                    "Model",
                                    choice(
                                        "Use the model chosen for the main agent, or the \
                                         provider's default model when it has none",
                                    ),
                                )
                                .description("Choose the model for new conversations."),
                            )
                            .item(
                                SettingItem::new(
                                    "Signature",
                                    SettingField::input(|_| "Sent from my desk".into(), |_, _| {}),
                                )
                                .description("Add this line to the end of every message."),
                            )
                            .item(
                                SettingItem::new(
                                    "Spell check",
                                    SettingField::switch(|_| true, |_, _| {}),
                                )
                                .description("Underline misspelled words while you type."),
                            ),
                    ),
                ),
            )
    }
}

fn long_choices(mode: ThemeMode) -> Screenshot {
    let mut app = app(mode);
    // The page beside the sidebar is a little wider than the stacking threshold.
    let window = open(&mut app, (760., 560.), SCALE, |_, _| LongChoicesPreview);
    app.capture(window).unwrap()
}

#[test]
fn settings_long_choices_light() {
    goldens().assert(
        "settings-long-choices-light",
        &long_choices(ThemeMode::Light),
    );
}

#[test]
fn settings_long_choices_dark() {
    goldens().assert("settings-long-choices-dark", &long_choices(ThemeMode::Dark));
}

fn settings(mode: ThemeMode, width: f32) -> Screenshot {
    let mut app = app(mode);
    let window = open(&mut app, (width, 720.), SCALE, |window, cx| {
        SettingsPreview {
            instructions: cx
                .new(|cx| InputState::new(window, cx).default_value("Use plain language.")),
        }
    });
    app.capture(window).unwrap()
}

#[test]
fn settings_narrow_light() {
    goldens().assert("settings-narrow-light", &settings(ThemeMode::Light, 460.));
}

#[test]
fn settings_narrow_dark() {
    goldens().assert("settings-narrow-dark", &settings(ThemeMode::Dark, 460.));
}

#[test]
fn settings_wide_light() {
    goldens().assert("settings-wide-light", &settings(ThemeMode::Light, 1000.));
}

#[test]
fn settings_wide_dark() {
    goldens().assert("settings-wide-dark", &settings(ThemeMode::Dark, 1000.));
}
