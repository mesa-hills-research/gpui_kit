use super::*;
use gpui_kit::component::{
    input::{ContextMenuStyle, InputGroup, InputGroupInput, NumberInput, OtpInput, OtpState},
    time_field::{TimeField, TimeFieldState},
};

const FIELDS: [&str; 6] = ["input", "textarea", "number", "otp", "group", "time"];

struct FocusedField {
    kind: usize,
    input: Entity<InputState>,
    textarea: Entity<TextareaState>,
    otp: Entity<OtpState>,
    time: Entity<TimeFieldState>,
}

impl Render for FocusedField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let field = match self.kind {
            0 => Input::new(&self.input).into_any_element(),
            1 => Textarea::new(&self.textarea).into_any_element(),
            2 => NumberInput::new(&self.input).into_any_element(),
            3 => OtpInput::new(&self.otp).into_any_element(),
            4 => InputGroup::new("group")
                .input(InputGroupInput::new(&self.input))
                .into_any_element(),
            5 => TimeField::new(&self.time).into_any_element(),
            _ => unreachable!(),
        };
        page(cx)
            .child(FIELDS[self.kind])
            .child(div().flex().w_full().child(field))
    }
}

/// The theme's outer-ring setting must not change a text field's focused pixels.
#[test]
fn text_fields_use_one_focus_border() {
    for (mode, suffix) in [(ThemeMode::Light, ""), (ThemeMode::Dark, "-dark")] {
        for (kind, name) in FIELDS.into_iter().enumerate() {
            let mut app = app(mode);
            app.update(|cx| cx.set_reduce_motion(true));
            let window = open(&mut app, (320., 150.), SCALE, |window, cx| {
                let input = cx.new(|cx| {
                    InputState::new(window, cx).default_value(if kind == 2 {
                        "42"
                    } else {
                        "A focused field"
                    })
                });
                let textarea = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .rows(2)
                        .default_value("A focused textarea")
                });
                let otp = cx.new(|cx| OtpState::new(6, window, cx).default_value("123"));
                let time = cx.new(|cx| TimeFieldState::new(window, cx));
                let focus = match kind {
                    0 | 2 | 4 => input.focus_handle(cx),
                    1 => textarea.focus_handle(cx),
                    3 => otp.focus_handle(cx),
                    5 => time.focus_handle(cx),
                    _ => unreachable!(),
                };
                focus.focus(window, cx);
                FocusedField {
                    kind,
                    input,
                    textarea,
                    otp,
                    time,
                }
            });
            app.advance_clock(Duration::from_millis(100));
            let focused = app.capture(window).unwrap();
            goldens().assert(&format!("focus-border-{name}{suffix}"), &focused);
            app.update(|cx| Theme::update(cx, |theme| theme.focus_ring = false));
            let without_outer_ring = app.capture(window).unwrap();
            assert!(
                focused.image == without_outer_ring.image,
                "the {name} focus border must not depend on the theme's outer ring"
            );
            act(&mut app, window, |window, cx| window.blur(cx));
            app.advance_clock(Duration::from_millis(100));
            let blurred = app.capture(window).unwrap();
            assert!(
                focused.image != blurred.image,
                "the {name} focus indicator must remain visible"
            );
        }
    }
}

/// A text field's drawn context menu takes the keyboard focus as it opens,
/// and the field goes on looking focused while the menu is open. Its
/// selection, caret and focus border stay as they were.
#[test]
fn a_text_field_looks_focused_under_its_drawn_menu() {
    for (kind, name) in [(0, "input"), (1, "textarea")] {
        let mut app = app(ThemeMode::Light);
        app.update(|cx| {
            cx.set_reduce_motion(true);
            ContextMenuStyle::set_app_default(ContextMenuStyle::Drawn, cx);
        });
        let fields = Rc::new(RefCell::new(None));
        let window = open(&mut app, (320., 260.), SCALE, {
            let fields = fields.clone();
            move |window, cx| {
                let input =
                    cx.new(|cx| InputState::new(window, cx).default_value("A focused field"));
                let textarea = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .rows(2)
                        .default_value("A focused textarea")
                });
                *fields.borrow_mut() = Some((input.clone(), textarea.clone()));
                FocusedField {
                    kind,
                    input,
                    textarea,
                    otp: cx.new(|cx| OtpState::new(6, window, cx)),
                    time: cx.new(|cx| TimeFieldState::new(window, cx)),
                }
            }
        });
        let (input, textarea) = fields.borrow_mut().take().unwrap();
        // The input's state and the textarea's are different types.
        macro_rules! field {
            (|$field:ident| $body:expr) => {
                if kind == 0 {
                    let $field = &input;
                    $body
                } else {
                    let $field = &textarea;
                    $body
                }
            };
        }
        let selected = 2..9;
        act(&mut app, window, |window, cx| {
            field!(|field| field.update(cx, |state, cx| {
                state.focus(window, cx);
                state.set_selected_range(selected.clone(), cx);
            }));
        });
        app.advance_clock(Duration::from_millis(100));
        let before = app.capture(window).unwrap();

        // Just above the bottom of the selection's last letter, so the
        // menu opens below nearly all of the field.
        let position = app.update(|cx| {
            let letter = field!(|field| field
                .read(cx)
                .range_to_bounds(&(selected.end - 1..selected.end))
                .unwrap());
            gpui_kit::point(letter.center().x, letter.bottom() - px(2.))
        });
        act(&mut app, window, |window, cx| {
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
        app.advance_clock(Duration::from_millis(100));
        let during = app.capture(window).unwrap();

        assert!(
            before.image != during.image,
            "the {name}'s context menu opens"
        );
        // The menu's shadow reaches a few pixels above its top.
        let above_menu = ((position.y - px(6.)).as_f32() * SCALE) as u32;
        for y in 0..above_menu {
            for x in 0..before.image.width() {
                assert_eq!(
                    before.image.get_pixel(x, y),
                    during.image.get_pixel(x, y),
                    "the {name} looks as it did before its menu opened, at device pixel ({x}, {y})"
                );
            }
        }
        let kept = app.update(|cx| field!(|field| field.read(cx).selected_range()));
        assert_eq!(kept, selected, "the {name} keeps its selection");
    }
}
