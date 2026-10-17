use std::time::Duration;

use gpui::{Rems, rems};
use gpui_base::{Easing, Spring};

/// The most frames a second a repeating loading indicator draws, such as a
/// spinner, a skeleton, an indeterminate progress bar or ring, or shimmering
/// text.
///
/// A loading indicator can show for as long as a request runs, and one that
/// draws every frame keeps the window drawing at the display's full refresh
/// rate all that time. At 30 frames a second the motion stays smooth while
/// the window draws a quarter as often on a 120 Hz display.
pub const LOADING_ANIMATION_MAX_FPS: f32 = 30.;

/// Semantic motion policy shared by styled components.
#[derive(Clone, Debug)]
pub struct MotionTokens {
    pub duration_instant: Duration,
    pub duration_fast: Duration,
    pub duration_normal: Duration,
    pub duration_slow: Duration,
    pub easing_enter: Easing,
    pub easing_exit: Easing,
    pub easing_move: Easing,
    pub spring_control: Spring,
    pub spring_move: Spring,
    pub distance_short: Rems,
    pub distance_medium: Rems,
}

impl Default for MotionTokens {
    fn default() -> Self {
        Self {
            duration_instant: Duration::ZERO,
            duration_fast: Duration::from_millis(120),
            duration_normal: Duration::from_millis(180),
            duration_slow: Duration::from_millis(280),
            easing_enter: Easing::cubic_bezier(0.16, 1.0, 0.3, 1.0)
                .expect("static enter curve is valid"),
            easing_exit: Easing::cubic_bezier(0.4, 0.0, 1.0, 1.0)
                .expect("static exit curve is valid"),
            easing_move: Easing::cubic_bezier(0.2, 0.0, 0.0, 1.0)
                .expect("static move curve is valid"),
            spring_control: Spring::new(Duration::from_millis(180)),
            spring_move: Spring::new(Duration::from_millis(280))
                .with_damping(0.85)
                .with_epsilon(0.1),
            distance_short: rems(0.25),
            distance_medium: rems(0.5),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod loading_frame_rate {
        use std::{cell::Cell, rc::Rc, time::Duration};

        use gpui::{
            AnyElement, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
            Render, Styled as _, TestAppContext, Window, div, px, size,
        };

        use crate::{
            marker::Marker,
            progress::{Progress, ProgressCircle},
            shimmer::ShimmerText,
            skeleton::Skeleton,
            spinner::Spinner,
        };

        struct Loading(fn() -> AnyElement);

        impl Render for Loading {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
                div().size_full().child((self.0)())
            }
        }

        /// A loading indicator that asked for every frame of the display kept
        /// a window drawing at its full refresh rate for as long as it showed.
        #[gpui::test]
        fn loading_indicators_draw_at_a_capped_frame_rate(cx: &mut TestAppContext) {
            cx.update(crate::init);
            let indicators: [(&str, fn() -> AnyElement); 6] = [
                ("Spinner", || Spinner::new().into_any_element()),
                ("Skeleton", || Skeleton::new().into_any_element()),
                ("Progress", || {
                    Progress::new("progress").loading(true).into_any_element()
                }),
                ("ProgressCircle", || {
                    ProgressCircle::new("circle")
                        .loading(true)
                        .into_any_element()
                }),
                ("ShimmerText", || {
                    ShimmerText::new("Thinking…").into_any_element()
                }),
                ("Marker", || Marker::new().loading(true).into_any_element()),
            ];
            for (name, indicator) in indicators {
                let window =
                    cx.open_window(size(px(200.), px(200.)), move |_, _| Loading(indicator));
                cx.run_until_parked();
                let per_frame = window
                    .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                    .unwrap();
                assert_eq!(per_frame, 0, "{name} redraws on every display frame");
            }
        }

        /// Spinners shown one after another, as parallel requests start.
        struct Spinners {
            shown: usize,
            renders: Rc<Cell<usize>>,
        }

        impl Render for Spinners {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
                self.renders.set(self.renders.get() + 1);
                div()
                    .size_full()
                    .children((0..self.shown).map(|ix| div().id(ix).child(Spinner::new())))
            }
        }

        /// How often a window showing `spinners` spinners, each started
        /// `apart` after the one before, draws in a second.
        fn renders_in_a_second(spinners: usize, apart: Duration, cx: &mut TestAppContext) -> usize {
            cx.update(crate::init);
            let renders = Rc::new(Cell::new(0));
            let window = cx.open_window(size(px(200.), px(200.)), {
                let renders = renders.clone();
                move |_, _| Spinners { shown: 1, renders }
            });
            cx.run_until_parked();
            for _ in 1..spinners {
                cx.executor().advance_clock(apart);
                cx.run_until_parked();
                window
                    .update(cx, |spinners, _, cx| {
                        spinners.shown += 1;
                        cx.notify();
                    })
                    .unwrap();
                cx.run_until_parked();
            }
            renders.set(0);
            for _ in 0..1000 {
                cx.executor().advance_clock(Duration::from_millis(1));
                cx.run_until_parked();
            }
            renders.get()
        }

        /// The cap leaves an indicator drawing, about 30 times a second.
        #[gpui::test]
        fn a_loading_indicator_draws_about_thirty_times_a_second(cx: &mut TestAppContext) {
            let renders = renders_in_a_second(1, Duration::ZERO, cx);
            assert!((29..=31).contains(&renders), "{renders} frames in a second");
        }

        /// Indicators started at different times draw in the same frames, so
        /// the window draws 30 frames a second however many it shows.
        #[gpui::test]
        fn loading_indicators_started_apart_draw_in_the_same_frames(cx: &mut TestAppContext) {
            let renders = renders_in_a_second(3, Duration::from_millis(13), cx);
            assert!(
                (29..=31).contains(&renders),
                "{renders} frames in a second for three spinners"
            );
        }
    }

    #[test]
    fn normalized_and_pixel_springs_use_unit_appropriate_tolerances() {
        let motion = MotionTokens::default();

        assert!(motion.spring_control.epsilon() < 0.01);
        assert_eq!(motion.spring_move.epsilon(), 0.1);
    }
}
