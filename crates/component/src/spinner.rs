use crate::{Icon, IconName, LOADING_ANIMATION_MAX_FPS, Sizable, Size};
use gpui::{
    Animation, AnimationExt as _, App, Hsla, IntoElement, ParentElement, RenderOnce, Styled as _,
    Transformation, Window, div, ease_in_out, percentage, prelude::FluentBuilder as _,
};
use instant::Duration;

/// A cycling loading spinner.
#[derive(IntoElement)]
pub struct Spinner {
    size: Size,
    icon: Icon,
    speed: Duration,
    easing: Box<dyn Fn(f32) -> f32>,
    color: Option<Hsla>,
}

impl Spinner {
    /// Create a new loading spinner.
    pub fn new() -> Self {
        Self {
            size: Size::Medium,
            speed: Duration::from_secs_f64(0.8),
            easing: Box::new(ease_in_out),
            icon: Icon::new(IconName::Loader),
            color: None,
        }
    }

    /// Set specified icon for the spinner.
    ///
    /// Default is [`IconName::Loader`].
    ///
    /// Please ensure the icon used is suitable for a loading spinner.
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = icon.into();
        self
    }

    /// Set the icon color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Set the easing function.
    pub fn ease(mut self, easing: impl Fn(f32) -> f32 + 'static) -> Self {
        self.easing = Box::new(easing);
        self
    }
}

impl Sizable for Spinner {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .child(
                self.icon
                    .with_size(self.size)
                    .when_some(self.color, |this, color| this.text_color(color))
                    .with_animation(
                        "circle",
                        Animation::new(self.speed)
                            .repeat()
                            .with_easing(self.easing)
                            .with_max_fps(LOADING_ANIMATION_MAX_FPS),
                        |this, delta| this.transform(Transformation::rotate(percentage(delta))),
                    ),
            )
            .into_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Render, TestAppContext, px, size};

    struct SpinnerHost;

    impl Render for SpinnerHost {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            Spinner::new()
        }
    }

    /// A spinner redraws at 30 frames a second on a timer, not at every
    /// frame the display offers.
    #[gpui::test]
    fn a_spinner_redraws_at_thirty_frames_a_second(cx: &mut TestAppContext) {
        let renders = std::rc::Rc::new(std::cell::Cell::new(0));
        struct Counted(std::rc::Rc<std::cell::Cell<usize>>);
        impl Render for Counted {
            fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
                self.0.set(self.0.get() + 1);
                Spinner::new()
            }
        }
        let window = cx.open_window(size(px(100.), px(100.)), {
            let renders = renders.clone();
            move |_, _| Counted(renders)
        });
        cx.run_until_parked();
        assert_eq!(
            window
                .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                .unwrap(),
            0,
            "no redraw is asked of every frame"
        );
        let before = renders.get();
        for _ in 0..30 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(1000 / 30 + 1));
            // A notified test window redraws once the executor parks.
            cx.run_until_parked();
        }
        let redraws = renders.get() - before;
        assert!(
            (25..=31).contains(&redraws),
            "{redraws} redraws in a second"
        );
    }

    #[gpui::test]
    fn reduced_motion_spinner_is_static_and_requests_no_frame(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| SpinnerHost);
        cx.run_until_parked();

        assert_eq!(
            window
                .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                .unwrap(),
            0
        );
    }
}
