//! Timing harness for code block highlighting while a block streams in.
//!
//! Ignored by default: run in release, one test at a time, with
//!
//! ```text
//! cargo test --release -p gpui-component --features tree-sitter-languages --lib \
//!     code_block_perf -- --ignored --nocapture --test-threads 1
//! ```
//!
//! The input is this repository's own Rust source, so the numbers can be
//! compared across commits.

use std::time::{Duration, Instant};

use gpui::SharedString;
use gpui_base::text::CodeBlock;

use crate::highlighter::HighlightTheme;

/// About 500 KB of Rust from this repository.
fn rust_source() -> String {
    [
        include_str!("../../../base/src/text/node.rs"),
        include_str!("../../../base/src/text/state.rs"),
        include_str!("../../../base/src/text/text_view.rs"),
    ]
    .concat()
}

/// Cut `text` to at most `len` bytes, on a char boundary.
fn prefix(text: &str, len: usize) -> &str {
    let mut end = len.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

const CHUNK: usize = 40;
const CHUNKS: usize = 25;

#[test]
#[ignore = "timing harness, run by hand in release"]
fn first_highlight_of_a_language() {
    let highlighter = super::component_code_block_highlighter(HighlightTheme::default_light());
    let block = CodeBlock::from_code("fn main() {}\n", Some("rust"));
    let start = Instant::now();
    let styles = highlighter(&block);
    let first = start.elapsed();
    let start = Instant::now();
    let _ = highlighter(&CodeBlock::from_code("fn main() { 1 }\n", Some("rust")));
    let second = start.elapsed();
    println!(
        "first rust highlight: {first:.2?} ({} spans), second: {second:.2?}",
        styles.len()
    );
}

#[test]
#[ignore = "timing harness, run by hand in release"]
fn another_highlighter_of_a_language() {
    let first = Instant::now();
    let _ = crate::highlighter::SyntaxHighlighter::new("rust");
    let first = first.elapsed();
    let second = Instant::now();
    let _ = crate::highlighter::SyntaxHighlighter::new("rust");
    let second = second.elapsed();
    println!("first rust highlighter: {first:.2?}, another one: {second:.2?}");
}

#[test]
#[ignore = "timing harness, run by hand in release"]
fn streamed_block_per_chunk() {
    let source = rust_source();
    let highlighter = super::component_code_block_highlighter(HighlightTheme::default_light());
    let lang = Some(SharedString::from("rust"));
    // Warm the grammar and the query outside the measurement.
    let _ = highlighter(&CodeBlock::from_code("fn main() {}", lang.clone()));

    println!("| block size | per chunk (avg of {CHUNKS}) | worst |");
    println!("|---:|---:|---:|");
    for size_kb in [5usize, 20, 50, 100, 200] {
        let base = size_kb * 1024;
        let _ = highlighter(&CodeBlock::from_code(
            prefix(&source, base).to_string(),
            lang.clone(),
        ));
        let mut total = Duration::ZERO;
        let mut worst = Duration::ZERO;
        for chunk in 1..=CHUNKS {
            let code = prefix(&source, base + chunk * CHUNK).to_string();
            let block = CodeBlock::from_code(code, lang.clone());
            let start = Instant::now();
            let styles = highlighter(&block);
            let elapsed = start.elapsed();
            std::hint::black_box(styles);
            total += elapsed;
            worst = worst.max(elapsed);
        }
        println!(
            "| {size_kb} KB | {:.3} ms | {:.3} ms |",
            total.as_secs_f64() * 1000. / CHUNKS as f64,
            worst.as_secs_f64() * 1000.
        );
    }
}

#[test]
#[ignore = "timing harness, run by hand in release"]
fn theme_change_of_a_long_block() {
    let source = rust_source();
    let code = prefix(&source, 200 * 1024).to_string();
    let lang = Some(SharedString::from("rust"));
    let light = super::component_code_block_highlighter(HighlightTheme::default_light());
    let dark = super::component_code_block_highlighter(HighlightTheme::default_dark());
    let block = CodeBlock::from_code(code, lang);
    let start = Instant::now();
    let styles = light(&block);
    let first = start.elapsed();
    let start = Instant::now();
    let _ = dark(&block);
    let switched = start.elapsed();
    println!(
        "200 KB block: first highlight {first:.2?} ({} spans), in another theme {switched:.2?}",
        styles.len()
    );
}
