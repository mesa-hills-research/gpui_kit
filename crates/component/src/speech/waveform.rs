use gpui::{
    Animation, AnimationExt as _, App, Bounds, ContentMask, ElementId, Entity, EntityId,
    IntoElement, PaintEffect, ParentElement as _, Path, PathBuilder, Pixels, RenderOnce,
    StyleRefinement, Styled, Window, canvas, div, point, px, size,
};

use crate::{ActiveTheme as _, Sizable, Size, StyledExt as _};

use super::{SpeechState, level::PLAYHEAD_MAX_LEAD};

/// The width of a waveform the caller does not size.
const DEFAULT_WIDTH: Pixels = px(96.);

/// The space between bars, relative to their width.
const GAP_RATIO: f32 = 2.5;

/// A live waveform of a [`SpeechState`]'s input levels.
///
/// Each level is a bar that grows up and down from the midline, and silence is
/// a dot. The newest level enters at the trailing edge and older ones scroll
/// toward the leading edge at a steady pace while audio is captured, so the
/// trail grows from the trailing edge until it fills the width. Before any
/// audio, nothing is drawn. With reduced motion the bars still show each level
/// but do not scroll between them.
///
/// The bars scroll as they paint, so the view the waveform is in draws once
/// per level (every 80 ms) rather than at every frame. A cached view that holds
/// it has to observe the [`SpeechState`] to show new levels.
///
/// Size the waveform like any element, e.g. `.w_full()` to span a row. It is
/// 96 px wide by default, and its height follows [`Sizable`].
#[derive(IntoElement)]
pub struct SpeechWaveform {
    state: Entity<SpeechState>,
    size: Size,
    style: StyleRefinement,
}

impl SpeechWaveform {
    /// A waveform for `state`.
    pub fn new(state: &Entity<SpeechState>) -> Self {
        Self {
            state: state.clone(),
            size: Size::default(),
            style: StyleRefinement::default(),
        }
    }

    fn height(&self) -> Pixels {
        match self.size {
            Size::Size(height) => height,
            Size::XSmall => px(12.),
            Size::Small => px(16.),
            Size::Medium => px(20.),
            Size::Large => px(24.),
        }
    }
}

impl Sizable for SpeechWaveform {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl Styled for SpeechWaveform {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SpeechWaveform {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let height = self.height();
        // Bars thicken with the waveform so a tall one does not read as hairlines,
        // and stand apart so a run of silence reads as a row of dots.
        let bar = (height * 0.125).round().clamp(px(2.), px(4.));
        let gap = (bar * GAP_RATIO).round();
        let state_id = self.state.entity_id();
        let state = self.state.read(cx);
        let capturing = state.status().is_capturing();
        let levels: Vec<f32> = state.levels().collect();
        // The bars follow an even clock rather than the arrival of each level,
        // which comes in uneven bursts. The newest bar slides in from just past
        // the trailing edge, so the waveform scrolls without stepping or
        // stalling. Without motion, and once no level has arrived for long
        // enough that the playhead rests, the newest bar sits at the edge.
        let now = cx.background_executor().now();
        let motion = state
            .session_id()
            .zip(state.playhead())
            .filter(|(_, playhead)| {
                capturing
                    && !cx.reduce_motion()
                    && playhead.lead_at(now) > 0.
                    && !playhead.rest_after_arrival().is_zero()
            });
        // Bars that scroll in from past the leading edge are painted too, cut off
        // until they come into view.
        let lead = if motion.is_some() {
            PLAYHEAD_MAX_LEAD
        } else {
            0.
        };
        let color = if capturing {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        };

        let bars = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                // A bar sliding in is cut at the trailing edge, not drawn past it.
                // Bars are anti-aliased paths, not quads, because quads snap to
                // whole device pixels, so a bar moving a fraction of a pixel per
                // frame would step instead of glide.
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    for rect in bar_rects(bounds, &levels, lead, bar, gap) {
                        if let Some(path) = capsule(rect) {
                            window.paint_path(path, color);
                        }
                    }
                });
            },
        )
        .size_full();

        div()
            .h(height)
            .w(DEFAULT_WIDTH)
            .flex_shrink_0()
            .refine_style(&self.style)
            .child(match motion {
                // The bars move as they paint, so the views around the waveform are
                // not drawn again for each frame. The content mask doesn't move, so
                // the bars are still cut at the waveform's edges.
                Some((session, playhead)) => {
                    let clock = cx.background_executor().clone();
                    let pitch = bar + gap;
                    let run = MotionRun {
                        state: state_id,
                        session,
                        level: playhead.recorded(),
                    };
                    bars.with_paint_animation(
                        // Each level starts a run of its own, as long as the playhead
                        // takes to come to rest after it, so frames stop when levels
                        // do. A run that starts after the level arrived ends as much
                        // later, with the bars already at rest. The effect reads the
                        // clock itself.
                        run,
                        Animation::new(playhead.rest_after_arrival()),
                        move |progress| {
                            // The last frame lands at rest, however the clocks line up.
                            let lead = if progress < 1. {
                                playhead.lead_at(clock.now())
                            } else {
                                0.
                            };
                            PaintEffect::translation(point(pitch * lead, px(0.)))
                        },
                    )
                    .into_any_element()
                }
                None => bars.into_any_element(),
            })
    }
}

/// One run of the bars' motion, from a level's arrival until the bars rest.
/// The window keeps a run's start under its element id, so the id names the
/// state, its session and the level. Waveforms of different states, and the
/// sessions of one state, never share a start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MotionRun {
    state: EntityId,
    session: usize,
    level: u64,
}

impl From<MotionRun> for ElementId {
    fn from(run: MotionRun) -> Self {
        let state = ElementId::from(("speech-waveform", run.state));
        ElementId::from((state, format!("{}.{}", run.session, run.level)))
    }
}

/// A capsule filling `rect`, with round caps on its short ends, or a dot when
/// square.
fn capsule(rect: Bounds<Pixels>) -> Option<Path<Pixels>> {
    let radius = rect.size.width.min(rect.size.height) / 2.;
    let radii = point(radius, radius);
    let (left, right) = (rect.left(), rect.right());
    let (top, bottom) = (rect.top() + radius, rect.bottom() - radius);
    let mut path = PathBuilder::fill();
    path.move_to(point(left, top));
    path.arc_to(radii, px(0.), false, true, point(right, top));
    path.line_to(point(right, bottom));
    path.arc_to(radii, px(0.), false, true, point(left, bottom));
    path.close();
    path.build().ok()
}

/// The bars for `levels` (oldest first) in `bounds`, from the newest, which
/// sits at the trailing edge, to older ones a step apart toward the leading
/// edge. Each is centered on the midline and at least as tall as it is wide. A
/// bar past the leading edge is left out unless moving the bars up to `lead`
/// steps toward the trailing edge brings it into view. Only levels that exist
/// are drawn, so a short history leaves the leading part empty.
fn bar_rects(
    bounds: Bounds<Pixels>,
    levels: &[f32],
    lead: f32,
    bar: Pixels,
    gap: Pixels,
) -> Vec<Bounds<Pixels>> {
    let pitch = bar + gap;
    let height = bounds.size.height;
    let middle = bounds.origin.y + height / 2.;
    let leading = bounds.left() - px(0.5) - pitch * lead;
    levels
        .iter()
        .rev()
        .enumerate()
        .map_while(|(from_end, level)| {
            let left = bounds.right() - bar - pitch * from_end as f32;
            (left >= leading).then(|| {
                let bar_height = (height * level.clamp(0., 1.)).max(bar);
                Bounds::new(point(left, middle - bar_height / 2.), size(bar, bar_height))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(10.), px(20.)), size(px(width), px(height)))
    }

    #[test]
    fn a_short_history_grows_from_the_trailing_edge() {
        let bounds = bounds(98., 20.);
        let rects = bar_rects(bounds, &[0.2, 0.9], 0., px(2.), px(5.));
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].right(), bounds.right());
        assert_eq!(rects[0].size.height, px(18.));
        assert_eq!(rects[1].right(), bounds.right() - px(7.));
        assert_eq!(rects[1].size.height, px(4.));
    }

    #[test]
    fn a_long_history_fills_the_width() {
        let bounds = bounds(96., 20.);
        let rects = bar_rects(bounds, &[0.; 64], 0., px(2.), px(5.));
        // 96 px at a 7 px pitch fits 14 bars, and the oldest sit off the leading edge.
        assert_eq!(rects.len(), 14);
        assert!(
            rects
                .iter()
                .all(|rect| rect.left() >= bounds.left() - px(0.5))
        );
    }

    #[test]
    fn nothing_is_drawn_before_any_audio() {
        assert!(bar_rects(bounds(96., 20.), &[], 0., px(2.), px(5.)).is_empty());
    }

    #[test]
    fn bars_grow_from_the_midline_and_silence_is_a_dot() {
        let bounds = bounds(40., 20.);
        for rect in bar_rects(bounds, &[0.5, 0.], 0., px(2.), px(5.)) {
            let middle = rect.origin.y + rect.size.height / 2.;
            assert_eq!(middle, px(30.));
            assert!(rect.size.height >= px(2.));
        }
    }

    #[test]
    fn bars_that_scroll_in_from_the_leading_edge_are_drawn_ahead() {
        let bounds = bounds(96., 20.);
        let still = bar_rects(bounds, &[1.; 64], 0., px(2.), px(5.));
        let moving = bar_rects(bounds, &[1.; 64], 3., px(2.), px(5.));
        // Three steps of scrolling bring three more bars in, in the same places.
        assert_eq!(moving.len(), still.len() + 3);
        assert_eq!(moving[..still.len()], still[..]);
        assert!(moving[still.len()].left() < bounds.left());
    }

    mod motion {
        use std::{
            cell::{Cell, RefCell},
            rc::Rc,
            time::Duration,
        };

        use gpui::{
            AppContext as _, Context, Render, StyleRefinement, Subscription, TestAppContext,
            WindowHandle,
        };

        use super::*;
        use crate::speech::{
            AudioFormat, AudioInput, AudioSink, RecognitionSession, SpeechError, SpeechRecognizer,
            SpeechSink,
        };

        struct Recognizer;

        struct Session;

        impl SpeechRecognizer for Recognizer {
            fn start(
                &self,
                _: SpeechSink,
                _: &mut App,
            ) -> Result<Box<dyn RecognitionSession>, SpeechError> {
                Ok(Box::new(Session))
            }
        }

        impl RecognitionSession for Session {
            fn push_audio(&mut self, _: &[i16], _: &mut App) {}

            fn finish(&mut self, _: &mut App) {}
        }

        #[derive(Clone, Default)]
        struct Input(Rc<RefCell<Option<AudioSink>>>);

        impl AudioInput for Input {
            fn start(
                &self,
                _: AudioFormat,
                sink: AudioSink,
                _: &mut App,
            ) -> Result<Subscription, SpeechError> {
                *self.0.borrow_mut() = Some(sink);
                Ok(Subscription::new(|| {}))
            }
        }

        /// A pane that shows a waveform, draws again when its state notifies, as
        /// a cached view has to, and counts its draws.
        struct Pane {
            speech: Entity<SpeechState>,
            renders: Rc<Cell<usize>>,
            _observe: Subscription,
        }

        impl Render for Pane {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                self.renders.set(self.renders.get() + 1);
                SpeechWaveform::new(&self.speech)
            }
        }

        /// A window whose root keeps the pane, as an app keeps its panes.
        struct PaneWindow {
            pane: Entity<Pane>,
        }

        impl Render for PaneWindow {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().size_full().child(
                    self.pane
                        .clone()
                        .cached(StyleRefinement::default().size_full()),
                )
            }
        }

        /// A window whose cached pane shows the waveform of a state that is
        /// listening and has had one level. Returns the window, the state's audio
        /// sink and the pane's draw count.
        fn listening(
            cx: &mut TestAppContext,
        ) -> (WindowHandle<PaneWindow>, AudioSink, Rc<Cell<usize>>) {
            cx.update(crate::theme::init);
            let input = Input::default();
            let speech = cx.update(|cx| {
                cx.new(|cx| {
                    SpeechState::new(cx)
                        .recognizer(Recognizer)
                        .input(input.clone())
                })
            });
            let renders = Rc::new(Cell::new(0));
            let window = cx.open_window(gpui::size(px(200.), px(40.)), {
                let (speech, renders) = (speech.clone(), renders.clone());
                move |_, cx| PaneWindow {
                    pane: cx.new(|cx| Pane {
                        _observe: cx.observe(&speech, |_, _, cx| cx.notify()),
                        speech,
                        renders,
                    }),
                }
            });
            speech.update(cx, |speech, cx| speech.start(cx));
            let audio = input.0.borrow().clone().unwrap();
            push_level(&audio, cx);
            (window, audio, renders)
        }

        /// Pushes 80 ms of 16 kHz mono, one level, and parks the executor.
        fn push_level(audio: &AudioSink, cx: &mut TestAppContext) {
            cx.update(|cx| audio.push(vec![8_000; 1_280], cx));
            cx.run_until_parked();
        }

        /// Shows the next frame of a 60 Hz display and parks the executor.
        /// Returns whether the frame before asked for it.
        fn next_frame(window: WindowHandle<PaneWindow>, cx: &mut TestAppContext) -> bool {
            cx.executor().advance_clock(Duration::from_micros(16_667));
            let callbacks = window
                .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                .unwrap();
            cx.run_until_parked();
            callbacks > 0
        }

        /// Pushes a level every 5 frames (12 levels) over 60 frames. Returns how
        /// often the pane drew and how many frames were asked for.
        fn listen_for_a_second(
            window: WindowHandle<PaneWindow>,
            audio: &AudioSink,
            renders: &Cell<usize>,
            cx: &mut TestAppContext,
        ) -> (usize, usize) {
            let before = renders.get();
            let mut asked = 0;
            for frame in 0..60 {
                if frame % 5 == 0 {
                    push_level(audio, cx);
                }
                asked += usize::from(next_frame(window, cx));
            }
            (renders.get() - before, asked)
        }

        /// The waveform scrolls at every frame without drawing the view it is in,
        /// which draws once per level, with motion or without.
        #[gpui::test]
        fn a_listening_waveform_draws_its_view_once_per_level(cx: &mut TestAppContext) {
            let (window, audio, renders) = listening(cx);

            let (drawn, asked) = listen_for_a_second(window, &audio, &renders, cx);
            assert!(drawn <= 12, "the pane drew {drawn} times for 12 levels");
            assert_eq!(asked, 60, "a moving waveform asks for every frame");

            cx.update(|cx| cx.set_reduce_motion(true));
            // The frame the moving waveform asked for last.
            next_frame(window, cx);
            let (drawn, asked) = listen_for_a_second(window, &audio, &renders, cx);
            assert!(drawn <= 12, "the pane drew {drawn} times for 12 levels");
            assert_eq!(asked, 0, "a still waveform asked for frames");
        }

        /// A waveform whose input stalls stops asking for frames once its bars
        /// come to rest, and moves again with the next level.
        #[gpui::test]
        fn a_stalled_waveform_comes_to_rest_and_moves_again(cx: &mut TestAppContext) {
            let (window, audio, _) = listening(cx);
            // Half a second with no audio after the first level, whose bar comes to
            // rest 140 ms after it arrived.
            let asked: Vec<bool> = (0..30).map(|_| next_frame(window, cx)).collect();
            assert!(asked[..5].iter().all(|asked| *asked), "{asked:?}");
            assert!(asked[15..].iter().all(|asked| !asked), "{asked:?}");

            push_level(&audio, cx);
            let asked: Vec<bool> = (0..3).map(|_| next_frame(window, cx)).collect();
            assert!(asked.iter().all(|asked| *asked), "{asked:?}");
        }
    }
}
