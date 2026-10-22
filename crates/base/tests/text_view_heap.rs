//! The heap a text view holds for the Markdown it shows, before anything is
//! drawn.
//!
//! This is a test of its own because it counts every allocation of the
//! process, so nothing else may run beside it.

use gpui::{AppContext as _, TestAppContext};
use gpui_base::text::TextViewState;

#[path = "../benches/support/mod.rs"]
mod support;

/// How many bytes of heap a parsed answer may take for each byte of its
/// Markdown. Parsing the text twice and keeping both documents took 86,
/// giving every view its own style and message channels took 53, and
/// nodes that held room for an image and an inline object took 42.
const BYTES_PER_SOURCE_BYTE: f64 = 28.;

// The test harness runs a test off the main thread, and a real macOS platform,
// which `gpui_platform::bench_text_system` creates, only exists on the main
// thread. So this runs on GPUI's test platform, as the unit tests do. The bench
// has a main of its own and uses the real text system.
#[gpui::test]
fn parsed_markdown_takes_a_bounded_number_of_bytes_for_each_byte_of_source(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_base::init);
    let sources: Vec<String> = (0..40).map(support::answer).collect();
    let source_bytes: usize = sources.iter().map(String::len).sum();

    let before = support::live();
    let states = cx.update(|cx| {
        sources
            .iter()
            .map(|source| cx.new(|cx| TextViewState::markdown(source, cx)))
            .collect::<Vec<_>>()
    });
    cx.run_until_parked();
    let per_byte = (support::live() - before) as f64 / source_bytes as f64;

    assert!(
        per_byte < BYTES_PER_SOURCE_BYTE,
        "{per_byte:.1} bytes of heap for each byte of Markdown"
    );
    drop(states);
}
