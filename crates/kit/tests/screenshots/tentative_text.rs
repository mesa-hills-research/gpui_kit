use super::*;

/// Words dictated into a composer, the last of them tentative.
const SETTLED: &str = "Draft the release notes";
const TENTATIVE: &str = " and send them to the team";
/// The tentative words that settle.
const COMMITTED: &str = " and send";

struct Composer {
    textarea: Entity<TextareaState>,
}

impl Render for Composer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page(cx).child(Textarea::new(&self.textarea))
    }
}

/// A composer holding `settled` with the caret after it and `tentative` showing there.
fn composer(
    app: &mut ScreenshotApp,
    settled: &'static str,
    tentative: &'static str,
) -> (AnyWindowHandle, Entity<TextareaState>) {
    let opened = Rc::new(RefCell::new(None));
    let window = open(app, (300., 110.), SCALE, {
        let opened = opened.clone();
        move |window, cx| {
            let textarea = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .rows(2)
                    .default_value(settled)
            });
            textarea.update(cx, |textarea, cx| {
                textarea.focus(window, cx);
                textarea.set_selected_range(settled.len()..settled.len(), cx);
                textarea.set_tentative_text(tentative, cx);
            });
            *opened.borrow_mut() = Some(textarea.clone());
            Composer { textarea }
        }
    });
    let textarea = opened.borrow_mut().take().unwrap();
    app.capture(window).unwrap();
    (window, textarea)
}

/// Tentative words after the caret, in the muted colour with the accent's hairline under them on
/// each row they wrap to.
#[test]
fn tentative_text() {
    for (mode, name) in [
        (ThemeMode::Light, "tentative-text"),
        (ThemeMode::Dark, "tentative-text-dark"),
    ] {
        let mut app = app(mode);
        app.update(|cx| cx.set_reduce_motion(true));
        let (window, _) = composer(&mut app, SETTLED, TENTATIVE);
        app.advance_clock(Duration::from_millis(100));
        goldens().assert(name, &app.capture(window).unwrap());
    }
}

/// The muted colour and the accent keep their contrast on the background in every theme, 4.5:1
/// for the tentative words and 3:1 for the hairline.
#[test]
fn tentative_text_keeps_its_contrast_in_every_theme() {
    use gpui_kit::component::ThemeRegistry;

    let mut app = app(ThemeMode::Light);
    app.update(|cx| {
        let themes = concat!(env!("CARGO_MANIFEST_DIR"), "/../../themes");
        for entry in std::fs::read_dir(themes).unwrap() {
            let content = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            ThemeRegistry::global_mut(cx)
                .load_themes_from_str(&content)
                .unwrap();
        }
        let configs: Vec<_> = ThemeRegistry::global(cx)
            .sorted_themes()
            .into_iter()
            .cloned()
            .collect();
        assert!(configs.len() > 40, "{} themes", configs.len());
        for config in configs {
            Theme::update(cx, |theme| theme.apply_config(&config));
            let theme = cx.theme();
            let muted = contrast_ratio(theme.muted_foreground, theme.background);
            let accent = contrast_ratio(theme.primary, theme.background);
            assert!(muted >= 4.5, "{} muted {muted:.2}", config.name);
            assert!(accent >= 3.0, "{} accent {accent:.2}", config.name);
        }
    });
}

/// " and send" settles. Its colour fills in from the left on the smooth caret's glide, 0, 30, 60
/// and 120 ms in, pushing the hairline's left end along with the caret on its edge. Nothing
/// else moves, and once the fill is done the composer is drawn as one that had the words typed.
#[test]
fn tentative_text_fills_in_as_it_settles() {
    let mut app = app(ThemeMode::Light);
    app.update(|cx| cx.set_reduce_motion(false));
    let (window, textarea) = composer(&mut app, SETTLED, TENTATIVE);
    // The hairline has drawn in under the words.
    app.advance_clock(Duration::from_millis(200));
    act(&mut app, window, |window, cx| {
        textarea.update(cx, |textarea, cx| {
            assert!(textarea.commit_tentative_text(COMMITTED.len(), window, cx));
        });
    });
    let mut elapsed = 0;
    let mut last = None;
    for ms in [0, 30, 60, 120] {
        app.advance_clock(Duration::from_millis(ms - elapsed));
        elapsed = ms;
        let shot = app.capture(window).unwrap();
        goldens().assert(&format!("tentative-fill-{ms}ms"), &shot);
        last = Some(shot.image);
    }

    let mut typed = super::app(ThemeMode::Light);
    typed.update(|cx| cx.set_reduce_motion(false));
    let settled: &'static str = Box::leak(format!("{SETTLED}{COMMITTED}").into_boxed_str());
    let (window, _) = composer(&mut typed, settled, &TENTATIVE[COMMITTED.len()..]);
    typed.advance_clock(Duration::from_millis(120));
    assert!(
        last.unwrap() == typed.capture(window).unwrap().image,
        "a settled fill is drawn as typed text"
    );
}
