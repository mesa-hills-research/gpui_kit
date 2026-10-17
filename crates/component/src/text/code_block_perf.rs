//! Timing harness for code block highlighting while a block streams in.
//!
//! Ignored by default. Run it in release, one test at a time, with
//!
//! ```text
//! cargo test --release -p gpui-component --features tree-sitter-languages --lib \
//!     code_block_perf -- --ignored --nocapture --test-threads 1
//! ```
//!
//! The input is generated the same way every time, so the numbers can be
//! compared across commits.

use std::time::{Duration, Instant};

use gpui::SharedString;
use gpui_base::text::CodeBlock;

use crate::highlighter::HighlightTheme;

/// About 500 KB of Rust.
fn rust_source() -> String {
    generated_code("rust", 500)
}

/// At least `kb` kilobytes of code in `lang`, generated the same way every
/// time. Rust has items with nested bodies, and Python, TypeScript, JSON,
/// YAML, diff and Markdown are flat, with roots that hold thousands of
/// nodes.
pub(crate) fn generated_code(lang: &str, kb: usize) -> String {
    let mut code = String::new();
    match lang {
        "json" => code.push_str("[\n"),
        "rust" => code.push_str("//! Generated records.\nuse std::collections::HashMap;\n\n"),
        _ => {}
    }
    let mut i = 0;
    while code.len() < kb * 1024 + 8192 {
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
            "python" => format!("value_{i} = compute({i}, \"label {i}\")  # note\n"),
            "typescript" => format!("export const v{i}: number = compute({i}, `t${{{i}}}`);\n"),
            "json" => {
                format!("  {{\"id\": {i}, \"name\": \"item {i}\", \"tags\": [\"a\", \"b\"]}},\n")
            }
            "yaml" => format!("- id: {i}\n  name: \"item {i}\"\n  tags: [a, b]\n"),
            "diff" => format!("+line {i} added\n-line {i} removed\n context {i}\n"),
            "markdown" => format!(
                "## Section {i}\n\nSome *text* with `code {i}` and a [link](http://x/{i}).\n\n- item **{i}**\n\n"
            ),
            _ => panic!("no generated {lang}"),
        };
        code.push_str(&item);
        i += 1;
    }
    code
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

/// The resident memory of this process, in megabytes.
fn resident_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
            line.split_whitespace().nth(1)?.parse::<f64>().ok()
        })
        .map_or(0., |kb| kb / 1024.)
}

#[test]
#[ignore = "timing harness, run by hand in release"]
fn first_highlight_of_a_language() {
    let before = resident_mb();
    let highlighter =
        super::component_code_block_highlighter(HighlightTheme::default_light(), None);
    let block = CodeBlock::from_code("fn main() {}\n", Some("rust"));
    let start = Instant::now();
    let styles = highlighter(&block);
    let first = start.elapsed();
    let start = Instant::now();
    let _ = highlighter(&CodeBlock::from_code("fn main() { 1 }\n", Some("rust")));
    let second = start.elapsed();
    println!(
        "first rust highlight: {first:.2?} ({} spans), second: {second:.2?}, resident memory {before:.0} MB -> {:.0} MB",
        styles.len(),
        resident_mb()
    );
}

/// The first highlight of a language the way an application gets it. The
/// call returns plain code at once and the language compiles in the
/// background.
#[gpui::test]
#[ignore = "timing harness, run by hand in release"]
fn first_highlight_off_the_ui_thread(cx: &mut gpui::TestAppContext) {
    use crate::highlighter::{Waiter, highlight_code_block};

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    let theme = HighlightTheme::default_light();
    let code = SharedString::from("fn main() {}\n");
    let start = Instant::now();
    let plain = highlight_code_block("rust", &code, true, &theme, Some(&waiter));
    let call = start.elapsed();
    let start = Instant::now();
    cx.run_until_parked();
    let compile = start.elapsed();
    let styles = highlight_code_block("rust", &code, true, &theme, Some(&waiter));
    println!(
        "first rust highlight: the call took {call:.2?} ({} spans), compiling in the background {compile:.2?}, then {} spans",
        plain.len(),
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
    let highlighter =
        super::component_code_block_highlighter(HighlightTheme::default_light(), None);
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
    let light = super::component_code_block_highlighter(HighlightTheme::default_light(), None);
    let dark = super::component_code_block_highlighter(HighlightTheme::default_dark(), None);
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

/// The languages and block sizes of the streaming tables.
const STREAMED: [&str; 7] = [
    "rust",
    "python",
    "typescript",
    "json",
    "yaml",
    "diff",
    "markdown",
];

/// Average, 95th percentile and worst of `times`, in milliseconds.
fn summary(mut times: Vec<Duration>) -> (f64, f64, f64) {
    times.sort();
    let ms = |time: Duration| time.as_secs_f64() * 1000.;
    let average = times.iter().map(|time| ms(*time)).sum::<f64>() / times.len() as f64;
    (
        average,
        ms(times[times.len() * 95 / 100]),
        ms(times[times.len() - 1]),
    )
}

/// The work of every 4-byte chunk of a block that streams in, done in the
/// background as for a code block of a text view.
#[gpui::test]
#[ignore = "timing harness, run by hand in release"]
fn background_work_per_chunk(cx: &mut gpui::TestAppContext) {
    use crate::highlighter::{Waiter, time_streamed_chunks};

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    println!(
        "| language | block | avg | p95 | worst | parse | first change | layers | highlight |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    for lang in STREAMED {
        for kb in [100usize, 200] {
            let code = generated_code(lang, kb);
            let from = kb * 1024;
            let (times, phases) =
                time_streamed_chunks(lang, &code, from, from + 3000, 4, Some(&waiter));
            let (average, p95, worst) = summary(times);
            let [parse, change, layers, highlight] =
                phases.map(|phase| phase.as_secs_f64() * 1000.);
            println!(
                "| {lang} | {kb} KB | {average:.2} ms | {p95:.2} ms | {worst:.2} ms | {parse:.2} | {change:.2} | {layers:.2} | {highlight:.2} |"
            );
        }
    }
}

/// What asking for the highlights of a chunk of a streamed block costs the
/// UI thread, while the background catches up.
#[gpui::test]
#[ignore = "timing harness, run by hand in release"]
fn ui_thread_work_per_chunk(cx: &mut gpui::TestAppContext) {
    use crate::highlighter::{Waiter, highlight_code_block};

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    let theme = HighlightTheme::default_light();
    println!("| language | block | avg | p95 | worst |");
    println!("|---|---:|---:|---:|---:|");
    for lang in STREAMED {
        let code = generated_code(lang, 200);
        let mut end = 200 * 1024;
        let first = SharedString::from(prefix(&code, end).to_string());
        highlight_code_block(lang, &first, false, &theme, Some(&waiter));
        cx.run_until_parked();
        let mut times = Vec::new();
        for chunk in 0..750 {
            end += 4;
            let block = SharedString::from(prefix(&code, end).to_string());
            let start = Instant::now();
            let styles = highlight_code_block(lang, &block, false, &theme, Some(&waiter));
            let elapsed = start.elapsed();
            times.push(elapsed);
            if elapsed > Duration::from_millis(1) {
                println!("  {lang}: chunk {chunk} took {elapsed:.2?}");
            }
            std::hint::black_box(styles);
            // The background takes a chunk now and then, as it can.
            if chunk % 8 == 7 {
                cx.run_until_parked();
            }
        }
        let (average, p95, worst) = summary(times);
        println!(
            "| {lang} | 200 KB | {:.1} µs | {:.1} µs | {:.1} µs |",
            average * 1000.,
            p95 * 1000.,
            worst * 1000.
        );
    }
}

/// The memory the kept highlighting of 40 blocks of 200 KB takes, each
/// highlighted once.
#[test]
#[ignore = "memory harness, run by hand in release"]
fn memory_of_kept_blocks() {
    let highlighter =
        super::component_code_block_highlighter(HighlightTheme::default_light(), None);
    let lang = Some(SharedString::from("rust"));
    let _ = highlighter(&CodeBlock::from_code("fn main() {}", lang.clone()));
    let source = rust_source();
    let before = resident_mb();
    for block in 0..40 {
        // Each block differs from the start, so none extends another.
        let code = format!("// block {block}\n{}", prefix(&source, 200 * 1024));
        std::hint::black_box(highlighter(&CodeBlock::from_code(code, lang.clone())));
    }
    println!(
        "40 blocks of 200 KB: resident memory {before:.0} MB -> {:.0} MB",
        resident_mb()
    );
}

/// What the call that asks for the highlights of a 200 KB block in a new
/// theme costs, for a block highlighted in the background.
#[gpui::test]
#[ignore = "timing harness, run by hand in release"]
fn theme_change_off_the_ui_thread(cx: &mut gpui::TestAppContext) {
    use crate::highlighter::{Waiter, highlight_code_block};

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    let (light, dark) = (
        HighlightTheme::default_light(),
        HighlightTheme::default_dark(),
    );
    println!("| language | block | spans | switch |");
    println!("|---|---:|---:|---:|");
    for lang in STREAMED {
        let code = SharedString::from(prefix(&generated_code(lang, 200), 200 * 1024).to_string());
        // The language and those it injects compile, then the code is
        // highlighted.
        for _ in 0..4 {
            highlight_code_block(lang, &code, true, &light, Some(&waiter));
            cx.run_until_parked();
        }
        let start = Instant::now();
        let styles = highlight_code_block(lang, &code, true, &dark, Some(&waiter));
        let switched = start.elapsed();
        assert!(!styles.is_pending());
        println!("| {lang} | 200 KB | {} | {switched:.2?} |", styles.len());
    }
}

/// The background work of showing a capped 200 KB block whole, whose first
/// 60 lines were highlighted before.
#[gpui::test]
#[ignore = "timing harness, run by hand in release"]
fn showing_a_capped_block_whole(cx: &mut gpui::TestAppContext) {
    use crate::highlighter::{Waiter, highlight_code_block};

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    let theme = HighlightTheme::default_light();
    println!("| language | block | background work |");
    println!("|---|---:|---:|");
    for lang in STREAMED {
        let code = prefix(&generated_code(lang, 200), 200 * 1024).to_string();
        let first_lines: SharedString = code
            .split_inclusive('\n')
            .take(60)
            .collect::<String>()
            .into();
        // The language and those it injects compile, then the first lines
        // are highlighted.
        for _ in 0..4 {
            highlight_code_block(lang, &first_lines, true, &theme, Some(&waiter));
            cx.run_until_parked();
        }
        let whole = SharedString::from(code);
        highlight_code_block(lang, &whole, true, &theme, Some(&waiter));
        let start = Instant::now();
        cx.run_until_parked();
        let work = start.elapsed();
        assert!(!highlight_code_block(lang, &whole, true, &theme, Some(&waiter)).is_pending());
        println!("| {lang} | 200 KB | {work:.2?} |");
    }
}
