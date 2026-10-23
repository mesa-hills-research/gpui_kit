//! A long paragraph streamed into a text view, as a model writes an answer
//! without paragraph breaks. Each iteration appends 16 bytes to the end of a
//! 20 KB or 80 KB paragraph, lets the background parse land and draws the
//! frame that shows them, so the window draw times are what a frame of the
//! stream costs. The paragraph is plain text, or has bold words and inline
//! code in every sentence.
//!
//! Run with `cargo bench -p mhr-gpui-kit-component --bench long_paragraph`.

use gpui::{
    AppContext as _, BenchAppContext, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Window, div, px,
};
use gpui_component::text::{TextView, TextViewState};

/// A paragraph of at least `kb` KB and 4 KB more to stream.
fn paragraph(kb: usize, markup: bool) -> String {
    let mut text = String::new();
    let mut sentence = 0;
    while text.len() < (kb + 4) * 1024 {
        if markup {
            text.push_str(&format!(
                "Sentence {sentence} has **bold words** and `code {sentence}` in it. "
            ));
        } else {
            text.push_str(&format!(
                "Sentence {sentence} has plain words and some more words in it. "
            ));
        }
        sentence += 1;
    }
    text
}

const CHUNK: usize = 16;

struct Answer {
    state: Entity<TextViewState>,
}

impl Render for Answer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(760.))
            .px(px(16.))
            .child(TextView::new(&self.state))
    }
}

/// Streams the last 4 KB of a paragraph of `kb` KB.
fn stream_paragraph(kb: usize, markup: bool, cx: &mut BenchAppContext) {
    cx.update(gpui_component::init);
    let source = paragraph(kb, markup);
    let mut end = kb * 1024;
    let mut window = cx.add_empty_window();
    let answer = window.update(|window, cx| {
        window.replace_root(cx, |_, cx| Answer {
            state: cx.new(|cx| TextViewState::markdown(&source[..end], cx)),
        })
    });
    let state = cx.read(|cx| answer.read(cx).state.clone());
    cx.run_until_idle();

    cx.bench_iter(move |cx| {
        let next = (end + CHUNK).min(source.len());
        if next == end {
            // The paragraph is all in, and an iteration past it measures
            // nothing. The stream is set up again for every sample.
            return;
        }
        let chunk = source[end..next].to_string();
        end = next;
        cx.update(|cx| state.update(cx, |state, cx| state.push_str(&chunk, cx)));
        cx.run_until_idle();
    });
}

#[gpui::bench]
fn paragraph_20kb_with_markup(cx: &mut BenchAppContext) {
    stream_paragraph(20, true, cx);
}

#[gpui::bench]
fn paragraph_80kb_with_markup(cx: &mut BenchAppContext) {
    stream_paragraph(80, true, cx);
}

#[gpui::bench]
fn paragraph_20kb_plain(cx: &mut BenchAppContext) {
    stream_paragraph(20, false, cx);
}

#[gpui::bench]
fn paragraph_80kb_plain(cx: &mut BenchAppContext) {
    stream_paragraph(80, false, cx);
}

gpui::bench_group!(
    benches,
    paragraph_20kb_with_markup,
    paragraph_80kb_with_markup,
    paragraph_20kb_plain,
    paragraph_80kb_plain
);
gpui::bench_main!(benches);
