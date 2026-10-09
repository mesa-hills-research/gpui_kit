//! Golden screenshots of GPUI Kit components, rendered by the gpui fork's gpui-pre-screenshot
//! on Mesa's software Vulkan driver. The goldens live in `tests/screenshots`. On a mismatch the
//! actual image and a diff go to `tests/screenshots/failures`, and `UPDATE_GOLDENS=1` rewrites
//! the goldens. The gpui fork's `docs/screenshots.md` explains how to review and update them.
#![cfg(target_os = "linux")]

use std::{borrow::Cow, cell::RefCell, rc::Rc, time::Duration};

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Focusable as _, InputEvent as _,
    IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement as _, Render,
    Result, SharedString, Styled as _, Task, Window,
    assets::Assets,
    component::{
        ActiveTheme as _, Disableable as _, IconName, IndexPath, Theme, ThemeMode,
        button::{Button, ButtonVariants as _},
        font_picker::{FontCatalog, FontPicker, FontPickerState, FontSettings},
        input::{
            Editor, EditorState, Input, InputState, RopeExt as _, SpellCheck, SpellCheckRequest,
            SpellChecker, Suggestion, SuggestionProvider, SuggestionRequest, TextEditor, Textarea,
            TextareaState,
        },
        list::{List, ListDelegate, ListItem, ListState},
        menu::{PopupMenu, PopupMenuItem},
    },
    div, px, size,
    test::TestWindowExt as _,
};
use gpui_screenshot::{Goldens, Screenshot, ScreenshotApp, Tolerance};

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

/// The smooth caret 0, 50, 100 and 200 ms after a "W" is typed in the middle of a line. The
/// caret uncovers the letter as it glides, the rest of the line moves with it, and after 200 ms
/// the line is drawn as without the smooth caret.
fn smooth_caret(mode: ThemeMode) -> Vec<(u64, Screenshot)> {
    let (mut app, window, _) = smooth_caret_window(app(mode), "Hello world", 5, true);
    act(&mut app, window, |window, cx| window.input("W", cx));
    let mut elapsed = 0;
    [0, 50, 100, 200]
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

/// Once the caret has settled, the line is drawn exactly as without the smooth caret.
#[test]
fn smooth_caret_settles_to_the_plain_drawing() {
    let settled = |smooth| {
        let (mut app, window, _) =
            smooth_caret_window(app(ThemeMode::Light), "Hello world", 5, smooth);
        act(&mut app, window, |window, cx| window.input("W", cx));
        app.advance_clock(Duration::from_millis(200));
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
        app.advance_clock(Duration::from_millis(10));
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
