//! Golden screenshots of GPUI Kit components, rendered by the gpui fork's mhr-gpui-screenshot
//! on Mesa's software Vulkan driver. The goldens live in `tests/screenshots`. On a mismatch the
//! actual image and a diff go to `tests/screenshots/failures`, and `UPDATE_GOLDENS=1` rewrites
//! the goldens. The gpui fork's `docs/screenshots.md` explains how to review and update them.
#![cfg(target_os = "linux")]

use std::{borrow::Cow, cell::RefCell, rc::Rc, time::Duration};

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Focusable as _, FontWeight,
    InputEvent as _, InteractiveElement as _, IntoElement, Modifiers, MouseButton, MouseDownEvent,
    MouseUpEvent, ParentElement as _, Render, Result, SharedString, Styled as _, Task, Window,
    assets::Assets,
    component::{
        ActiveTheme as _, Disableable as _, IconName, IndexPath, Sizable as _, Theme, ThemeMode,
        button::{Button, ButtonVariants as _},
        clipboard::Clipboard,
        font_picker::{FontCatalog, FontPicker, FontPickerState, FontSettings},
        input::{
            Editor, EditorState, Input, InputState, RopeExt as _, SpellCheck, SpellCheckRequest,
            SpellChecker, Suggestion, SuggestionProvider, SuggestionRequest, TextEditor, Textarea,
            TextareaState,
        },
        list::{List, ListDelegate, ListItem, ListState},
        menu::{PopupMenu, PopupMenuItem},
        progress::{Progress, ProgressCircle, ProgressSegment, UsageBar},
    },
    div,
    prelude::FluentBuilder as _,
    px, size,
    test::TestWindowExt as _,
};
use gpui_screenshot::{Goldens, Screenshot, ScreenshotApp, Tolerance};

#[path = "screenshots/concentric_radii.rs"]
mod concentric_radii;
#[path = "screenshots/segmented_tabs.rs"]
mod segmented_tabs;
#[path = "screenshots/settings.rs"]
mod settings;
#[path = "screenshots/table_spacing.rs"]
mod table_spacing;
#[path = "screenshots/text_field_focus.rs"]
mod text_field_focus;

/// A typical phone's scale factor. Each golden is this many device pixels per logical pixel.
const SCALE: f32 = 2.;

/// The goldens in `tests/screenshots`. Mesa and LLVM releases round antialiased edges a level or
/// two apart, so each channel of each pixel may differ from its golden by up to 2.
fn goldens() -> Goldens {
    gpui_screenshot::goldens!().tolerance(Tolerance::channel(2))
}

/// A headless app with GPUI Kit initialized in `mode`.
fn app(mode: ThemeMode) -> ScreenshotApp {
    let mut app = ScreenshotApp::with_assets(Assets).unwrap();
    app.update(|cx| {
        gpui_kit::init(cx);
        Theme::change(mode, None, cx);
    });
    app
}

/// Opens `build`'s view the way an application does, inside GPUI Kit's `Root`.
fn open<V: Render + 'static>(
    app: &mut ScreenshotApp,
    (width, height): (f32, f32),
    scale: f32,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> AnyWindowHandle {
    app.open_window_with(size(px(width), px(height)), scale, |options, cx| {
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| build(window, cx)))
            .map(|(window, _)| window)
    })
    .unwrap()
}

/// Runs `act` on the window, for the interactions in `gpui_kit::test::TestWindowExt`.
fn act(app: &mut ScreenshotApp, window: AnyWindowHandle, act: impl FnOnce(&mut Window, &mut App)) {
    app.update_window(window, |_, window, cx| act(window, cx))
        .unwrap();
}

fn page(cx: &App) -> gpui_kit::Div {
    div()
        .size_full()
        .bg(cx.theme().background)
        .text_color(cx.theme().foreground)
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
}

struct TokenTooltip(Entity<InputState>);

impl Render for TokenTooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(Input::new(&self.0).token(|context, _, _| {
            div().id("reference").child(
                gpui_kit::component::input::InputToken::new(context)
                    .icon(IconName::File)
                    .tooltip("Project notes in docs/notes.md"),
            )
        }))
    }
}

#[test]
fn input_token_tooltip() {
    for (mode, name) in [
        (ThemeMode::Light, "input-token-tooltip"),
        (ThemeMode::Dark, "input-token-tooltip-dark"),
    ] {
        let mut app = app(mode);
        app.update(|cx| cx.set_reduce_motion(true));
        let window = open(&mut app, (360., 150.), SCALE, |window, cx| {
            TokenTooltip(cx.new(|cx| {
                let mut input = InputState::new(window, cx);
                input
                    .replace_with_token(
                        gpui_kit::component::input::InlineToken::new("notes", "notes.md"),
                        window,
                        cx,
                    )
                    .unwrap();
                input
            }))
        });
        act(&mut app, window, |window, cx| {
            window.dispatch_event(
                gpui_kit::MouseMoveEvent {
                    position: gpui_kit::point(px(60.), px(30.)),
                    pressed_button: None,
                    modifiers: Modifiers::default(),
                }
                .to_platform_input(),
                cx,
            );
        });
        app.advance_clock(Duration::from_secs(2));
        goldens().assert(name, &app.capture(window).unwrap());
    }
}

/// Two icon buttons side by side, as under a message.
struct IconButtons;

impl Render for IconButtons {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).pl(px(120.)).child(
            gpui_kit::component::h_flex()
                .child(
                    Button::new("edit")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Undo)
                        .tooltip("Edit"),
                )
                .child(
                    Button::new("branch")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Redo)
                        .tooltip("Branch"),
                ),
        )
    }
}

/// Moves the pointer onto Edit (`x` 130) or Branch (`x` 150) of [`IconButtons`].
fn hover_icon_button(app: &mut ScreenshotApp, window: AnyWindowHandle, x: f32) {
    act(app, window, |window, cx| {
        window.dispatch_event(
            gpui_kit::MouseMoveEvent {
                position: gpui_kit::point(px(x), px(26.)),
                pressed_button: None,
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
            cx,
        );
    });
    // Starts the show timer before the clock moves.
    app.run_until_parked();
}

/// The tooltip handing over from Edit to Branch 12, 56 and 80 ms into the switch. The box moves
/// and widens on whole pixels and uncovers Branch, which shows whole where it stays from the
/// switch's second frame.
#[test]
fn tooltip_switch() {
    let mut app = app(ThemeMode::Light);
    app.update(|cx| cx.set_reduce_motion(false));
    let window = open(&mut app, (300., 90.), SCALE, |_, _| IconButtons);
    hover_icon_button(&mut app, window, 130.);
    app.advance_clock(Duration::from_secs(2));
    app.capture(window).unwrap();
    // Edit's entrance animation runs on the wall clock, and the switch starts where it ends.
    std::thread::sleep(Duration::from_millis(300));
    app.capture(window).unwrap();
    hover_icon_button(&mut app, window, 150.);
    // The pointer rests on Branch for the switch delay.
    app.advance_clock(Duration::from_millis(100));
    app.capture(window).unwrap();
    let mut elapsed = 0;
    for ms in [12, 56, 80] {
        app.advance_clock(Duration::from_millis(ms - elapsed));
        elapsed = ms;
        goldens().assert(
            &format!("tooltip-switch-{ms}ms"),
            &app.capture(window).unwrap(),
        );
    }
}

/// With reduced motion, the first frame of the switch is already its last.
#[test]
fn tooltip_switch_with_reduced_motion() {
    let mut app = app(ThemeMode::Light);
    let window = open(&mut app, (300., 90.), SCALE, |_, _| IconButtons);
    hover_icon_button(&mut app, window, 130.);
    app.advance_clock(Duration::from_secs(2));
    app.capture(window).unwrap();
    hover_icon_button(&mut app, window, 150.);
    // The pointer rests on Branch for the switch delay.
    app.advance_clock(Duration::from_millis(100));
    goldens().assert("tooltip-switch-80ms", &app.capture(window).unwrap());
}

struct SelectableHighlights;

impl Render for SelectableHighlights {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(
            gpui_kit::base::SelectableText::new(
                "highlighted-text",
                "Find matches. The active match stays selectable.",
            )
            .highlights([
                (
                    5..12,
                    gpui_kit::HighlightStyle {
                        background_color: Some(cx.theme().search_match),
                        ..Default::default()
                    },
                ),
                (
                    25..30,
                    gpui_kit::HighlightStyle {
                        background_color: Some(cx.theme().search_match_active),
                        font_weight: Some(FontWeight::BOLD),
                        ..Default::default()
                    },
                ),
            ]),
        )
    }
}

#[test]
fn selectable_highlights() {
    for (mode, name) in [
        (ThemeMode::Light, "selectable-highlights"),
        (ThemeMode::Dark, "selectable-highlights-dark"),
    ] {
        let mut app = app(mode);
        let window = open(&mut app, (320., 100.), SCALE, |_, _| SelectableHighlights);
        goldens().assert(name, &app.capture(window).unwrap());
    }
}

/// Markdown with a fenced code block, which sits on the theme's editor surface with a hairline
/// edge, so it shows where that surface matches the page.
struct MarkdownCode;

impl Render for MarkdownCode {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(gpui_kit::component::text::TextView::markdown(
            "markdown-code",
            "Run `cargo test` and read this.\n\n```\nfn main() {\n    run();\n}\n```\n\nThen it passes.",
        ))
    }
}

#[test]
fn markdown_code_block() {
    for (mode, name) in [
        (ThemeMode::Light, "markdown-code-block"),
        (ThemeMode::Dark, "markdown-code-block-dark"),
    ] {
        let mut app = app(mode);
        let window = open(&mut app, (320., 190.), SCALE, |_, _| MarkdownCode);
        goldens().assert(name, &app.capture(window).unwrap());
    }
}

struct Buttons;

impl Render for Buttons {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row = || div().flex().items_center().gap_2();
        page(cx)
            .child(
                row()
                    .child(Button::new("primary").primary().label("Save"))
                    .child(Button::new("secondary").label("Cancel"))
                    .child(Button::new("outline").outline().label("Outline"))
                    .child(Button::new("danger").danger().label("Delete")),
            )
            .child(
                row()
                    .child(
                        Button::new("ghost")
                            .ghost()
                            .icon(IconName::Copy)
                            .label("Copy"),
                    )
                    .child(Button::new("link").link().label("Learn more"))
                    .child(
                        Button::new("disabled")
                            .primary()
                            .label("Disabled")
                            .disabled(true),
                    )
                    .child(
                        Button::new("loading")
                            .outline()
                            .icon(IconName::Inbox)
                            .label("Saving")
                            .loading(true),
                    ),
            )
    }
}

fn buttons(mode: ThemeMode, scale: f32) -> Screenshot {
    let mut app = app(mode);
    let window = open(&mut app, (440., 108.), scale, |_, _| Buttons);
    app.capture(window).unwrap()
}

struct Fields {
    name: Entity<InputState>,
    email: Entity<InputState>,
    notes: Entity<TextareaState>,
}

impl Render for Fields {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx)
            .child(Input::new(&self.name))
            .child(Input::new(&self.email))
            .child(Textarea::new(&self.notes))
    }
}

/// A focused input with typed text and its caret, an empty one showing its placeholder, and a
/// textarea whose text wraps.
fn fields() -> Screenshot {
    let mut app = app(ThemeMode::Light);
    let window = open(&mut app, (320., 232.), SCALE, |window, cx| {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let email = cx.new(|cx| InputState::new(window, cx).placeholder("Email"));
        let notes = cx.new(|cx| {
            TextareaState::new(window, cx).rows(3).default_value(
                "Notes wrap across lines when they run past the edge of the field.\n\
                 A second paragraph.",
            )
        });
        name.update(cx, |name, cx| name.focus(window, cx));
        Fields { name, email, notes }
    });
    act(&mut app, window, |window, cx| {
        window.input("Ada Lovelace", cx)
    });
    app.capture(window).unwrap()
}

const ROWS: [&str; 5] = ["Inbox", "Drafts", "Archive", "Notifications", "Calendar"];

fn row_icon(row: usize) -> IconName {
    match row {
        0 => IconName::Inbox,
        1 => IconName::File,
        2 => IconName::Folder,
        3 => IconName::Bell,
        _ => IconName::Calendar,
    }
}

struct Rows;

impl ListDelegate for Rows {
    type Item = ListItem;

    fn items_count(&self, _: usize, _: &App) -> usize {
        ROWS.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        Some(
            ListItem::new(("row", ix.row)).child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(row_icon(ix.row))
                    .child(ROWS[ix.row]),
            ),
        )
    }

    fn set_selected_index(
        &mut self,
        _: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
    }
}

struct Listed {
    list: Entity<ListState<Rows>>,
}

impl Render for Listed {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(div().w(px(220.)).h(px(180.)).child(List::new(&self.list)))
    }
}

/// A focused list with the keyboard cursor on its second row.
fn list() -> Screenshot {
    let mut app = app(ThemeMode::Light);
    let window = open(&mut app, (260., 220.), SCALE, |window, cx| {
        let list = cx.new(|cx| ListState::new(Rows, window, cx));
        list.read(cx).focus_handle(cx).focus(window, cx);
        Listed { list }
    });
    act(&mut app, window, |window, cx| {
        window.press("down", cx);
        window.press("down", cx);
    });
    app.capture(window).unwrap()
}

struct Menu {
    menu: Entity<PopupMenu>,
}

impl Render for Menu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(div().w(px(220.)).child(self.menu.clone()))
    }
}

/// A popup menu with a label, icons, a checked item, a separator and a disabled item, the
/// keyboard cursor moved onto `Copy`.
fn menu(mode: ThemeMode) -> Screenshot {
    let mut app = app(mode);
    let window = open(&mut app, (260., 236.), SCALE, |window, cx| {
        let menu = PopupMenu::build(window, cx, |menu, _, _| {
            menu.label("Edit")
                .item(
                    PopupMenuItem::new("Copy")
                        .icon(IconName::Copy)
                        .on_click(|_, _, _| {}),
                )
                .item(PopupMenuItem::new("Paste").on_click(|_, _, _| {}))
                .item(
                    PopupMenuItem::new("Show hidden files")
                        .checked(true)
                        .on_click(|_, _, _| {}),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Delete")
                        .icon(IconName::Delete)
                        .disabled(true),
                )
        });
        menu.read(cx).focus_handle(cx).focus(window, cx);
        Menu { menu }
    });
    act(&mut app, window, |window, cx| {
        window.press("down", cx);
        window.press("down", cx);
    });
    app.capture(window).unwrap()
}

gpui_kit::actions!(screenshots, [Undo, Redo, Save, SaveAll, Settings]);

/// A menu of actions with their shortcuts, each drawn as a key cap: a single key, several
/// modifiers, and a sequence of two keystrokes, the keyboard cursor on the second item.
fn shortcut_menu(mode: ThemeMode) -> Screenshot {
    let mut app = app(mode);
    app.update(|cx| {
        cx.bind_keys([
            gpui_kit::KeyBinding::new("ctrl-z", Undo, None),
            gpui_kit::KeyBinding::new("ctrl-shift-z", Redo, None),
            gpui_kit::KeyBinding::new("ctrl-s", Save, None),
            gpui_kit::KeyBinding::new("ctrl-k s", SaveAll, None),
            gpui_kit::KeyBinding::new("f2", Settings, None),
        ])
    });
    let window = open(&mut app, (256., 196.), SCALE, |window, cx| {
        let menu = PopupMenu::build(window, cx, |menu, _, _| {
            menu.menu("Undo", Box::new(Undo))
                .menu("Redo", Box::new(Redo))
                .separator()
                .menu("Save", Box::new(Save))
                .menu("Save All", Box::new(SaveAll))
                .separator()
                .menu("Settings", Box::new(Settings))
        });
        menu.read(cx).focus_handle(cx).focus(window, cx);
        Menu { menu }
    });
    act(&mut app, window, |window, cx| {
        window.press("down", cx);
        window.press("down", cx);
    });
    app.capture(window).unwrap()
}

/// The words [`Words`] completes from, each with the detail its menu row shows.
const WORDS: [(&str, &str); 6] = [
    ("comment", "noun"),
    ("commit", "verb"),
    ("component", "noun"),
    ("compose", "verb"),
    ("draft", "noun"),
    ("review", "verb"),
];

/// Completes the word before the caret from [`WORDS`].
struct Words;

impl SuggestionProvider for Words {
    fn suggestions(
        &self,
        request: &SuggestionRequest,
        _: &mut Window,
        _: &mut App,
    ) -> Task<Result<Vec<Suggestion>>> {
        let (text, offset) = (request.text(), request.offset());
        let line_start = text.line_start_offset(text.offset_to_point(offset).row);
        let before = text.slice(line_start..offset).to_string();
        let typed = before
            .rsplit(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap_or_default();
        let start = offset - typed.len();
        let suggestions = WORDS
            .iter()
            .filter(|(word, _)| !typed.is_empty() && word.starts_with(typed))
            .map(|(word, detail)| {
                Suggestion::new(*word)
                    .with_range(start..offset)
                    .with_detail(*detail)
            })
            .collect();
        Task::ready(Ok(suggestions))
    }
}

struct Note {
    note: Entity<TextareaState>,
}

impl Render for Note {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(Textarea::new(&self.note))
    }
}

/// A textarea completing the word typed into it: the suggestion menu open under the word, with
/// what was typed highlighted in each row and the keyboard cursor moved to the second row.
fn suggestion_menu(mode: ThemeMode) -> Screenshot {
    let mut app = app(mode);
    // Room for the whole menu under the first line. In a shorter window the menu moves up to
    // stay inside it and covers the line.
    let window = open(&mut app, (320., 200.), SCALE, |window, cx| {
        let note = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(3)
                .suggestion_provider(Rc::new(Words))
        });
        note.update(cx, |note, cx| note.focus(window, cx));
        Note { note }
    });
    act(&mut app, window, |window, cx| {
        window.input("Ship the com", cx);
        window.press("down", cx);
    });
    app.capture(window).unwrap()
}

/// The misspellings [`Corrections`] knows, with their replacements.
const CORRECTIONS: [(&str, &[&str]); 2] = [
    ("teh", &["the", "tea", "ten"]),
    ("recieve", &["receive", "relieve", "deceive"]),
];

/// Marks the words in [`CORRECTIONS`] and suggests their replacements.
struct Corrections;

impl SpellChecker for Corrections {
    fn check(&self, request: &SpellCheckRequest, _: &mut App) -> Task<Result<SpellCheck>> {
        let mut misspelled = Vec::new();
        for range in request.ranges() {
            let text = request.text().slice(range.clone()).to_string();
            let mut offset = range.start;
            for word in text.split(|c: char| !c.is_alphabetic()) {
                if CORRECTIONS.iter().any(|(wrong, _)| *wrong == word) {
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
        CORRECTIONS
            .iter()
            .find(|(wrong, _)| *wrong == word)
            .map(|(_, right)| right.iter().map(|word| SharedString::from(*word)).collect())
            .unwrap_or_default()
    }

    fn add_to_dictionary(&self, _: &str, _: &mut App) {}
}

struct Document {
    document: Entity<TextareaState>,
}

impl Render for Document {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(div().h(px(150.)).child(TextEditor::new(&self.document)))
    }
}

const DOCUMENT: &str = "The quick brown fox jumps over teh lazy dog.\n\
    A text editor numbers its lines and wraps the long ones at the edge, so this line takes \
    two rows.\n\
    Words it doesn't know, like recieve, get a wavy underline.";

/// A text editor in `mode`: its gutter with line numbers, the caret's line highlighted, a
/// wrapped line, and two misspellings underlined.
fn text_editor_window(
    mode: ThemeMode,
    size: (f32, f32),
) -> (ScreenshotApp, AnyWindowHandle, Entity<TextareaState>) {
    let mut app = app(mode);
    let opened = Rc::new(std::cell::RefCell::new(None));
    let window = open(&mut app, size, SCALE, {
        let opened = opened.clone();
        move |window, cx| {
            let document = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .text_editor()
                    .spell_checker(Rc::new(Corrections))
                    .default_value(DOCUMENT)
            });
            document.update(cx, |document, cx| document.focus(window, cx));
            *opened.borrow_mut() = Some(document.clone());
            Document { document }
        }
    });
    let document = opened.borrow_mut().take().unwrap();
    (app, window, document)
}

fn text_editor(mode: ThemeMode) -> Screenshot {
    let (mut app, window, _) = text_editor_window(mode, (380., 182.));
    app.capture(window).unwrap()
}

/// The text editor's context menu, right-clicked just inside the end of "recieve": the
/// spelling suggestions, Add to Dictionary and Ignore above the standard items.
fn text_editor_menu(mode: ThemeMode) -> Screenshot {
    // Wide enough for the menu to open beside the word rather than over it.
    let (mut app, window, document) = text_editor_window(mode, (500., 400.));
    app.capture(window).unwrap();
    act(&mut app, window, |window, cx| {
        let last_letter = DOCUMENT.find("recieve").unwrap() + "recieve".len() - 1;
        let letter = document
            .read(cx)
            .range_to_bounds(&(last_letter..last_letter + 1))
            .unwrap();
        let position = gpui_kit::point(letter.right() - px(1.), letter.center().y);
        let modifiers = Modifiers::default();
        window.dispatch_event(
            MouseDownEvent {
                position,
                button: MouseButton::Right,
                modifiers,
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            MouseUpEvent {
                position,
                button: MouseButton::Right,
                modifiers,
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
    });
    app.capture(window).unwrap()
}

struct Surfaces {
    document: Entity<TextareaState>,
    note: Entity<TextareaState>,
}

impl Render for Surfaces {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx)
            .child(div().h(px(80.)).child(TextEditor::new(&self.document)))
            .child(Textarea::new(&self.note).h(px(80.)))
    }
}

/// The color a device pixel shows, as `[r, g, b]`.
fn pixel(screenshot: &Screenshot, (x, y): (f32, f32)) -> [u8; 3] {
    let [r, g, b, _] = screenshot
        .image
        .get_pixel((x * SCALE) as u32, (y * SCALE) as u32)
        .0;
    [r, g, b]
}

/// `color` as the 8-bit `[r, g, b]` a window shows it in.
fn rgb(color: gpui_kit::Hsla) -> [u8; 3] {
    let color = gpui_kit::Rgba::from(color);
    [color.r, color.g, color.b].map(|channel| (channel * 255.).round() as u8)
}

/// Every bundled theme, light and dark, draws a text editor and its line-number gutter on the
/// theme's `editor.background`, as a code editor, and a plain textarea on the input background.
#[test]
fn text_editor_draws_on_each_themes_editor_background() {
    let mut app = app(ThemeMode::Light);
    let themes_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../themes");
    app.update(|cx| {
        for entry in std::fs::read_dir(themes_dir).unwrap() {
            let content = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            gpui_kit::component::ThemeRegistry::global_mut(cx)
                .load_themes_from_str(&content)
                .unwrap();
        }
    });
    let window = open(&mut app, (240., 200.), SCALE, |window, cx| Surfaces {
        document: cx.new(|cx| {
            TextareaState::new(window, cx)
                .text_editor()
                .default_value("Hi")
        }),
        note: cx.new(|cx| TextareaState::new(window, cx).default_value("Hi")),
    });
    let themes = app.update(|cx| {
        gpui_kit::component::ThemeRegistry::global(cx)
            .sorted_themes()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    });
    assert!(themes.len() > 60, "found {} themes", themes.len());
    let modes = themes.iter().map(|theme| theme.mode).collect::<Vec<_>>();
    assert!(modes.contains(&ThemeMode::Light) && modes.contains(&ThemeMode::Dark));

    for theme in themes {
        let editor_background = theme
            .highlight
            .as_ref()
            .and_then(|highlight| highlight.editor_background)
            .unwrap_or_else(|| panic!("{} sets no editor.background", theme.name));
        let input_background = app.update(|cx| {
            Theme::update(cx, |current| current.apply_config(&theme));
            let current = cx.theme();
            current.background.blend(current.input_background())
        });
        let shot = app.capture(window).unwrap();
        let close = |actual: [u8; 3], expected: [u8; 3]| {
            actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2)
        };
        // Below the one line: the text area right of the gutter, then the gutter itself.
        for (place, at) in [("text", (200., 80.)), ("gutter", (22., 80.))] {
            let actual = pixel(&shot, at);
            assert!(
                close(actual, rgb(editor_background)),
                "{}: the text editor's {place} shows {actual:?}, editor.background is {:?}",
                theme.name,
                rgb(editor_background),
            );
        }
        let actual = pixel(&shot, (200., 170.));
        assert!(
            close(actual, rgb(input_background)),
            "{}: the textarea shows {actual:?}, the input background is {:?}",
            theme.name,
            rgb(input_background),
        );
    }
}

/// A text editor alone, for the smooth caret.
struct Typing {
    document: Entity<TextareaState>,
}

impl Render for Typing {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(div().h(px(40.)).child(TextEditor::new(&self.document)))
    }
}

/// A text editor holding `text` with the caret at `caret`, with the smooth caret on unless
/// `smooth` is false. The screenshot app shows animations in their final state, so this one
/// turns motion back on.
fn smooth_caret_window(
    mut app: ScreenshotApp,
    text: &'static str,
    caret: usize,
    smooth: bool,
) -> (ScreenshotApp, AnyWindowHandle, Entity<TextareaState>) {
    app.update(|cx| cx.set_reduce_motion(false));
    let opened = Rc::new(RefCell::new(None));
    let window = open(&mut app, (300., 72.), SCALE, {
        let opened = opened.clone();
        move |window, cx| {
            let document = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .text_editor()
                    .smooth_caret(smooth)
                    .default_value(text)
            });
            document.update(cx, |document, cx| {
                document.focus(window, cx);
                document.set_selected_range(caret..caret, cx);
            });
            *opened.borrow_mut() = Some(document.clone());
            Typing { document }
        }
    });
    let document = opened.borrow_mut().take().unwrap();
    app.capture(window).unwrap();
    (app, window, document)
}

/// The smooth caret 0, 25, 50 and 100 ms after a "W" is typed in the middle of a line. The
/// caret uncovers the letter as it glides, the rest of the line moves with it, and after 100 ms
/// the line is drawn as without the smooth caret.
fn smooth_caret(mode: ThemeMode) -> Vec<(u64, Screenshot)> {
    let (mut app, window, _) = smooth_caret_window(app(mode), "Hello world", 5, true);
    act(&mut app, window, |window, cx| window.input("W", cx));
    let mut elapsed = 0;
    [0, 25, 50, 100]
        .into_iter()
        .map(|ms| {
            app.advance_clock(Duration::from_millis(ms - elapsed));
            elapsed = ms;
            (ms, app.capture(window).unwrap())
        })
        .collect()
}

/// A single-line input alone, for the smooth caret.
struct Field {
    input: Entity<InputState>,
}

impl Render for Field {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(Input::new(&self.input))
    }
}

/// A single-line input holding `text` with the caret at `caret`, with the smooth caret on unless
/// `smooth` is false, and motion back on, as in [`smooth_caret_window`].
fn input_smooth_caret_window(
    mut app: ScreenshotApp,
    text: &'static str,
    caret: usize,
    smooth: bool,
) -> (ScreenshotApp, AnyWindowHandle) {
    app.update(|cx| cx.set_reduce_motion(false));
    let window = open(&mut app, (300., 72.), SCALE, move |window, cx| {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .smooth_caret(smooth)
                .default_value(text)
        });
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.set_selected_range(caret..caret, cx);
        });
        Field { input }
    });
    app.capture(window).unwrap();
    (app, window)
}

/// A single-line input's smooth caret 0, 25, 50 and 100 ms after a "W" is typed in the middle,
/// as [`smooth_caret`] for a text editor.
fn input_smooth_caret(mode: ThemeMode) -> Vec<(u64, Screenshot)> {
    let (mut app, window) = input_smooth_caret_window(app(mode), "Hello world", 5, true);
    act(&mut app, window, |window, cx| window.input("W", cx));
    let mut elapsed = 0;
    [0, 25, 50, 100]
        .into_iter()
        .map(|ms| {
            app.advance_clock(Duration::from_millis(ms - elapsed));
            elapsed = ms;
            (ms, app.capture(window).unwrap())
        })
        .collect()
}

struct Gutters {
    short: Entity<TextareaState>,
    long: Entity<TextareaState>,
    code: Entity<EditorState>,
}

impl Render for Gutters {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = || div().h(px(72.)).border_1().border_color(cx.theme().border);
        page(cx)
            .child(frame().child(TextEditor::new(&self.short)))
            .child(frame().child(TextEditor::new(&self.long)))
            .child(frame().child(Editor::new(&self.code).bordered(false).h(px(70.))))
    }
}

/// Line-number gutters sized to the last line's number: a text editor with a few lines takes
/// two digits, one scrolled to its end shows three, and a code editor keeps a column for its
/// folding markers. A thin line separates each gutter from the text.
fn gutters(scale: f32) -> Screenshot {
    let mut app = app(ThemeMode::Light);
    let opened = Rc::new(std::cell::RefCell::new(None));
    let window = open(&mut app, (320., 300.), scale, {
        let opened = opened.clone();
        move |window, cx| {
            let short = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .text_editor()
                    .default_value("A short note.\nIt has three lines.\nThe last one.")
            });
            let long_text = (1..=120)
                .map(|line| format!("Line {line} of a long document."))
                .collect::<Vec<_>>()
                .join("\n");
            let long = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .text_editor()
                    .default_value(long_text)
            });
            // No language, so the code editor looks the same with and without
            // tree-sitter.
            let code = cx.new(|cx| {
                EditorState::new(window, cx)
                    .default_value("fn main() {\n    println!(\"compact\");\n}")
            });
            long.update(cx, |long, cx| long.focus(window, cx));
            *opened.borrow_mut() = Some(long.clone());
            Gutters { short, long, code }
        }
    });
    let long = opened.borrow_mut().take().unwrap();
    app.capture(window).unwrap();
    act(&mut app, window, |window, cx| {
        let end = long.read(cx).text().len();
        long.update(cx, |long, cx| long.set_selected_range(end..end, cx));
        window.press("down", cx);
    });
    app.capture(window).unwrap()
}

struct FontPickerView {
    picker: Entity<FontPickerState>,
}

impl Render for FontPickerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(FontPicker::new(&self.picker))
    }
}

/// The font picker with JetBrains Mono chosen: the family list, its weight, its features with the
/// slashed zero turned on, and the preview line.
fn font_picker(mode: ThemeMode) -> Screenshot {
    let mut app = app(mode);
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!(
            "../../story-web/fonts/JetBrainsMono-Regular.source.ttf"
        )),
        Cow::Borrowed(include_bytes!("../../story-web/fonts/Inter-Regular.ttf")),
    ];
    app.add_fonts(fonts.clone()).unwrap();
    let catalog = FontCatalog::from_fonts(&[gpui_screenshot::bundled_fonts(), fonts].concat());
    let window = open(&mut app, (720., 400.), SCALE, move |window, cx| {
        let picker = cx.new(|cx| {
            FontPickerState::new(window, cx)
                .catalog(catalog)
                .default_settings(
                    FontSettings::new("JetBrains Mono")
                        .with_size(px(14.))
                        .with_feature("zero", true),
                )
        });
        FontPickerView { picker }
    });
    app.capture(window).unwrap()
}

struct CompactFontPickerView {
    picker: Entity<FontPickerState>,
}

impl Render for CompactFontPickerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(FontPicker::new(&self.picker).compact())
    }
}

/// The compact font picker with JetBrains Mono chosen and its slashed zero turned on: the
/// family in a dropdown, the weight, size and line height in rows, the features behind a
/// disclosure, open when `features` is set, and the preview line.
fn compact_font_picker(mode: ThemeMode, features: bool) -> Screenshot {
    let mut app = app(mode);
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!(
            "../../story-web/fonts/JetBrainsMono-Regular.source.ttf"
        )),
        Cow::Borrowed(include_bytes!("../../story-web/fonts/Inter-Regular.ttf")),
    ];
    app.add_fonts(fonts.clone()).unwrap();
    let catalog = FontCatalog::from_fonts(&[gpui_screenshot::bundled_fonts(), fonts].concat());
    let height = if features { 720. } else { 340. };
    let picker: Rc<RefCell<Option<Entity<FontPickerState>>>> = Rc::default();
    let window = open(&mut app, (560., height), SCALE, {
        let picker = picker.clone();
        move |window, cx| {
            let state = cx.new(|cx| {
                FontPickerState::new(window, cx)
                    .catalog(catalog)
                    .default_settings(
                        FontSettings::new("JetBrains Mono")
                            .with_size(px(14.))
                            .with_feature("zero", true),
                    )
            });
            *picker.borrow_mut() = Some(state.clone());
            CompactFontPickerView { picker: state }
        }
    });
    if features {
        let picker = picker.borrow().clone().unwrap();
        act(&mut app, window, |_, cx| {
            picker.update(cx, |picker, cx| picker.set_features_open(true, cx))
        });
    }
    app.capture(window).unwrap()
}

#[test]
fn compact_font_picker_light() {
    goldens().assert(
        "font-picker-compact",
        &compact_font_picker(ThemeMode::Light, false),
    );
}

#[test]
fn compact_font_picker_dark() {
    goldens().assert(
        "font-picker-compact-dark",
        &compact_font_picker(ThemeMode::Dark, false),
    );
}

#[test]
fn compact_font_picker_features_light() {
    goldens().assert(
        "font-picker-compact-features",
        &compact_font_picker(ThemeMode::Light, true),
    );
}

#[test]
fn compact_font_picker_features_dark() {
    goldens().assert(
        "font-picker-compact-features-dark",
        &compact_font_picker(ThemeMode::Dark, true),
    );
}

#[test]
fn font_picker_light() {
    goldens().assert("font-picker", &font_picker(ThemeMode::Light));
}

#[test]
fn font_picker_dark() {
    goldens().assert("font-picker-dark", &font_picker(ThemeMode::Dark));
}

struct FontPickerWithoutFilterView {
    picker: Entity<FontPickerState>,
    compact: bool,
}

impl Render for FontPickerWithoutFilterView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(
            FontPicker::new(&self.picker)
                .show_monospace_filter(false)
                .when(self.compact, FontPicker::compact),
        )
    }
}

fn font_picker_without_filter(mode: ThemeMode, compact: bool) -> Screenshot {
    let mut app = app(mode);
    let fonts: Vec<Cow<'static, [u8]>> = vec![Cow::Borrowed(include_bytes!(
        "../../story-web/fonts/JetBrainsMono-Regular.source.ttf"
    ))];
    app.add_fonts(fonts.clone()).unwrap();
    let catalog = FontCatalog::from_fonts(&[gpui_screenshot::bundled_fonts(), fonts].concat());
    let dimensions = if compact { (560., 340.) } else { (720., 400.) };
    let window = open(&mut app, dimensions, SCALE, move |window, cx| {
        let picker = cx.new(|cx| {
            FontPickerState::new(window, cx)
                .catalog(catalog)
                .default_settings(FontSettings::new("JetBrains Mono").with_size(px(14.)))
        });
        FontPickerWithoutFilterView { picker, compact }
    });
    app.capture(window).unwrap()
}

#[test]
fn font_picker_without_monospace_filter() {
    for (name, mode, compact) in [
        ("font-picker-no-filter", ThemeMode::Light, false),
        ("font-picker-no-filter-dark", ThemeMode::Dark, false),
        ("font-picker-compact-no-filter", ThemeMode::Light, true),
        ("font-picker-compact-no-filter-dark", ThemeMode::Dark, true),
    ] {
        goldens().assert(name, &font_picker_without_filter(mode, compact));
    }
}

#[test]
fn buttons_light() {
    goldens().assert("buttons", &buttons(ThemeMode::Light, SCALE));
}

#[test]
fn buttons_dark() {
    goldens().assert("buttons-dark", &buttons(ThemeMode::Dark, SCALE));
}

/// 2.625 is a common Android density (420 dpi). Fractional scales round edges and glyph
/// positions differently from whole ones.
#[test]
fn buttons_at_a_fractional_scale() {
    goldens().assert("buttons@2.625x", &buttons(ThemeMode::Light, 2.625));
}

struct Copies;

impl Render for Copies {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(
            div()
                .flex()
                .gap_2()
                .child(Clipboard::new("idle").value("one"))
                .child(Clipboard::new("copied").value("two")),
        )
    }
}

/// A Clipboard as it rests, and one just clicked, its check mark in the success color. Reduced
/// motion shows the check at once.
fn clipboard_copied(mode: ThemeMode) -> Screenshot {
    let mut app = app(mode);
    app.update(|cx| cx.set_reduce_motion(true));
    let window = open(&mut app, (120., 60.), SCALE, |_, _| Copies);
    act(&mut app, window, |window, cx| {
        window.render_frame(cx);
        window.click("copied", cx);
    });
    app.capture(window).unwrap()
}

#[test]
fn clipboard_copied_light() {
    goldens().assert("clipboard-copied", &clipboard_copied(ThemeMode::Light));
}

#[test]
fn clipboard_copied_dark() {
    goldens().assert("clipboard-copied-dark", &clipboard_copied(ThemeMode::Dark));
}

struct Meters;

impl Render for Meters {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let ring = |id: &'static str| {
            ProgressCircle::new(id)
                .value(62.)
                .tick(95.)
                .accessibility_label("Context")
        };
        page(cx)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(ring("ring"))
                    .child(ring("ring-large").size_10())
                    .child(
                        ProgressCircle::new("ring-segments")
                            .size_10()
                            .stroke_width(gpui_kit::rems(0.375))
                            .tick(95.)
                            .segments([
                                ProgressSegment::new(10., theme.chart_1),
                                ProgressSegment::new(35., theme.chart_2),
                                ProgressSegment::new(20., theme.chart_3),
                            ]),
                    )
                    .child(
                        ProgressCircle::new("ring-warning")
                            .size_10()
                            .value(91.)
                            .color(theme.warning)
                            .tick(95.),
                    ),
            )
            .child(
                UsageBar::new("split", "Context")
                    .value_text("650k / 1M")
                    .tick(95.)
                    .segments([
                        ProgressSegment::new(10., theme.chart_1)
                            .with_label("System and tools")
                            .with_value_text("about 100k"),
                        ProgressSegment::new(35., theme.chart_2)
                            .with_label("Conversation")
                            .with_value_text("about 350k"),
                        ProgressSegment::new(20., theme.chart_3)
                            .with_label("Tool results")
                            .with_value_text("about 200k"),
                    ])
                    .description("Compacts at 950k"),
            )
            .child(
                UsageBar::new("plan", "5-hour limit")
                    .small()
                    .value(82.)
                    .color(theme.warning)
                    .value_text("82%"),
            )
            // Past its limit: the tick stays visible over the fill.
            .child(Progress::new("past-limit").value(97.).tick(90.))
    }
}

fn meters(mode: ThemeMode) -> Screenshot {
    let mut app = app(mode);
    let window = open(&mut app, (360., 284.), SCALE, |_, _| Meters);
    app.capture(window).unwrap()
}

/// A ring with a tick at its limit, a segmented ring, and usage bars that name their segments.
#[test]
fn progress_meters() {
    goldens().assert("progress-meters", &meters(ThemeMode::Light));
}

#[test]
fn progress_meters_dark() {
    goldens().assert("progress-meters-dark", &meters(ThemeMode::Dark));
}

/// The weights a UI uses, in the bundled variable fonts.
const SPECIMEN_WEIGHTS: [FontWeight; 4] = [
    FontWeight::NORMAL,
    FontWeight::MEDIUM,
    FontWeight::SEMIBOLD,
    FontWeight::BOLD,
];

/// An app with GPUI Kit's bundled Work Sans and JetBrains Mono registered.
fn app_with_bundled_fonts() -> ScreenshotApp {
    let mut app = app(ThemeMode::Light);
    app.update(|cx| {
        cx.text_system()
            .add_fonts(gpui_kit_assets::fonts::all())
            .unwrap()
    });
    app
}

struct FontSpecimens;

impl Render for FontSpecimens {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit_assets::fonts::{JETBRAINS_MONO_FAMILY, WORK_SANS_FAMILY};
        let row = |family: &'static str, weight: FontWeight, italic: bool| {
            div()
                .font_family(family)
                .font_weight(weight)
                .when(italic, |this| this.italic())
                .child(format!(
                    "{family} {}{} Hamburgefonstiv 0123",
                    weight.0,
                    if italic { " italic" } else { "" }
                ))
        };
        page(cx)
            .gap_1()
            .children(SPECIMEN_WEIGHTS.map(|weight| row(WORK_SANS_FAMILY, weight, false)))
            .child(row(WORK_SANS_FAMILY, FontWeight::NORMAL, true))
            .child(row(WORK_SANS_FAMILY, FontWeight::BOLD, true))
            .child(row(JETBRAINS_MONO_FAMILY, FontWeight::NORMAL, false))
            .child(row(JETBRAINS_MONO_FAMILY, FontWeight::BOLD, false))
            .child(row(JETBRAINS_MONO_FAMILY, FontWeight::NORMAL, true))
    }
}

/// Work Sans and JetBrains Mono at the weights a UI uses, upright and italic.
#[test]
fn bundled_fonts() {
    let mut app = app_with_bundled_fonts();
    let window = open(&mut app, (480., 290.), SCALE, |_, _| FontSpecimens);
    goldens().assert("bundled-fonts", &app.capture(window).unwrap());
}

struct Ink(&'static str, FontWeight);

impl Render for Ink {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx)
            .font_family(self.0)
            .font_weight(self.1)
            .child("Hamburgefonstiv")
    }
}

/// How much ink a screenshot holds: the sum of how far each pixel is from white.
fn ink(screenshot: &Screenshot) -> u64 {
    screenshot
        .image
        .pixels()
        .map(|pixel| {
            let [r, g, b, _] = pixel.0;
            (765 - (r as u64 + g as u64 + b as u64)) / 3
        })
        .sum()
}

/// The bundled fonts are variable fonts, and each weight draws with its own instance: every step
/// from regular to bold puts more ink on the page. A static regular face would draw the same
/// strokes at every weight, or one fake bold.
#[test]
fn bundled_fonts_draw_each_weight() {
    use gpui_kit_assets::fonts::{JETBRAINS_MONO_FAMILY, WORK_SANS_FAMILY};
    let mut app = app_with_bundled_fonts();
    for family in [WORK_SANS_FAMILY, JETBRAINS_MONO_FAMILY] {
        let inks = SPECIMEN_WEIGHTS.map(|weight| {
            let window = open(&mut app, (240., 48.), SCALE, |_, _| Ink(family, weight));
            ink(&app.capture(window).unwrap())
        });
        assert!(
            inks.windows(2).all(|pair| pair[0] < pair[1]),
            "{family} draws {inks:?} ink at weights {:?}",
            SPECIMEN_WEIGHTS.map(|weight| weight.0)
        );
    }
}

/// An in-memory picture: a framed window mock-up in one hue, no text.
fn picture(width: u32, height: u32, hue: u32) -> std::sync::Arc<gpui_kit::Image> {
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}">
<rect width="100%" height="100%" fill="hsl({hue}, 45%, 62%)"/>
<rect x="5%" y="6%" width="90%" height="10%" rx="6" fill="hsl({hue}, 40%, 82%)"/>
<rect x="5%" y="22%" width="38%" height="68%" rx="6" fill="hsl({hue}, 40%, 75%)"/>
<rect x="48%" y="22%" width="47%" height="68%" rx="6" fill="hsl({hue}, 40%, 88%)"/>
</svg>"#
    );
    std::sync::Arc::new(gpui_kit::Image::from_bytes(
        gpui_kit::ImageFormat::Svg,
        svg.into_bytes(),
    ))
}

struct Behind;

impl Render for Behind {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(gpui_kit::component::TitleBar::new())
            .child(div().p_4().child("The window under the viewer"))
    }
}

fn image_viewer(mode: ThemeMode) -> Screenshot {
    use gpui_kit::component::{WindowExt as _, image_viewer::ImageViewerItem};
    let mut app = app(mode);
    let window = open(&mut app, (520., 360.), SCALE, |_, _| Behind);
    act(&mut app, window, |window, cx| {
        let items = [(400, 240, 210), (300, 300, 150), (640, 360, 20)]
            .into_iter()
            .enumerate()
            .map(|(ix, (width, height, hue))| {
                ImageViewerItem::new(picture(width, height, hue))
                    .with_caption(format!("shot-{}.png · {width} × {height}", ix + 1))
            });
        window.open_image_viewer(items, 1, cx);
    });
    app.capture(window).unwrap()
}

/// The image viewer over a window: the counter, the caption, the image fitted below the title
/// bar, the previous and next buttons.
#[test]
fn image_viewer_light() {
    goldens().assert("image-viewer", &image_viewer(ThemeMode::Light));
}

#[test]
fn image_viewer_dark() {
    goldens().assert("image-viewer-dark", &image_viewer(ThemeMode::Dark));
}

struct Changes(Entity<gpui_kit::component::diff_view::DiffViewState>);

impl Render for Changes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(cx.theme().background)
            .child(gpui_kit::component::diff_view::DiffView::new(&self.0))
    }
}

fn diff_view(mode: ThemeMode) -> Screenshot {
    use gpui_kit::component::diff_view::{Diff, DiffViewState};
    let old = "fn timeout() -> u64 {\n    30\n}\n\nfn retries() -> u32 {\n    3\n}\n\n\n\n\n\nfn log(message: &str) {\n    eprintln!(\"{message}\");\n}\n";
    let new = "fn timeout() -> u64 {\n    10\n}\n\nfn retries() -> u32 {\n    3\n}\n\n\n\n\n\nfn log(message: &str) {\n    tracing::info!(\"{message}\");\n    flush();\n}\n";
    let mut app = app(mode);
    let window = open(&mut app, (480., 400.), SCALE, |_, cx| {
        Changes(cx.new(|cx| DiffViewState::new(Diff::between("src/net.rs", old, new), cx)))
    });
    app.capture(window).unwrap()
}

/// A computed diff: the file row with its counts, hunk headers, the fold between the hunks,
/// numbered added and removed lines, and Rust's syntax colors. They are found on a background
/// thread, which the capture waits for, with the Rust grammar this crate's Linux tests enable.
#[test]
fn diff_view_light() {
    goldens().assert("diff-view", &diff_view(ThemeMode::Light));
}

#[test]
fn diff_view_dark() {
    goldens().assert("diff-view-dark", &diff_view(ThemeMode::Dark));
}

fn diff_view_notes(mode: ThemeMode) -> Screenshot {
    use gpui_kit::component::diff_view::{Diff, DiffViewState};
    let text = "diff --git a/src/old.rs b/src/new.rs\nsimilarity index 100%\n\
                rename from src/old.rs\nrename to src/new.rs\n\
                diff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\n\
                diff --git a/logo.png b/logo.png\n\
                Binary files a/logo.png and b/logo.png differ\n\
                diff --git a/notes.txt b/notes.txt\n--- a/notes.txt\n+++ b/notes.txt\n\
                @@ -1,2 +1,2 @@\n-first\r\n+first\n-last\n\\ No newline at end of file\n+last\n";
    let mut app = app(mode);
    let window = open(&mut app, (480., 220.), SCALE, |_, cx| {
        Changes(cx.new(|cx| DiffViewState::new(Diff::parse(text), cx)))
    });
    app.capture(window).unwrap()
}

/// A parsed git diff's notes: a rename's two paths, a mode change, a binary file, a line break
/// changed from CRLF to LF, and a final newline added.
#[test]
fn diff_view_notes_light() {
    goldens().assert("diff-view-notes", &diff_view_notes(ThemeMode::Light));
}

#[test]
fn diff_view_notes_dark() {
    goldens().assert("diff-view-notes-dark", &diff_view_notes(ThemeMode::Dark));
}

struct Bar;

impl Render for Bar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .child(gpui_kit::component::TitleBar::new())
    }
}

/// The title bar is the theme's `title_bar.background` from its top edge down to its border, one
/// flat color. It used to fade from a mix with the window background at the top, a shadow on
/// themes whose background is darker, which Windows and macOS also dither into grain.
#[test]
fn title_bar_is_one_flat_color() {
    let mut app = app(ThemeMode::Dark);
    // The jot theme macOS Classic Dark, whose window background is darker than its title bar.
    app.update(|cx| {
        Theme::update(cx, |theme| {
            theme.title_bar = gpui_kit::rgb(0x1c1c1e).into();
            theme.background = gpui_kit::rgb(0x131313).into();
        })
    });
    let window = open(&mut app, (400., 60.), SCALE, |_, _| Bar);
    let image = app.capture(window).unwrap().image;

    // Down the middle of the empty bar, every device row above the 1 px bottom border.
    let x = image.width() / 2;
    let height = gpui_kit::component::TITLE_BAR_HEIGHT.as_f32();
    for y in 0..((height - 1.) * SCALE) as u32 {
        let [r, g, b, _] = image.get_pixel(x, y).0;
        assert!(
            [r, g, b]
                .into_iter()
                .zip([0x1c_u8, 0x1c, 0x1e])
                .all(|(actual, expected)| actual.abs_diff(expected) <= 2),
            "row {y} of the title bar is {:?}, not #1c1c1e",
            [r, g, b]
        );
    }
}

/// A title bar with menus at the start, a button at the end and a document's title in the middle.
struct Titled;

impl Render for Titled {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = |name: &'static str| Button::new(name).label(name).small().ghost();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                gpui_kit::component::TitleBar::new()
                    .child(
                        div()
                            .flex()
                            .child(menu("File"))
                            .child(menu("Edit"))
                            .child(menu("View")),
                    )
                    .center(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child("Quarterly report.md · Documents"),
                    )
                    .suffix(
                        Button::new("sidebar")
                            .icon(IconName::PanelLeft)
                            .small()
                            .ghost()
                            .compact(),
                    ),
            )
    }
}

fn title_bar(mode: ThemeMode, width: f32) -> Screenshot {
    let mut app = app(mode);
    let window = open(&mut app, (width, 48.), SCALE, |_, _| Titled);
    app.capture(window).unwrap()
}

/// The title sits in the middle of the window, not of the space between the menus and the button.
#[test]
fn title_bar_center_light() {
    goldens().assert("title-bar-center", &title_bar(ThemeMode::Light, 560.));
}

#[test]
fn title_bar_center_dark() {
    goldens().assert("title-bar-center-dark", &title_bar(ThemeMode::Dark, 560.));
}

/// In a narrow window the title moves aside for the menus and ends with an ellipsis before the
/// button.
#[test]
fn title_bar_center_narrow() {
    goldens().assert(
        "title-bar-center-narrow",
        &title_bar(ThemeMode::Light, 300.),
    );
}

#[test]
fn input_and_textarea() {
    goldens().assert("fields", &fields());
}

#[test]
fn list_with_keyboard_cursor() {
    goldens().assert("list", &list());
}

#[test]
fn popup_menu() {
    goldens().assert("menu", &menu(ThemeMode::Light));
}

#[test]
fn popup_menu_dark() {
    goldens().assert("menu-dark", &menu(ThemeMode::Dark));
}

#[test]
fn popup_menu_shortcuts() {
    goldens().assert("menu-shortcuts", &shortcut_menu(ThemeMode::Light));
}

#[test]
fn popup_menu_shortcuts_dark() {
    goldens().assert("menu-shortcuts-dark", &shortcut_menu(ThemeMode::Dark));
}

#[test]
fn textarea_suggestion_menu() {
    goldens().assert("suggestions", &suggestion_menu(ThemeMode::Light));
}

#[test]
fn textarea_suggestion_menu_dark() {
    goldens().assert("suggestions-dark", &suggestion_menu(ThemeMode::Dark));
}

#[test]
fn compact_line_number_gutters() {
    goldens().assert("gutters", &gutters(SCALE));
}

/// The line beside each gutter covers whole device pixels at a fractional scale too.
#[test]
fn compact_line_number_gutters_at_a_fractional_scale() {
    goldens().assert("gutters@2.625x", &gutters(2.625));
}

#[test]
fn text_editor_gutter_and_underlines() {
    goldens().assert("text-editor", &text_editor(ThemeMode::Light));
}

#[test]
fn text_editor_gutter_and_underlines_dark() {
    goldens().assert("text-editor-dark", &text_editor(ThemeMode::Dark));
}

#[test]
fn text_editor_spelling_menu() {
    goldens().assert("text-editor-menu", &text_editor_menu(ThemeMode::Light));
}

#[test]
fn text_editor_spelling_menu_dark() {
    goldens().assert("text-editor-menu-dark", &text_editor_menu(ThemeMode::Dark));
}

#[test]
fn smooth_caret_uncovering_a_letter() {
    for (ms, shot) in smooth_caret(ThemeMode::Light) {
        goldens().assert(&format!("smooth-caret-{ms}ms"), &shot);
    }
}

#[test]
fn smooth_caret_uncovering_a_letter_dark() {
    for (ms, shot) in smooth_caret(ThemeMode::Dark) {
        goldens().assert(&format!("smooth-caret-{ms}ms-dark"), &shot);
    }
}

#[test]
fn input_smooth_caret_uncovering_a_letter() {
    for (ms, shot) in input_smooth_caret(ThemeMode::Light) {
        goldens().assert(&format!("input-smooth-caret-{ms}ms"), &shot);
    }
}

/// Once its caret has settled, a single-line input is drawn exactly as without the smooth
/// caret.
#[test]
fn input_smooth_caret_settles_to_the_plain_drawing() {
    let settled = |smooth| {
        let (mut app, window) =
            input_smooth_caret_window(app(ThemeMode::Light), "Hello world", 5, smooth);
        act(&mut app, window, |window, cx| window.input("W", cx));
        app.advance_clock(Duration::from_millis(100));
        app.capture(window).unwrap().image
    };
    assert!(settled(true) == settled(false));
}

/// Once the caret has settled, the line is drawn exactly as without the smooth caret.
#[test]
fn smooth_caret_settles_to_the_plain_drawing() {
    let settled = |smooth| {
        let (mut app, window, _) =
            smooth_caret_window(app(ThemeMode::Light), "Hello world", 5, smooth);
        act(&mut app, window, |window, cx| window.input("W", cx));
        app.advance_clock(Duration::from_millis(100));
        app.capture(window).unwrap().image
    };
    assert!(settled(true) == settled(false));
}

/// Typing at the end of a line, every frame of the glide draws nothing right of the caret: the
/// letter is uncovered by the caret, never ahead of it. The caret is red here so it can be told
/// apart from the text.
#[test]
fn smooth_caret_draws_nothing_right_of_the_caret() {
    let mut app = app(ThemeMode::Light);
    app.update(|cx| Theme::update(cx, |theme| theme.caret = gpui_kit::red()));
    let (mut app, window, document) = smooth_caret_window(app, "Hello", 5, true);
    act(&mut app, window, |window, cx| window.input("W", cx));
    let (caret, input) = app.update(|cx| {
        let document = document.read(cx);
        (document.cursor_layout().unwrap().0, document.input_bounds())
    });
    let device = |x: gpui_kit::Pixels| (x.as_f32() * SCALE) as u32;
    let (top, bottom) = (device(caret.top()), device(caret.bottom()));
    let middle = (top + bottom) / 2;
    let right = device(input.right()) - 8;

    let mut uncovered = Vec::new();
    for _ in 0..20 {
        let image = app.capture(window).unwrap().image;
        let red = |x: u32| {
            let [r, g, b, _] = image.get_pixel(x, middle).0;
            r > 200 && g < 80 && b < 80
        };
        let caret_end = (0..right)
            .filter(|x| red(*x))
            .max()
            .expect("the caret is drawn while it glides");
        let background = image.get_pixel(right, middle).0;
        for y in top..bottom {
            for x in caret_end + 1..right {
                let pixel = image.get_pixel(x, y).0;
                assert!(
                    pixel
                        .iter()
                        .zip(background)
                        .all(|(a, b)| a.abs_diff(b) <= 2),
                    "ink at ({x}, {y}), right of the caret ending at {caret_end}: {pixel:?}"
                );
            }
        }
        uncovered.push(caret_end);
        app.advance_clock(Duration::from_millis(5));
    }
    assert!(
        uncovered.windows(2).all(|pair| pair[0] <= pair[1]) && uncovered[0] < uncovered[19],
        "the caret glides right: {uncovered:?}"
    );
}

/// Renders every scene on three threads at once: each thread must produce the same pixels,
/// whatever the order the threads share the software device in.
#[test]
fn every_thread_renders_the_same_pixels() {
    let scenes = || {
        [
            buttons(ThemeMode::Light, SCALE),
            buttons(ThemeMode::Dark, 2.625),
            fields(),
            list(),
            menu(ThemeMode::Light),
        ]
        .map(|screenshot| screenshot.image)
    };
    let runs: Vec<_> = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..3).map(|_| scope.spawn(scenes)).collect();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect()
    });
    for run in &runs[1..] {
        for (scene, image) in run.iter().enumerate() {
            assert!(
                *image == runs[0][scene],
                "scene {scene} differs between threads"
            );
        }
    }
}

struct KeyboardFocus;

impl Render for KeyboardFocus {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::{
            sidebar::{SidebarItem as _, SidebarMenu, SidebarMenuItem},
            tab::{Tab, TabBar},
        };
        page(cx)
            .child(
                TabBar::new("focus-tabs")
                    .selected_index(0)
                    .child(Tab::new().label("General"))
                    .child(Tab::new().label("Appearance"))
                    .child(Tab::new().label("Advanced")),
            )
            .child(
                TabBar::new("focus-segmented")
                    .segmented()
                    .selected_index(0)
                    .child(Tab::new().label("Day"))
                    .child(Tab::new().label("Week"))
                    .child(Tab::new().label("Month")),
            )
            .child(
                div().w(px(200.)).child(
                    SidebarMenu::new()
                        .child(
                            SidebarMenuItem::new("Inbox")
                                .icon(IconName::Bell)
                                .active(true),
                        )
                        .child(SidebarMenuItem::new("Drafts").icon(IconName::File))
                        .child(SidebarMenuItem::new("Settings").icon(IconName::Settings))
                        .render("focus-menu", window, cx),
                ),
            )
    }
}

/// Keyboard focus on the second item of a control, after focus reaches the control's Tab stop
/// and an arrow key moves along.
fn keyboard_focus(mode: ThemeMode, stops: usize, arrow: &str) -> Screenshot {
    let mut app = app(mode);
    app.update(|cx| cx.set_reduce_motion(true));
    let window = open(&mut app, (360., 260.), SCALE, |_, _| KeyboardFocus);
    for _ in 0..stops {
        act(&mut app, window, |window, cx| window.focus_next(cx));
    }
    act(&mut app, window, |window, cx| {
        window.dispatch_keystroke(gpui_kit::Keystroke::parse(arrow).unwrap(), cx);
    });
    app.advance_clock(Duration::from_secs(1));
    app.capture(window).unwrap()
}

#[test]
fn keyboard_focus_rings() {
    for (mode, suffix) in [(ThemeMode::Light, ""), (ThemeMode::Dark, "-dark")] {
        goldens().assert(
            &format!("tab-keyboard-focus{suffix}"),
            &keyboard_focus(mode, 1, "right"),
        );
        goldens().assert(
            &format!("segmented-tab-keyboard-focus{suffix}"),
            &keyboard_focus(mode, 2, "right"),
        );
        goldens().assert(
            &format!("sidebar-keyboard-focus{suffix}"),
            &keyboard_focus(mode, 3, "down"),
        );
    }
}
