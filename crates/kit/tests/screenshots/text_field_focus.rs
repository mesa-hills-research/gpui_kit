use super::*;
use gpui_kit::component::{
    input::{InputGroup, InputGroupInput, NumberInput, OtpInput, OtpState},
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
