use crate::{Icon, IconName, Sizable, Size};
use gpui::{
    Animation, AnimationExt as _, App, Hsla, IntoElement, PaintEffect, ParentElement, RenderOnce,
    Styled as _, Window, div, linear, percentage, prelude::FluentBuilder as _,
};
use instant::Duration;

/// A cycling loading spinner.
///
/// It turns at a constant speed, in a frame for each frame the display shows,
/// and holds still while the system asks for reduced motion.
#[derive(IntoElement)]
pub struct Spinner {
    size: Size,
    icon: Icon,
    speed: Duration,
    easing: Box<dyn Fn(f32) -> f32>,
    color: Option<Hsla>,
    max_fps: Option<f32>,
}

impl Spinner {
    /// Create a new loading spinner.
    pub fn new() -> Self {
        Self {
            size: Size::Medium,
            speed: Duration::from_secs_f64(0.8),
            easing: Box::new(linear),
            icon: Icon::new(IconName::Loader),
            color: None,
            max_fps: None,
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
    ///
    /// The default is [`linear`], which turns at a constant speed. Any other
    /// curve makes the spinner speed up and slow down once in each turn.
    pub fn ease(mut self, easing: impl Fn(f32) -> f32 + 'static) -> Self {
        self.easing = Box::new(easing);
        self
    }

    /// Draw at most `max_fps` frames a second.
    ///
    /// The default is a frame for each frame the display shows, which is the
    /// smoothest. A lower rate draws on a timer that does not line up with the
    /// display, so some frames stay up longer than others.
    pub fn max_fps(mut self, max_fps: f32) -> Self {
        self.max_fps = Some(max_fps);
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
                    // The icon turns as it paints, so the view around the spinner is not
                    // drawn again for each step, and stays cached. The turn goes by the
                    // app's shared clock, so spinners turn together and a spinner left up
                    // for days keeps its precision.
                    .with_paint_animation(
                        "circle",
                        {
                            let animation = Animation::new(self.speed)
                                .repeat_synced()
                                .with_easing(self.easing);
                            match self.max_fps {
                                Some(max_fps) => animation.with_max_fps(max_fps),
                                None => animation,
                            }
                        },
                        |delta| PaintEffect::rotation(percentage(delta).into()),
                    ),
            )
            .into_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, Render, TestAppContext, px, size};

    struct SpinnerHost;

    impl Render for SpinnerHost {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            Spinner::new()
        }
    }

    /// A window's root that holds a spinner and counts how often it is drawn.
    struct Counted(
        std::rc::Rc<std::cell::Cell<usize>>,
        Box<dyn Fn() -> Spinner>,
    );

    impl Render for Counted {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            self.0.set(self.0.get() + 1);
            (self.1)()
        }
    }

    fn counted_window(
        cx: &mut TestAppContext,
        spinner: impl Fn() -> Spinner + 'static,
    ) -> (
        gpui::WindowHandle<Counted>,
        std::rc::Rc<std::cell::Cell<usize>>,
    ) {
        let renders = std::rc::Rc::new(std::cell::Cell::new(0));
        let window = cx.open_window(size(px(100.), px(100.)), {
            let renders = renders.clone();
            move |_, _| Counted(renders, Box::new(spinner))
        });
        cx.run_until_parked();
        (window, renders)
    }

    /// A spinner that is capped redraws at its rate on a timer, not at every
    /// frame the display offers.
    #[gpui::test]
    fn a_capped_spinner_redraws_at_its_rate(cx: &mut TestAppContext) {
        let (window, renders) = counted_window(cx, || Spinner::new().max_fps(30.));
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

    /// A spinner draws in each frame the display shows, unless it is capped.
    #[gpui::test]
    fn a_spinner_redraws_at_every_frame(cx: &mut TestAppContext) {
        let (window, renders) = counted_window(cx, Spinner::new);
        let before = renders.get();
        for _ in 0..60 {
            cx.executor()
                .advance_clock(std::time::Duration::from_micros(16_667));
            assert!(
                window
                    .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                    .unwrap()
                    > 0,
                "the next frame was not asked for"
            );
            cx.run_until_parked();
        }
        assert_eq!(renders.get() - before, 60);
    }

    /// A pane that holds a spinner, and the values its easing was asked for.
    struct Pane {
        renders: std::rc::Rc<std::cell::Cell<usize>>,
        eased: std::rc::Rc<std::cell::RefCell<Vec<f32>>>,
        max_fps: Option<f32>,
    }

    impl Render for Pane {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let eased = self.eased.clone();
            let spinner = Spinner::new().ease(move |delta| {
                eased.borrow_mut().push(delta);
                delta
            });
            match self.max_fps {
                Some(max_fps) => spinner.max_fps(max_fps),
                None => spinner,
            }
        }
    }

    /// A window whose root keeps the pane, as the panes of an app's window are kept.
    struct PaneWindow {
        pane: gpui::Entity<Pane>,
    }

    impl Render for PaneWindow {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            div().size_full().child(
                self.pane
                    .clone()
                    .cached(gpui::StyleRefinement::default().size_full()),
            )
        }
    }

    /// Turns a spinner in a kept pane for a second, with a frame every 1/60 s and the
    /// executor parked after each. Returns how often the pane was drawn and the values
    /// the spinner's easing was asked for.
    fn turn_in_a_kept_pane(cx: &mut TestAppContext, max_fps: Option<f32>) -> (usize, Vec<f32>) {
        let renders = std::rc::Rc::new(std::cell::Cell::new(0));
        let eased = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let window = cx.open_window(size(px(100.), px(100.)), {
            let (renders, eased) = (renders.clone(), eased.clone());
            move |_, cx| PaneWindow {
                pane: cx.new(|_| Pane {
                    renders,
                    eased,
                    max_fps,
                }),
            }
        });
        cx.run_until_parked();
        assert_eq!(renders.get(), 1);
        eased.borrow_mut().clear();

        for _ in 0..60 {
            cx.executor()
                .advance_clock(std::time::Duration::from_micros(16_667));
            window
                .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                .unwrap();
            cx.run_until_parked();
        }
        window
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        let eased = eased.borrow().clone();
        (renders.get(), eased)
    }

    /// The view a spinner is in is not drawn again for each step of its turn.
    #[gpui::test]
    fn a_spinner_turns_in_a_view_that_is_kept(cx: &mut TestAppContext) {
        let (renders, eased) = turn_in_a_kept_pane(cx, None);
        assert_eq!(renders, 1, "the pane is drawn again");
        assert!(
            (59..=61).contains(&eased.len()),
            "{} steps in a second",
            eased.len()
        );
        // The turn goes round once in 0.8 s and starts again.
        assert!(eased.iter().any(|delta| *delta > 0.9), "{eased:?}");
        assert!(eased.windows(2).any(|pair| pair[1] < pair[0]), "{eased:?}");
    }

    /// A capped spinner steps at its rate in a kept view too.
    #[gpui::test]
    fn a_capped_spinner_turns_in_a_view_that_is_kept(cx: &mut TestAppContext) {
        let (renders, eased) = turn_in_a_kept_pane(cx, Some(30.));
        assert_eq!(renders, 1, "the pane is drawn again");
        assert!(
            (28..=34).contains(&eased.len()),
            "{} steps in a second",
            eased.len()
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
