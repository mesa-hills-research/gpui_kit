//! Golden screenshots of GPUI Kit components, rendered by the gpui fork's gpui-pre-screenshot
//! on Mesa's software Vulkan driver. The goldens live in `tests/screenshots`. On a mismatch the
//! actual image and a diff go to `tests/screenshots/failures`, and `UPDATE_GOLDENS=1` rewrites
//! the goldens. The gpui fork's `docs/screenshots.md` explains how to review and update them.
#![cfg(target_os = "linux")]

use std::rc::Rc;

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Focusable as _, IntoElement,
    ParentElement as _, Render, Result, Styled as _, Task, Window,
    assets::Assets,
    component::{
        ActiveTheme as _, Disableable as _, IconName, IndexPath, Theme, ThemeMode,
        button::{Button, ButtonVariants as _},
        input::{
            Input, InputState, RopeExt as _, Suggestion, SuggestionProvider, SuggestionRequest,
            Textarea, TextareaState,
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
