//! A long fenced code block in a Markdown `TextView`.
//!
//! The streaming benchmarks append 40 bytes per iteration, the way a model
//! streams an answer, let the background Markdown parse land and draw the
//! frame that shows them, so the window draw times are the UI-thread cost of
//! highlighting, laying out and painting a chunk. The redraw
//! benchmark draws the finished block again, as every frame of a window
//! that shows it does.
//!
//! The code, Rust items with nested bodies and flat Python whose root holds a
//! node for every line, is generated the same way every time, so the numbers
//! can be compared across commits.
//!
//! Run with `cargo bench -p mhr-gpui-kit-component --features
//! tree-sitter-rust,tree-sitter-python --bench code_block_stream`.

use gpui::{
    AppContext as _, BenchAppContext, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::text::{TextView, TextViewState};

/// About 450 KB of code in `lang`, Rust or Python.
fn source(lang: &str) -> String {
    let mut code = String::new();
    if lang == "rust" {
        code.push_str("//! Generated records.\nuse std::collections::HashMap;\n\n");
    }
    let mut i = 0;
    while code.len() < 450 * 1024 {
        let item = match lang {
            "rust" => format!(
                r#"/// A record of kind {i}.
#[derive(Debug, Clone, PartialEq)]
pub struct Record{i} {{
    id: usize,
    name: String,
    tags: Vec<&'static str>,
}}

impl Record{i} {{
    /// The record, or `None` when `id` is out of range.
    pub fn new(id: usize, name: impl Into<String>) -> Option<Self> {{
        if id > {limit} {{
            return None;
        }}
        let tags = match id % 3 {{
            0 => vec!["zero", "first"],
            1 => vec!["one"],
            _ => Vec::new(),
        }};
        Some(Self {{ id, name: name.into(), tags }})
    }}

    pub fn describe(&self, cache: &mut HashMap<usize, String>) -> &str {{
        cache
            .entry(self.id)
            .or_insert_with(|| format!("{{}} #{{}} ({{:?}})", self.name, self.id, self.tags))
    }}
}}

"#,
                limit = i * 7 + 3
            ),
            _ => format!("value_{i} = compute({i}, \"label {i}\")  # note\n"),
        };
        code.push_str(&item);
        i += 1;
    }
    code
}

const CHUNK: usize = 40;

struct Answer {
    state: Entity<TextViewState>,
    /// Shows the first lines of the code block only, as a transcript does
    /// until the reader expands it.
    max_lines: Option<usize>,
}

impl Render for Answer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().px(px(20.)).child(
            TextView::new(&self.state).when_some(self.max_lines, |view, lines| {
                view.code_block_max_lines(lines)
            }),
        )
    }
}

/// Streams a fenced block in `lang` that starts at `size_kb` KB.
fn stream_block(lang: &str, size_kb: usize, max_lines: Option<usize>, cx: &mut BenchAppContext) {
    cx.update(gpui_component::init);
    let source = source(lang);
    let mut end = size_kb * 1024;
    while !source.is_char_boundary(end) {
        end += 1;
    }
    let markdown = format!("Here is the file:\n\n```{lang}\n{}", &source[..end]);
    let mut window = cx.add_empty_window();
    let answer = window.update(|window, cx| {
        window.replace_root(cx, |_, cx| Answer {
            state: cx.new(|cx| TextViewState::markdown(&markdown, cx)),
            max_lines,
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
    let source = source("rust");
    let mut end = 200 * 1024;
    while !source.is_char_boundary(end) {
        end += 1;
    }
    let markdown = format!("Here is the file:\n\n```rust\n{}\n```\n", &source[..end]);
    let mut window = cx.add_empty_window();
    let answer = window.update(|window, cx| {
        window.replace_root(cx, |_, cx| Answer {
            state: cx.new(|cx| TextViewState::markdown(&markdown, cx)),
            max_lines: None,
        })
    });
    cx.run_until_idle();

    cx.bench_renderer(answer, |_, _, cx| cx.notify());
}

#[gpui::bench]
fn stream_rust_block_50kb(cx: &mut BenchAppContext) {
    stream_block("rust", 50, None, cx);
}

#[gpui::bench]
fn stream_rust_block_200kb(cx: &mut BenchAppContext) {
    stream_block("rust", 200, None, cx);
}

/// The 200 KB block showing its first 40 lines.
#[gpui::bench]
fn stream_rust_block_200kb_capped(cx: &mut BenchAppContext) {
    stream_block("rust", 200, Some(40), cx);
}

#[gpui::bench]
fn stream_python_block_200kb(cx: &mut BenchAppContext) {
    stream_block("python", 200, None, cx);
}

/// The 200 KB block showing its first 40 lines.
#[gpui::bench]
fn stream_python_block_200kb_capped(cx: &mut BenchAppContext) {
    stream_block("python", 200, Some(40), cx);
}

gpui::bench_group!(
    benches,
    redraw_rust_block_200kb,
    stream_rust_block_50kb,
    stream_rust_block_200kb,
    stream_rust_block_200kb_capped,
    stream_python_block_200kb,
    stream_python_block_200kb_capped
);
gpui::bench_main!(benches);
