//! The heap a long transcript holds after the reader scrolls through it once.
//!
//! A transcript shows each answer as its own `TextView`, in a virtual list. A
//! text view keeps the shaped text of each paragraph for the next frame, so
//! a paragraph laid out again in the same place is not shaped again. This
//! measures what a scroll from the top to the bottom leaves behind, as the
//! live heap after the pass less the heap after the first frame.
//!
//! Run with `cargo bench -p gpui-base --bench text_view_memory`. The numbers
//! are the same in a debug build, which `cargo test -p gpui-base --bench
//! text_view_memory` runs.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    fmt::Write as _,
    sync::atomic::{AtomicIsize, Ordering},
};

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ListAlignment, ListState, ParentElement as _,
    Render, Styled as _, TestApp, Window, div, list, px,
};
use gpui_base::{
    TextSelectionLayer,
    text::{TextView, TextViewState},
};

/// Counts the bytes allocated and not yet freed.
struct LiveBytes;

static LIVE: AtomicIsize = AtomicIsize::new(0);

// SAFETY: every operation is forwarded unchanged to the system allocator. The
// counter has no bearing on allocation validity or lifetime.
unsafe impl GlobalAlloc for LiveBytes {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
        // SAFETY: `layout` is passed through from the caller.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        // SAFETY: `ptr` and `layout` are the pair the caller allocated.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(
            new_size as isize - layout.size() as isize,
            Ordering::Relaxed,
        );
        // SAFETY: all arguments are passed through from the caller.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: LiveBytes = LiveBytes;

fn live() -> isize {
    LIVE.load(Ordering::Relaxed)
}

/// Answers in the transcript.
const ANSWERS: usize = 160;
/// How far each frame of the pass scrolls.
const SCROLL_PER_FRAME: f32 = 240.;

/// An answer of about 4 KB: prose with emphasis, links and inline code, a
/// list, and a code block in every third answer.
fn answer(ix: usize) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "## Answer {ix}: what the data shows\n");
    for paragraph in 0..5 {
        let _ = writeln!(
            out,
            "Paragraph {paragraph} of answer {ix}. The **dip window** is `08-18` → `08-26` \
             (trough $208.48 on 08-24), followed by a post-earnings jump. Let me confirm the \
             earnings date and date the specific news events in that window, then weave in \
             [the filing](https://example.com/filing/{ix}) and the *guidance* update so the \
             numbers line up with what the market priced in before the open.\n"
        );
    }
    let _ = writeln!(out, "- FOMC 09-16: +25bp to 3.75–4.00%, 12-0.");
    let _ = writeln!(out, "- Dot plot: one more hike implied this year.");
    let _ = writeln!(out, "- XLE +45.2% YTD, the standout winner.\n");
    if ix % 3 == 0 {
        let _ = writeln!(
            out,
            "```rust\nfn main() {{\n    let total: u64 = (0..{ix}).sum();\n    println!(\"{{total}}\");\n}}\n```\n"
        );
    }
    out
}

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
    let mut window = app.open_window(|_, cx| Transcript {
        answers: (0..ANSWERS)
            .map(|ix| cx.new(|cx| TextViewState::markdown(&answer(ix), cx)))
            .collect(),
        list: ListState::new(ANSWERS, ListAlignment::Top, px(400.)),
    });
    window.simulate_resize(gpui::size(px(800.), px(900.)));
    app.run_until_parked();
    for _ in 0..3 {
        window.draw();
    }

    let text: usize = (0..ANSWERS).map(|ix| answer(ix).len()).sum();
    let start = live();
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

    let mb = |bytes: isize| bytes as f64 / (1024. * 1024.);
    println!(
        "{ANSWERS} answers, {:.0} KB of Markdown, {frames} frames at {SCROLL_PER_FRAME}px",
        text as f64 / 1024.
    );
    println!("heap after the first frame: {:.1} MB", mb(start));
    println!(
        "heap after one pass:        {:.1} MB ({:+.1} MB)",
        mb(after_pass),
        mb(after_pass - start)
    );
}
