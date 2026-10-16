//! A long fenced code block in a Markdown `TextView`.
//!
//! The streaming benchmarks append 40 bytes per iteration, the way a model
//! streams an answer, let the background Markdown parse land and draw the
//! frame that shows them, so the window draw times are the UI-thread cost of
//! a chunk: highlighting it, laying it out and painting it. The redraw
//! benchmark draws the finished block again, as every frame of a window
//! that shows it does.
//!
//! Run with
//! `cargo bench -p gpui-component --features tree-sitter-rust --bench code_block_stream`.

use gpui::{
    AppContext as _, BenchAppContext, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Window, div, px,
};
use gpui_component::text::{TextView, TextViewState};

/// About 450 KB of Rust from this repository.
fn rust_source() -> String {
    [
        include_str!("../../base/src/text/node.rs"),
        include_str!("../../base/src/text/state.rs"),
        include_str!("../../base/src/text/text_view.rs"),
    ]
    .concat()
}

const CHUNK: usize = 40;

struct Answer {
    state: Entity<TextViewState>,
}

impl Render for Answer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .px(px(20.))
            .child(TextView::new(&self.state))
    }
}

/// Streams a fenced Rust block that starts at `size_kb` KB.
fn stream_rust_block(size_kb: usize, cx: &mut BenchAppContext) {
    cx.update(gpui_component::init);
    let source = rust_source();
    let mut end = size_kb * 1024;
    while !source.is_char_boundary(end) {
        end += 1;
    }
    let markdown = format!("Here is the file:\n\n```rust\n{}", &source[..end]);
    let mut window = cx.add_empty_window();
    let answer = window.update(|window, cx| {
        window.replace_root(cx, |_, cx| Answer {
            state: cx.new(|cx| TextViewState::markdown(&markdown, cx)),
        })
    });
    let state = cx.read(|cx| answer.read(cx).state.clone());
    // The first parse and highlight are not part of the stream.
    cx.run_until_idle();

    cx.bench_iter(move |cx| {
        let mut next = (end + CHUNK).min(source.len());
        while !source.is_char_boundary(next) {
            next += 1;
        }
        let chunk = source[end..next].to_string();
        end = next;
        cx.update(|cx| state.update(cx, |state, cx| state.push_str(&chunk, cx)));
        cx.run_until_idle();
    });
}

/// Draws a finished 200 KB fenced Rust block again every iteration.
#[gpui::bench]
fn redraw_rust_block_200kb(cx: &mut BenchAppContext) {
    cx.update(gpui_component::init);
    let source = rust_source();
    let mut end = 200 * 1024;
    while !source.is_char_boundary(end) {
        end += 1;
    }
    let markdown = format!("Here is the file:\n\n```rust\n{}\n```\n", &source[..end]);
    let mut window = cx.add_empty_window();
    let answer = window.update(|window, cx| {
        window.replace_root(cx, |_, cx| Answer {
            state: cx.new(|cx| TextViewState::markdown(&markdown, cx)),
        })
    });
    cx.run_until_idle();

    cx.bench_renderer(answer, |_, _, cx| cx.notify());
}

#[gpui::bench]
fn stream_rust_block_50kb(cx: &mut BenchAppContext) {
    stream_rust_block(50, cx);
}

#[gpui::bench]
fn stream_rust_block_200kb(cx: &mut BenchAppContext) {
    stream_rust_block(200, cx);
}

gpui::bench_group!(
    benches,
    redraw_rust_block_200kb,
    stream_rust_block_50kb,
    stream_rust_block_200kb
);
gpui::bench_main!(benches);
