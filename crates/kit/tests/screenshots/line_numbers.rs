use super::*;
use gpui_kit::{Pixels, point, relative};

/// The line counts of each row of editors.
const LINE_COUNTS: [usize; 5] = [9, 10, 99, 100, 1_000];

/// The code the code editors repeat, a line at a time. The last line of every count is one of
/// the two that start at the left edge.
const CODE: [&str; 5] = [
    "for item in &items {",
    "    total += item.weight;",
    "}",
    "let total = items.len();",
    "let mut count = 0;",
];

/// The prose the text editors repeat, a line at a time.
const PROSE: [&str; 5] = [
    "Garden club",
    "Bring jars for the green tomatoes.",
    "",
    "Next month we plant garlic.",
    "Leave the shed key under the pot.",
];

const WRAPPED_PROSE: &str = "Garden club\nThe frost held off again this week, so the tomatoes \
are still going. Bring jars if you want some green ones for chutney.\nNext month we plant garlic.";

const WRAPPED_CODE: &str = "fn main() {\n    let message = format!(\"{} of {} rows were \
copied to the archive\", copied, total);\n}";

/// The size every editor draws its text at, the code editors and the text editors alike.
#[derive(Clone, Copy, Debug)]
enum TextSize {
    /// 14 px text on 20 px rows.
    Default,
    /// 20 px text on 28 px rows.
    Large,
}

impl TextSize {
    fn size(self) -> Pixels {
        match self {
            Self::Default => px(14.),
            Self::Large => px(20.),
        }
    }

    fn line_height(self) -> f32 {
        match self {
            Self::Default => 20.,
            Self::Large => 28.,
        }
    }

    /// The rows the wrapped editors show.
    fn wrapped_rows(self) -> usize {
        match self {
            Self::Default => 5,
            Self::Large => 6,
        }
    }

    fn window(self) -> (f32, f32) {
        match self {
            Self::Default => (760., 600.),
            Self::Large => (900., 788.),
        }
    }

    /// The height of a frame showing `rows` rows, the editor's 8 px top and bottom padding and
    /// the frame's border.
    fn frame_height(self, rows: usize) -> f32 {
        self.line_height() * rows as f32 + 18.
    }
}

#[derive(Clone)]
struct LineNumbers {
    code: Vec<Entity<EditorState>>,
    prose: Vec<Entity<TextareaState>>,
    text_size: TextSize,
}

impl Render for LineNumbers {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text_size = self.text_size;
        let frame = |rows: usize| {
            div()
                .flex_1()
                .min_w_0()
                .h(px(text_size.frame_height(rows)))
                .border_1()
                .border_color(cx.theme().border)
        };
        let rows = (0..self.code.len()).map(|ix| {
            let shown = if ix < LINE_COUNTS.len() {
                3
            } else {
                text_size.wrapped_rows()
            };
            div()
                .flex()
                .gap_3()
                .child(
                    frame(shown).child(
                        Editor::new(&self.code[ix])
                            .bordered(false)
                            .h(relative(1.))
                            .text_size(text_size.size())
                            .line_height(px(text_size.line_height())),
                    ),
                )
                .child(
                    frame(shown).child(
                        TextEditor::new(&self.prose[ix])
                            .text_size(text_size.size())
                            .line_height(px(text_size.line_height())),
                    ),
                )
        });
        page(cx).children(rows)
    }
}

fn repeat(lines: &[&str], count: usize) -> String {
    (0..count)
        .map(|line| lines[line % lines.len()])
        .collect::<Vec<_>>()
        .join("\n")
}

/// Code editors on the left and text editors on the right, with 9, 10, 99, 100 and 1,000 lines,
/// each scrolled to its end with the caret on the last line, then both with the caret on a
/// wrapped line. Every editor draws its text at the same size.
fn line_numbers_in(mut app: ScreenshotApp, text_size: TextSize) -> Screenshot {
    let opened: Rc<RefCell<Option<LineNumbers>>> = Rc::default();
    let window = open(&mut app, text_size.window(), SCALE, {
        let opened = opened.clone();
        move |window, cx| {
            let mut code = Vec::new();
            let mut prose = Vec::new();
            for &count in &LINE_COUNTS {
                code.push(cx.new(|cx| {
                    EditorState::new(window, cx)
                        .soft_wrap(false)
                        .scroll_beyond_last_line(Some(0))
                        .default_value(repeat(&CODE, count))
                }));
                prose.push(cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .text_editor()
                        .scroll_beyond_last_line(Some(0))
                        .default_value(repeat(&PROSE, count))
                }));
            }
            code.push(cx.new(|cx| {
                EditorState::new(window, cx)
                    .soft_wrap(true)
                    .default_value(WRAPPED_CODE)
            }));
            prose.push(cx.new(|cx| {
                TextareaState::new(window, cx)
                    .text_editor()
                    .default_value(WRAPPED_PROSE)
            }));
            let view = LineNumbers {
                code,
                prose,
                text_size,
            };
            *opened.borrow_mut() = Some(view.clone());
            view
        }
    });
    let view = opened.borrow_mut().take().unwrap();
    app.capture(window).unwrap();
    act(&mut app, window, |_, cx| {
        let wrapped = LINE_COUNTS.len();
        for ix in 0..wrapped {
            view.code[ix].update(cx, |state, cx| {
                let end = state.text().len();
                state.set_selected_range(end..end, cx);
                state.set_scroll_offset(point(px(0.), px(-100_000.)), cx);
            });
            view.prose[ix].update(cx, |state, cx| {
                let end = state.text().len();
                state.set_selected_range(end..end, cx);
                state.set_scroll_offset(point(px(0.), px(-100_000.)), cx);
            });
        }
        view.code[wrapped].update(cx, |state, cx| {
            let caret = WRAPPED_CODE.find("archive").unwrap();
            state.set_selected_range(caret..caret, cx);
        });
        view.prose[wrapped].update(cx, |state, cx| {
            let caret = WRAPPED_PROSE.find("chutney").unwrap();
            state.set_selected_range(caret..caret, cx);
        });
    });
    // The scroll offsets apply after the next layout.
    app.capture(window).unwrap();
    app.capture(window).unwrap()
}

fn line_numbers(mode: ThemeMode, text_size: TextSize) -> Screenshot {
    line_numbers_in(app(mode), text_size)
}

#[test]
fn line_numbers_light() {
    goldens().assert(
        "line-numbers",
        &line_numbers(ThemeMode::Light, TextSize::Default),
    );
}

#[test]
fn line_numbers_dark() {
    goldens().assert(
        "line-numbers-dark",
        &line_numbers(ThemeMode::Dark, TextSize::Default),
    );
}

#[test]
fn line_numbers_large_light() {
    goldens().assert(
        "line-numbers-large",
        &line_numbers(ThemeMode::Light, TextSize::Large),
    );
}

#[test]
fn line_numbers_large_dark() {
    goldens().assert(
        "line-numbers-large-dark",
        &line_numbers(ThemeMode::Dark, TextSize::Large),
    );
}

/// What a row of one editor shows, in logical pixels from the frame's inner left edge.
#[derive(Debug)]
struct Gutter {
    /// The ink of the line number.
    number: (f32, f32),
    /// The line at the gutter's edge.
    border: f32,
    /// Where the text's ink starts.
    text: f32,
    /// The current line's fill, from its first column to its last.
    current_line: (f32, f32),
}

/// Reads one row of an editor framed at `frame_x` (logical px, the frame's left border) and
/// `frame_width` wide, `row_top` being the row's top and `fill` the current line's color.
fn read_row(
    shot: &Screenshot,
    frame_x: f32,
    frame_width: f32,
    row_top: f32,
    line_height: f32,
    fill: [u8; 3],
) -> Gutter {
    let image = &shot.image;
    let px_of = |x: u32, y: u32| {
        let [r, g, b, _] = image.get_pixel(x, y).0;
        [r, g, b]
    };
    let device = |v: f32| (v * SCALE).round() as u32;
    let inner = device(frame_x + 1.);
    // Leave out the scrollbar at the right.
    let right = device(frame_x + frame_width - 16.);
    let (top, bottom) = (device(row_top), device(row_top + line_height));
    let distance = |a: [u8; 3], b: [u8; 3]| {
        a.iter()
            .zip(b)
            .map(|(a, b)| (*a as i32 - b as i32).abs())
            .sum::<i32>()
    };
    let logical = |x: u32| (x - inner) as f32 / SCALE;
    let mut glyph_columns = Vec::new();
    let mut lines = Vec::new();
    for x in inner..right {
        // A border is a faint line over the whole row, a glyph a dark mark in part of it.
        if (top..bottom).all(|y| distance(px_of(x, y), fill) > 12) {
            lines.push(x);
        } else if (top..bottom).any(|y| distance(px_of(x, y), fill) > 60) {
            glyph_columns.push(x);
        }
    }
    // Glyphs of one word sit closer than 3 logical px, and the gutter's parts further apart.
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for x in glyph_columns {
        match runs.last_mut() {
            Some(run) if x - run.1 <= device(3.) => run.1 = x,
            _ => runs.push((x, x)),
        }
    }
    let number = runs[0];
    let text = runs[1].0;
    let border = *lines
        .iter()
        .find(|&&x| x > number.1 && x < text)
        .expect("a line at the gutter's edge");
    // The fill shows above the glyphs, in the row's top device pixel.
    let filled: Vec<u32> = (device(frame_x + 1.)..device(frame_x + frame_width - 1.))
        .filter(|&x| px_of(x, top + 1) == fill)
        .collect();
    Gutter {
        number: (logical(number.0), logical(number.1 + 1)),
        border: logical(border),
        text: logical(text),
        current_line: (
            logical(*filled.first().expect("a current line")),
            logical(*filled.last().unwrap() + 1),
        ),
    }
}

/// The code editor and the text editor share one gutter at the same text size, in light and
/// dark and at both sizes. Read from the renders: the line numbers end at the same x, the line at
/// the gutter's edge sits at the same x, the text starts at the same x to within a glyph's side
/// bearing, and the current line's fill spans the same columns. The gutter widens by one digit
/// at 100 lines and at 1,000, and by the same amount each time. The space before the numbers
/// and between them and the gutter's edge is one digit each, and the space after the edge is
/// two digits and at least 18 px.
#[test]
fn every_editor_follows_one_gutter() {
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for text_size in [TextSize::Default, TextSize::Large] {
            let mut app = app(mode);
            let (fill, digit) = app.update(|cx| {
                let theme = cx.theme();
                let fill = theme
                    .highlight_theme
                    .style
                    .editor_active_line
                    .expect("the default themes mark the current line");
                let font = gpui_kit::font(theme.mono_font_family.clone());
                let text_system = cx.text_system();
                let digit = text_system
                    .advance(text_system.resolve_font(&font), text_size.size(), '0')
                    .unwrap()
                    .width
                    .as_f32();
                (rgb(fill), digit)
            });
            let shot = line_numbers_in(app, text_size);
            let lh = text_size.line_height();
            let (width, _) = text_size.window();
            let column = (width - 32. - 12.) / 2.;
            let (left_x, right_x) = (16., 16. + column + 12.);
            let mut top = 16.;
            let mut previous: Option<(usize, f32)> = None;
            let mut steps = Vec::new();
            for &count in &LINE_COUNTS {
                let row_top = top + 1. + 8. + 2. * lh;
                let code = read_row(&shot, left_x, column, row_top, lh, fill);
                let prose = read_row(&shot, right_x, column, row_top, lh, fill);
                let at = format!("{mode:?} {text_size:?}, {count} lines");
                let near = |a: f32, b: f32, tolerance: f32, what: &str| {
                    assert!((a - b).abs() <= tolerance, "{at}: {what} {a} and {b}");
                };
                near(code.number.1, prose.number.1, 0., "numbers end at");
                near(code.border, prose.border, 0., "gutter edges at");
                near(code.text, prose.text, 1.5, "text starts at");
                near(
                    code.current_line.0,
                    prose.current_line.0,
                    0.,
                    "fills start at",
                );
                near(
                    code.current_line.1,
                    prose.current_line.1,
                    0.,
                    "fills end at",
                );
                near(code.current_line.0, 0., 0., "the fill starts at the frame");
                // One digit before the numbers and one between them and the edge, read from
                // the ink, which sits inside each digit's advance.
                let digits = count.to_string().len().max(2);
                let column_start = code.number.1 - digit * digits as f32;
                near(column_start, digit, 1.5, "the space before the numbers");
                near(
                    code.border - code.number.1,
                    digit,
                    1.5,
                    "the space after the numbers",
                );
                near(
                    code.text - code.border,
                    (2. * digit).max(18.),
                    2.,
                    "the margin",
                );
                if let Some((previous_digits, previous_border)) = previous {
                    if previous_digits == digits {
                        near(code.border, previous_border, 0., "same digits, same gutter");
                    } else {
                        steps.push(code.border - previous_border);
                    }
                }
                previous = Some((digits, code.border));
                top += text_size.frame_height(3) + 12.;
            }
            assert_eq!(
                steps.len(),
                2,
                "the gutter widens at 100 and at 1,000 lines"
            );
            assert!(
                (steps[0] - steps[1]).abs() <= 0.5,
                "each digit adds {steps:?}"
            );
        }
    }
}
