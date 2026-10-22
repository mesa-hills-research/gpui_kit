//! The heap a text view holds for the Markdown it shows, before anything is
//! drawn.
//!
//! This is a test of its own because it counts every allocation of the
//! process, so nothing else may run beside it.

use gpui::{AppContext as _, TestApp};
use gpui_base::text::TextViewState;

#[path = "../benches/support/mod.rs"]
mod support;

/// How many bytes of heap a parsed answer may take for each byte of its
/// Markdown. Parsing the text twice and keeping both documents took 86.
const BYTES_PER_SOURCE_BYTE: f64 = 60.;

#[test]
fn parsed_markdown_takes_a_bounded_number_of_bytes_for_each_byte_of_source() {
    let mut app = TestApp::with_text_system(gpui_platform::bench_text_system());
    app.update(gpui_base::init);
    let sources: Vec<String> = (0..40).map(support::answer).collect();
    let source_bytes: usize = sources.iter().map(String::len).sum();

    let before = support::live();
    let states = app.update(|cx| {
        sources
            .iter()
            .map(|source| cx.new(|cx| TextViewState::markdown(source, cx)))
            .collect::<Vec<_>>()
    });
    app.run_until_parked();
    let per_byte = (support::live() - before) as f64 / source_bytes as f64;

    assert!(
        per_byte < BYTES_PER_SOURCE_BYTE,
        "{per_byte:.1} bytes of heap for each byte of Markdown"
    );
    drop(states);
}
