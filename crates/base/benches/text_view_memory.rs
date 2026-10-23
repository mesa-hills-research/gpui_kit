//! The heap a long transcript holds, and the time it takes to parse and to
//! scroll through.
//!
//! A transcript shows each answer as its own `TextView`, in a virtual list.
//! This prints three things.
//!
//! - The heap the parsed answers take before anything is drawn, with the
//!   time to parse them, which is what a transcript holds for every answer
//!   it has ever scrolled past.
//! - The heap after the first frame.
//! - What a scroll from the top to the bottom leaves behind, as the live heap
//!   after the pass less the heap after the first frame, with the time the
//!   pass takes. A text view keeps the shaped text of each paragraph for the
//!   next frame, so a paragraph laid out again in the same place is not
//!   shaped again.
//!
//! Run with `cargo bench -p mhr-gpui-kit-base --bench text_view_memory`. The heap
//! numbers are the same in a debug build, which `cargo test -p mhr-gpui-kit-base
//! --bench text_view_memory` runs. The times are not.

use std::time::Instant;

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ListAlignment, ListState, ParentElement as _,
    Render, Styled as _, TestApp, Window, div, list, px,
};
use gpui_base::{
    TextSelectionLayer,
    text::{TextView, TextViewState},
};

mod support;
use support::{answer, live};

/// Answers in the transcript.
const ANSWERS: usize = 160;
/// How far each frame of the pass scrolls.
const SCROLL_PER_FRAME: f32 = 240.;

struct Transcript {
    answers: Vec<Entity<TextViewState>>,
    list: ListState,
}

impl Render for Transcript {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let answers = self.answers.clone();
        // A kit window draws the selection layer over its content.
        div().size_full().child(TextSelectionLayer).child(
            list(self.list.clone(), move |ix, _, _| {
                div()
                    .w_full()
                    .px(px(20.))
                    .pb(px(16.))
                    .child(TextView::new(&answers[ix]))
                    .into_any_element()
            })
            .size_full(),
        )
    }
}

fn main() {
    let mut app = TestApp::with_text_system(gpui_platform::bench_text_system());
    app.update(gpui_base::init);

    // The parsed state of every answer, before anything is drawn.
    let source_bytes: usize = (0..ANSWERS).map(|ix| answer(ix).len()).sum();
    let before_states = live();
    let parse_started = Instant::now();
    let answers = app.update(|cx| {
        (0..ANSWERS)
            .map(|ix| cx.new(|cx| TextViewState::markdown(&answer(ix), cx)))
            .collect::<Vec<Entity<TextViewState>>>()
    });
    app.run_until_parked();
    let parse_time = parse_started.elapsed();
    let parsed = live() - before_states;

    let mut window = app.open_window(move |_, _| Transcript {
        answers,
        list: ListState::new(ANSWERS, ListAlignment::Top, px(400.)),
    });
    window.simulate_resize(gpui::size(px(800.), px(900.)));
    app.run_until_parked();
    for _ in 0..3 {
        window.draw();
    }

    let text = source_bytes;
    let start = live();
    let pass_started = Instant::now();
    let mut frames = 0;
    loop {
        let at_end = window.update(|transcript, _, _| {
            transcript.list.scroll_by(px(SCROLL_PER_FRAME));
            transcript.list.is_scrolled_to_end().unwrap_or(false)
        });
        window.draw();
        frames += 1;
        if at_end || frames > 10_000 {
            break;
        }
    }
    let after_pass = live();
    let pass_time = pass_started.elapsed();

    let mb = |bytes: isize| bytes as f64 / (1024. * 1024.);
    println!(
        "{ANSWERS} answers, {:.0} KB of Markdown, {frames} frames at {SCROLL_PER_FRAME}px",
        text as f64 / 1024.
    );
    println!(
        "parsed state, nothing drawn: {:.1} MB ({:.0} bytes per source byte)",
        mb(parsed),
        parsed as f64 / source_bytes as f64
    );
    println!(
        "parse time:                 {:.1} ms",
        parse_time.as_secs_f64() * 1000.
    );
    println!("heap after the first frame: {:.1} MB", mb(start));
    println!(
        "heap after one pass:        {:.1} MB ({:+.1} MB)",
        mb(after_pass),
        mb(after_pass - start)
    );
    println!(
        "pass time:                  {:.1} ms ({:.2} ms a frame)",
        pass_time.as_secs_f64() * 1000.,
        pass_time.as_secs_f64() * 1000. / frames as f64
    );
}
