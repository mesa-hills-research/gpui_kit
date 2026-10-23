//! Streaming a long answer, about 20,000 tokens, into a chat transcript, a
//! `MessageScroller` that follows its tail. Each iteration appends 16 bytes
//! (about four tokens) to the end of the answer, lets the background parse
//! land and draws the frame that shows them.
//!
//! `whole_answer` shows the answer as one `TextView` row, so every frame lays
//! out every block of it. `block_rows` shows each top-level block as a row of
//! its own, a `TextViewState::block_range` view of the answer, so a frame lays
//! out the rows on screen. `block_rows_fading` does the same with the streamed
//! text fading in, which draws frames until the fade ends.
//!
//! Run with
//! `cargo bench -p mhr-gpui-kit-component --features tree-sitter-languages --bench transcript_stream`.

use std::{cell::RefCell, fmt::Write as _, rc::Rc};

use gpui::{
    AppContext as _, BenchAppContext, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Window, div, px,
};
use gpui_component::{
    message_scroller::{MessageScroller, MessageScrollerState},
    text::{TextView, TextViewState},
};

/// An answer the way a model writes a long one, about 80 KB of sections of
/// prose with emphasis, inline code and links, lists, code blocks and
/// tables.
fn answer() -> String {
    let mut out = String::new();
    let mut section = 0;
    while out.len() < 80 * 1024 {
        let _ = writeln!(out, "## Step {section}: what changes and why\n");
        for paragraph in 0..3 {
            let _ = writeln!(
                out,
                "Paragraph {paragraph} of step {section}. The **request handler** in \
                 `src/server/routes.rs` reads the body before it checks the session, so a \
                 large upload is buffered even when the caller is not signed in. Moving the \
                 check up, as [the review](https://example.com/review/{section}) suggests, \
                 rejects those requests early and keeps memory flat under load.\n"
            );
        }
        let _ = writeln!(out, "- Check the session before reading the body.");
        let _ = writeln!(out, "- Stream uploads to disk past `{section} MB`.");
        let _ = writeln!(out, "- Keep the old path behind a flag for one release.\n");
        if section % 3 == 0 {
            let _ = writeln!(out, "```rust");
            for line in 0..14 {
                let _ = writeln!(
                    out,
                    "    let chunk_{line} = reader.read_chunk({line}).await?; // step {section}"
                );
            }
            let _ = writeln!(out, "```\n");
        }
        if section % 5 == 4 {
            let _ = writeln!(out, "| Case | Before | After |\n|---|---:|---:|");
            for row in 0..4 {
                let _ = writeln!(out, "| upload {row} | {} ms | {} ms |", 40 + row, 12 + row);
            }
            let _ = writeln!(out);
        }
        section += 1;
    }
    out
}

const CHUNK: usize = 16;

struct Transcript {
    scroller: Entity<MessageScrollerState>,
    answer: Entity<TextViewState>,
    /// One view per block of the answer, when the answer is shown as rows.
    rows: Option<Rc<RefCell<Vec<Entity<TextViewState>>>>>,
    stream_fade: bool,
    _sync_rows: Subscription,
}

impl Transcript {
    fn new(markdown: &str, as_rows: bool, stream_fade: bool, cx: &mut Context<Self>) -> Self {
        let answer = cx.new(|cx| TextViewState::markdown(markdown, cx));
        let scroller = cx.new(|cx| MessageScrollerState::new(usize::from(!as_rows), cx));
        let rows = as_rows.then(|| Rc::new(RefCell::new(Vec::new())));
        let sync_rows = cx.observe(&answer, |this, answer, cx| this.sync_rows(&answer, cx));
        Self {
            scroller,
            answer,
            rows,
            stream_fade,
            _sync_rows: sync_rows,
        }
    }

    /// Adds a row for each block the answer gained, as a transcript does.
    fn sync_rows(&mut self, answer: &Entity<TextViewState>, cx: &mut Context<Self>) {
        let Some(rows) = &self.rows else {
            return;
        };
        let count = answer.read(cx).blocks().len();
        let added = {
            let mut rows = rows.borrow_mut();
            let old = rows.len();
            for ix in old..count {
                rows.push(cx.new(|cx| TextViewState::block_range(answer, ix..ix + 1, cx)));
            }
            count.saturating_sub(old)
        };
        if added > 0 {
            self.scroller
                .update(cx, |scroller, cx| scroller.append(added, cx));
        }
    }
}

impl Render for Transcript {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let answer = self.answer.clone();
        let rows = self.rows.clone();
        let stream_fade = self.stream_fade;
        div().w(px(760.)).h(px(800.)).child(
            MessageScroller::new("transcript", self.scroller.clone(), move |ix, _, _| {
                let view = match &rows {
                    Some(rows) => rows.borrow()[ix].clone(),
                    None => answer.clone(),
                };
                div()
                    .w_full()
                    .px(px(16.))
                    .child(TextView::new(&view).stream_fade(stream_fade))
            })
            .size_full(),
        )
    }
}

/// Streams the end of the answer into a transcript showing it whole or as
/// rows.
fn stream_answer(as_rows: bool, stream_fade: bool, cx: &mut BenchAppContext) {
    cx.update(gpui_component::init);
    let source = answer();
    let mut end = source.len() - 4 * 1024;
    while !source.is_char_boundary(end) {
        end += 1;
    }
    let shown = source[..end].to_string();
    let mut window = cx.add_empty_window();
    let transcript = window.update(|window, cx| {
        window.replace_root(cx, |_, cx| {
            Transcript::new(&shown, as_rows, stream_fade, cx)
        })
    });
    let answer = cx.read(|cx| transcript.read(cx).answer.clone());
    cx.run_until_idle();

    cx.bench_iter(move |cx| {
        let mut next = (end + CHUNK).min(source.len());
        while !source.is_char_boundary(next) {
            next += 1;
        }
        if next == end {
            // The answer is all in, and an iteration past it measures nothing.
            // The stream is set up again for every sample, and a sample's
            // iterations rarely reach the 256 chunks of the last 4 KB.
            return;
        }
        let chunk = source[end..next].to_string();
        end = next;
        cx.update(|cx| answer.update(cx, |answer, cx| answer.push_str(&chunk, cx)));
        cx.run_until_idle();
    });
}

#[gpui::bench]
fn whole_answer(cx: &mut BenchAppContext) {
    stream_answer(false, false, cx);
}

#[gpui::bench]
fn block_rows(cx: &mut BenchAppContext) {
    stream_answer(true, false, cx);
}

#[gpui::bench]
fn block_rows_fading(cx: &mut BenchAppContext) {
    stream_answer(true, true, cx);
}

gpui::bench_group!(benches, whole_answer, block_rows, block_rows_fading);
gpui::bench_main!(benches);
