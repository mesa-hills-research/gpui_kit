use std::ops::Range;

use gpui::{HighlightStyle, SharedString};
use ropey::Rope;

use super::{CodeBlockHighlighter, KeptBlocks, highlight_code_block};
use crate::highlighter::{HighlightTheme, SyntaxHighlighter, compiled::compiled_language};

/// The style of every byte of `len` bytes of code.
fn per_byte(len: usize, styles: &[(Range<usize>, HighlightStyle)]) -> Vec<HighlightStyle> {
    let mut bytes = vec![HighlightStyle::default(); len];
    for (range, style) in styles {
        for byte in &mut bytes[range.clone()] {
            *byte = *style;
        }
    }
    bytes
}

/// What [`SyntaxHighlighter`] makes of `code` from scratch: the reference.
fn from_scratch(lang: &str, code: &str, theme: &HighlightTheme) -> Vec<HighlightStyle> {
    let rope = Rope::from_str(code);
    let mut highlighter = SyntaxHighlighter::new(lang);
    highlighter.update(None, &rope, None);
    per_byte(code.len(), &highlighter.styles(&(0..code.len()), theme))
}

/// Asserts that `actual` styles `code` as `expected` does, naming the first
/// byte where they differ.
fn assert_same_styles(
    lang: &str,
    code: &str,
    expected: &[HighlightStyle],
    actual: &[HighlightStyle],
) {
    if let Some(ix) = expected.iter().zip(actual).position(|(e, a)| e != a) {
        let mut start = ix.saturating_sub(40);
        while !code.is_char_boundary(start) {
            start -= 1;
        }
        let mut end = (ix + 40).min(code.len());
        while !code.is_char_boundary(end) {
            end += 1;
        }
        panic!(
            "{lang}: byte {ix} of {} differs\nexpected {:?}\nactual   {:?}\naround: {:?}",
            code.len(),
            expected[ix],
            actual[ix],
            &code[start..end],
        );
    }
}

/// Chunk lengths of a stream, from a fixed seed.
fn chunk_lengths(seed: u64) -> impl Iterator<Item = usize> {
    let mut state = seed;
    std::iter::repeat_with(move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        1 + (state >> 33) as usize % 48
    })
}

/// Streams `code` into one highlighter in chunks and checks every step
/// against a highlight of the same text from scratch.
fn assert_streams_like_from_scratch(lang: &str, code: &str, seed: u64) {
    let theme = HighlightTheme::default_dark();
    let language = compiled_language(lang).unwrap_or_else(|| panic!("{lang} has a grammar"));
    let mut highlighter: Option<CodeBlockHighlighter> = None;
    let mut end = 0;
    let mut lengths = chunk_lengths(seed);
    while end < code.len() {
        end = (end + lengths.next().unwrap()).min(code.len());
        while !code.is_char_boundary(end) {
            end += 1;
        }
        let prefix = SharedString::from(code[..end].to_string());
        let highlighter = match &mut highlighter {
            Some(highlighter) => {
                highlighter.update(&prefix);
                highlighter
            }
            None => highlighter.insert(
                CodeBlockHighlighter::new(language.clone(), &prefix, None).expect("a parser"),
            ),
        };
        let actual = per_byte(end, &highlighter.styles(&theme));
        let expected = from_scratch(lang, &prefix, &theme);
        assert_same_styles(lang, &prefix, &expected, &actual);
    }
}

#[cfg(feature = "tree-sitter-languages")]
const RUST: &str = r##"//! A module doc comment.
use std::collections::HashMap;

/// A documented struct with a generic.
#[derive(Debug, Clone)]
pub struct Cache<T: Clone> {
    entries: HashMap<String, T>,
    hits: usize,
}

impl<T: Clone> Cache<T> {
    pub fn get(&mut self, key: &str) -> Option<T> {
        let value = self.entries.get(key).cloned();
        if value.is_some() {
            self.hits += 1;
        }
        println!("lookup {key}: {} hits", self.hits);
        value
    }
}

/* A block comment that
   spans lines, with "quotes" and fn keywords */
fn main() {
    let raw = r#"raw "string" with fn inside"#;
    let numbers = vec![1, 2, 3, 0x1f, 1_000];
    let total: i64 = numbers.iter().map(|n| n * 2).sum();
    assert_eq!(total, 2 * (1 + 2 + 3 + 31 + 1000), "sum of {:?}", numbers);
    macro_rules! square {
        ($x:expr) => {
            $x * $x
        };
    }
    let c = 'c';
    match total {
        0 => {}
        n if n > 10 => println!("big {n} {c} {raw}"),
        _ => unreachable!(),
    }
}
"##;

#[cfg(feature = "tree-sitter-languages")]
const TYPESCRIPT: &str = r#"/**
 * A documented function.
 * @param name the name to greet
 */
export function greet(name: string): string {
  const pattern = /^[a-z]+$/i;
  const html = `<b>${name}</b>`;
  return pattern.test(name) ? `Hello, ${name}!` : "Who?";
}

interface Point { x: number; y?: number }
type Pair<T> = [T, T];

class Counter {
  private count = 0;
  increment(by: number = 1): number {
    // A line comment
    this.count += by;
    return this.count;
  }
}

const sql = sql`select * from users where id = ${1}`;
const css = css`color: red;`;
"#;

#[cfg(feature = "tree-sitter-languages")]
const PYTHON: &str = r#"import os
from typing import Optional

class Greeter:
    """A docstring that
    spans lines."""

    def __init__(self, name: str = "world") -> None:
        self.name = name

    @property
    def greeting(self) -> str:
        return f"Hello, {self.name}! {1 + 2}"

def main(argv: Optional[list] = None) -> int:
    for i in range(10):
        if i % 2 == 0:
            print(i, os.getcwd(), r'raw\n', b"bytes")
    return 0
"#;

#[cfg(feature = "tree-sitter-languages")]
const HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <title>Streaming</title>
  <style>
    body { color: #333; margin: 0 auto; }
    .card > h1 { font-weight: 700; }
  </style>
</head>
<body>
  <!-- a comment -->
  <div class="card" data-id="1">
    <h1>Title &amp; more</h1>
  </div>
  <script>
    const items = [1, 2, 3];
    function render(list) {
      return list.map((item) => `<li>${item}</li>`).join("");
    }
    document.body.innerHTML += render(items);
  </script>
</body>
</html>
"#;

#[cfg(feature = "tree-sitter-languages")]
const MARKDOWN: &str = r#"# A title

Some *emphasis*, **strong** text and `inline code`, with a [link](https://example.com).

- a list item with `code`
- another *item*

```rust
fn main() { println!("fenced"); }
```

> A quote with _emphasis_.

```json
{"a": [1, 2, true, null]}
```
"#;

const JSON: &str = r#"{
  "name": "gpui-kit",
  "version": "0.7.1",
  "nested": { "list": [1, 2.5, -3e10, true, false, null], "escaped": "a \"quote\" \n" },
  "empty": {}
}
"#;

#[cfg(feature = "tree-sitter-languages")]
const BASH: &str = r#"#!/usr/bin/env bash
set -euo pipefail
# A comment
for file in "$@"; do
  if [[ -f "$file" ]]; then
    echo "found ${file##*/}" | tr a-z A-Z
  fi
done
cat <<EOF
heredoc $HOME
EOF
"#;

#[cfg(feature = "tree-sitter-languages")]
const CSS: &str = r#"@media (max-width: 600px) {
  .button:hover, #main > a[href^="http"] {
    color: rgb(10, 20, 30);
    margin: 0 auto !important;
  }
}
/* comment */
:root { --accent: #ff0080; }
"#;

#[cfg(feature = "tree-sitter-languages")]
const GO: &str = r#"package main

import (
	"fmt"
	"strings"
)

// Greeter greets.
type Greeter struct {
	Name string `json:"name"`
}

func (g *Greeter) Greet() string {
	return fmt.Sprintf("Hello, %s!", strings.ToUpper(g.Name))
}

func main() {
	g := &Greeter{Name: "world"}
	fmt.Println(g.Greet(), 42, 3.14, 'x', `raw`)
}
"#;

#[cfg(feature = "tree-sitter-languages")]
const YAML: &str = r#"name: CI
on:
  push:
    branches: [main]
jobs:
  test:
    runs-on: ubuntu-latest # a comment
    steps:
      - uses: actions/checkout@v4
      - run: |
          cargo test --all
          echo "done"
"#;

#[cfg(feature = "tree-sitter-languages")]
const PHP: &str = r#"<html>
<body>
<h1><?php echo $title; ?></h1>
<?php
/** A doc comment */
function greet(string $name): string {
    return "Hello, {$name}!";
}
foreach ([1, 2, 3] as $n) { echo greet("n$n"); }
?>
<p>After</p>
</body>
</html>
"#;

#[cfg(feature = "tree-sitter-languages")]
const LUA: &str = r#"local ffi = require("ffi")
ffi.cdef("int printf(const char *fmt, ...);")
-- a comment
local function greet(name)
  return "Hello, " .. name .. "!"
end
for i = 1, 10 do print(greet(tostring(i))) end
--[[ a long
comment ]]
"#;

#[cfg(feature = "tree-sitter-languages")]
const C: &str = r#"#include <stdio.h>
#define MAX(a, b) ((a) > (b) ? (a) : (b))
/* block */
static int count = 0;
int main(int argc, char **argv) {
    for (int i = 0; i < argc; i++) { printf("%s %d\n", argv[i], MAX(i, 2)); }
    return count;
}
"#;

#[cfg(feature = "tree-sitter-languages")]
const RUBY: &str = r#"# frozen_string_literal: true
class Greeter < Base
  attr_reader :name
  def initialize(name = "world")
    @name = name
  end
  def greet = "Hello, #{@name}!"
end
puts Greeter.new.greet if __FILE__ == $0
%w[a b c].each { |x| puts x.upcase }
"#;

#[cfg(feature = "tree-sitter-languages")]
const SQL: &str = r#"-- a comment
SELECT u.id, COUNT(o.id) AS orders, 'text' AS label
FROM users u LEFT JOIN orders o ON o.user_id = u.id
WHERE u.created_at > NOW() - INTERVAL '7 days' AND u.score >= 3.5
GROUP BY u.id ORDER BY orders DESC LIMIT 10;
"#;

#[cfg(feature = "tree-sitter-languages")]
const TOML: &str = r#"[package]
name = "gpui-kit" # name
version = "0.7.1"
features = ["a", "b"]
[dependencies.serde]
version = "1"
date = 1979-05-27T07:32:00Z
"#;

#[cfg(feature = "tree-sitter-languages")]
const DIFF: &str = r#"diff --git a/src/main.rs b/src/main.rs
index 1234567..89abcde 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
-    println!("old");
+    println!("new");
+    println!("more");
 }
"#;

#[cfg(feature = "tree-sitter-languages")]
/// Text in several scripts and an emoji, where a char boundary is not a
/// byte boundary.
const MULTIBYTE: &str = r#"// 你好，世界 — ünïcödé 🎉
fn main() {
    let s = "日本語のテキスト 🚀 and more";
    let c = '字';
    println!("{s} {c} — ✓");
}
"#;

#[cfg(feature = "tree-sitter-languages")]
#[test]
fn streamed_code_highlights_like_code_highlighted_from_scratch() {
    let cases: &[(&str, &str)] = &[
        ("rust", RUST),
        ("typescript", TYPESCRIPT),
        ("javascript", TYPESCRIPT.replace(": string", "").leak()),
        ("python", PYTHON),
        ("html", HTML),
        ("markdown", MARKDOWN),
        ("json", JSON),
        ("bash", BASH),
        ("css", CSS),
        ("go", GO),
        ("yaml", YAML),
        ("php", PHP),
        ("lua", LUA),
        ("c", C),
        ("cpp", C),
        ("ruby", RUBY),
        ("sql", SQL),
        ("toml", TOML),
        ("diff", DIFF),
        ("rust", MULTIBYTE),
    ];
    for (lang, code) in cases {
        for seed in [1, 7, 42] {
            assert_streams_like_from_scratch(lang, code, seed);
        }
    }
}

#[cfg(feature = "tree-sitter-languages")]
#[test]
fn code_streamed_a_character_at_a_time_highlights_like_code_highlighted_from_scratch() {
    // The smallest chunks put the end of the text inside every token once.
    let theme = HighlightTheme::default_dark();
    for (lang, code) in [("rust", MULTIBYTE), ("html", HTML), ("markdown", MARKDOWN)] {
        let language = compiled_language(lang).unwrap();
        let mut highlighter: Option<CodeBlockHighlighter> = None;
        for (end, _) in code.char_indices().skip(1).chain([(code.len(), ' ')]) {
            let prefix = SharedString::from(code[..end].to_string());
            let highlighter = match &mut highlighter {
                Some(highlighter) => {
                    highlighter.update(&prefix);
                    highlighter
                }
                None => highlighter
                    .insert(CodeBlockHighlighter::new(language.clone(), &prefix, None).unwrap()),
            };
            let actual = per_byte(end, &highlighter.styles(&theme));
            assert_same_styles(lang, &prefix, &from_scratch(lang, &prefix, &theme), &actual);
        }
    }
}

#[test]
fn streamed_json_highlights_like_json_highlighted_from_scratch() {
    // JSON is always compiled in, so this runs without any language feature.
    for seed in [1, 2, 3] {
        assert_streams_like_from_scratch("json", JSON, seed);
    }
}

#[cfg(feature = "tree-sitter-rust")]
#[test]
fn appending_reparses_once_and_a_theme_change_does_not_reparse() {
    let language = compiled_language("rust").unwrap();
    let code: SharedString = "fn main() {\n    let x = 1;\n".into();
    let mut highlighter = CodeBlockHighlighter::new(language, &code, None).unwrap();
    assert_eq!(highlighter.parses, 1);

    let longer: SharedString = format!("{code}    let y = \"two\";\n}}\n").into();
    highlighter.update(&longer);
    assert_eq!(highlighter.parses, 2, "an append reparses once");

    let light = HighlightTheme::default_light();
    let dark = HighlightTheme::default_dark();
    let light_styles = highlighter.styles(&light);
    let dark_styles = highlighter.styles(&dark);
    assert_eq!(highlighter.parses, 2, "a theme change only resolves colors");

    let string = longer.find("\"two\"").unwrap();
    let color_at = |styles: &[(Range<usize>, HighlightStyle)]| {
        styles
            .iter()
            .find(|(range, _)| range.contains(&string))
            .and_then(|(_, style)| style.color)
    };
    assert_eq!(
        color_at(&light_styles),
        light.style("string").and_then(|s| s.color)
    );
    assert_eq!(
        color_at(&dark_styles),
        dark.style("string").and_then(|s| s.color)
    );
    assert_ne!(color_at(&light_styles), color_at(&dark_styles));

    assert_same_styles(
        "rust",
        &longer,
        &from_scratch("rust", &longer, &dark),
        &per_byte(longer.len(), &dark_styles),
    );
}

#[cfg(feature = "tree-sitter-rust")]
#[test]
fn replaced_code_is_highlighted_from_scratch() {
    let language = compiled_language("rust").unwrap();
    let theme = HighlightTheme::default_dark();
    let mut highlighter =
        CodeBlockHighlighter::new(language, &"/* an open comment\nfn a() {}\n".into(), None)
            .unwrap();
    let replaced: SharedString = "fn a() { 1 }\n".into();
    highlighter.update(&replaced);
    assert_same_styles(
        "rust",
        &replaced,
        &from_scratch("rust", &replaced, &theme),
        &per_byte(replaced.len(), &highlighter.styles(&theme)),
    );
}

#[cfg(feature = "tree-sitter-rust")]
#[test]
fn interleaved_blocks_each_keep_their_highlighting() {
    let theme = HighlightTheme::default_dark();
    let a = "fn a() {\n    let s = \"a\";\n";
    let b = "struct B {\n    field: u8,\n";
    let mut a_code = String::new();
    let mut b_code = String::new();
    for (a_line, b_line) in a.split_inclusive('\n').zip(b.split_inclusive('\n')) {
        a_code.push_str(a_line);
        b_code.push_str(b_line);
        for code in [&a_code, &b_code] {
            let code = SharedString::from(code.clone());
            let actual = per_byte(
                code.len(),
                &highlight_code_block("rust", &code, true, &theme, None),
            );
            assert_same_styles("rust", &code, &from_scratch("rust", &code, &theme), &actual);
        }
    }
}

#[test]
fn a_block_extends_the_kept_block_whose_code_it_continues() {
    let language = compiled_language("json").unwrap();
    let mut kept = KeptBlocks::default();
    let short: SharedString = "[1".into();
    let long: SharedString = "[1, 2".into();
    kept.put(CodeBlockHighlighter::new(language.clone(), &short, None).unwrap());
    kept.put(CodeBlockHighlighter::new(language.clone(), &long, None).unwrap());

    let longer: SharedString = "[1, 2, 3]".into();
    let block = kept.take(&language, &longer).expect("a prefix is kept");
    assert_eq!(block.source, long, "the longest prefix is extended");
    kept.put(block);

    let block = kept
        .take(&language, &short)
        .expect("the exact code is kept");
    assert_eq!(block.source, short);
    assert!(kept.take(&language, &"{}".into()).is_none());
}

#[test]
fn kept_blocks_stay_within_their_budget() {
    let language = compiled_language("json").unwrap();
    let mut kept = KeptBlocks {
        max_blocks: 4,
        max_memory: 2048,
        ..KeptBlocks::default()
    };
    for ix in 0..8 {
        let code = SharedString::from(format!("[{ix}]"));
        kept.put(CodeBlockHighlighter::new(language.clone(), &code, None).unwrap());
    }
    assert_eq!(kept.blocks.len(), 4);
    assert!(
        kept.take(&language, &"[7]".into()).is_some(),
        "the latest is kept"
    );
    assert!(
        kept.take(&language, &"[0]".into()).is_none(),
        "the oldest is gone"
    );

    // A block over the memory budget on its own is kept while it is in use.
    let huge = SharedString::from(format!("[{}1]", "1,".repeat(64)));
    kept.put(CodeBlockHighlighter::new(language.clone(), &huge, None).unwrap());
    assert_eq!(kept.blocks.len(), 1);
}

#[test]
fn highlights_handed_out_stay_as_they_were_while_the_block_grows() {
    let theme = HighlightTheme::default_dark();
    let language = compiled_language("json").unwrap();
    let mut highlighter = CodeBlockHighlighter::new(language, &"[1, 2".into(), None).unwrap();
    let held = highlighter.styles(&theme);
    let before = held.to_vec();

    highlighter.update(&"[1, 2, \"three\"]".into());
    let grown = highlighter.styles(&theme);

    assert_eq!(
        held.to_vec(),
        before,
        "a block still showing them keeps its highlights"
    );
    assert_eq!(grown.last().map(|(range, _)| range.end), Some(15));
}

/// Rust from this repository whose unfinished prefixes often parse to an
/// ERROR root.
#[cfg(feature = "tree-sitter-rust")]
use {super::ts_point, gpui_base::input::RopeExt as _, tree_sitter::InputEdit};

#[cfg(feature = "tree-sitter-rust")]
fn rust_with_error_roots() -> &'static str {
    let source = include_str!("../../../../base/src/text/state.rs");
    let mut end = 8000;
    while !source.is_char_boundary(end) {
        end += 1;
    }
    &source[..end]
}

#[cfg(feature = "tree-sitter-rust")]
#[test]
fn code_that_parses_to_an_error_root_streams_like_code_highlighted_from_scratch() {
    let code = rust_with_error_roots();
    for seed in [3, 11] {
        assert_streams_like_from_scratch("rust", code, seed);
    }
}

#[cfg(feature = "tree-sitter-rust")]
#[test]
fn an_append_under_an_error_root_keeps_the_complete_items_before_it() {
    let code = rust_with_error_roots();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();
    // An unfinished prefix whose whole tree is one ERROR node, which stays
    // one after the append.
    let mut reparse = |old_len: usize| {
        let mut old = parser.parse(&code[..old_len], None).unwrap();
        let new_len = old_len + 20;
        let rope = Rope::from_str(&code[..new_len]);
        let start_position = ts_point(rope.offset_to_point(old_len));
        old.edit(&InputEdit {
            start_byte: old_len,
            old_end_byte: old_len,
            new_end_byte: new_len,
            start_position,
            old_end_position: start_position,
            new_end_position: ts_point(rope.offset_to_point(new_len)),
        });
        let new = parser.parse(&code[..new_len], Some(&old)).unwrap();
        (old, new)
    };
    let (old_len, old, new) = (1000..code.len() - 20)
        .step_by(7)
        .filter(|end| code.is_char_boundary(*end) && code.is_char_boundary(end + 20))
        .find_map(|old_len| {
            let (old, new) = reparse(old_len);
            // Tree-sitter reports the ERROR root changed as a whole.
            (old.changed_ranges(&new)
                .next()
                .map(|range| range.start_byte)
                == Some(0))
            .then_some((old_len, old, new))
        })
        .expect("an append that changes an ERROR root");
    let start = super::first_change(&old, &new, old_len, |_| false);
    // The complete items before the unfinished one are kept.
    assert!(
        start > 0 && start < old_len,
        "highlighting starts again at {start} of {old_len}"
    );
}

/// Registers JSON under a name no other test uses, so it starts out not
/// compiled.
fn register_fresh_json(test: &str) -> SharedString {
    use crate::highlighter::{LanguageConfig, LanguageRegistry};

    let name = SharedString::from(format!("json-{test}"));
    LanguageRegistry::singleton().register(
        &name,
        &LanguageConfig::new(
            name.clone(),
            tree_sitter_json::LANGUAGE.into(),
            vec![],
            "(number) @number",
            "",
            "",
        ),
    );
    name
}

#[gpui::test]
fn a_code_block_is_plain_until_its_language_is_ready(cx: &mut gpui::TestAppContext) {
    use crate::highlighter::compiled::Waiter;

    let lang = register_fresh_json("off-thread-test");
    let (ready, told) = gpui_base::async_util::unbounded::<()>();
    let waiter = Waiter::new(cx.background_executor.clone(), Some(ready));
    let theme = HighlightTheme::default_dark();
    let code = SharedString::from("[42]");
    let number = |styles: &[(Range<usize>, HighlightStyle)]| {
        styles
            .iter()
            .find(|(range, _)| range.contains(&1))
            .and_then(|(_, style)| style.color)
    };

    assert!(
        highlight_code_block(&lang, &code, true, &theme, Some(&waiter)).is_empty(),
        "plain while the language compiles"
    );
    cx.run_until_parked();
    assert!(told.try_recv().is_ok(), "the waiter hears when it is ready");
    // Then the code is highlighted in the background.
    let pending = highlight_code_block(&lang, &code, true, &theme, Some(&waiter));
    assert!(pending.is_pending());
    cx.run_until_parked();
    let styles = highlight_code_block(&lang, &code, true, &theme, Some(&waiter));
    assert!(!styles.is_pending());
    assert_eq!(
        number(&styles),
        theme.style("number").and_then(|style| style.color)
    );
}

#[gpui::test]
fn warming_a_language_up_compiles_it_in_the_background(cx: &mut gpui::TestAppContext) {
    use crate::highlighter::{
        LanguageRegistry,
        compiled::{Readiness, Waiter, compiled_language_now, layer_query_now},
    };

    let lang = register_fresh_json("warm-up-test");
    cx.update(|cx| LanguageRegistry::singleton().warm_up([lang.as_ref()], cx));
    cx.run_until_parked();

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    assert!(matches!(
        compiled_language_now(&lang, &waiter),
        Readiness::Ready(Some(_))
    ));
    // The query of a layer of it is compiled when a layer needs it.
    assert!(matches!(
        layer_query_now(&lang, &waiter),
        Readiness::Pending
    ));
}

#[gpui::test]
fn a_ready_language_leaves_the_installed_defaults_alone(cx: &mut gpui::TestAppContext) {
    use std::{cell::Cell, rc::Rc};

    cx.update(crate::init);
    let lang = register_fresh_json("ready-test");
    let installs = Rc::new(Cell::new(0));
    cx.update(|cx| {
        let installs = installs.clone();
        cx.observe_global::<gpui_base::TextViewDefaults>(move |_| {
            installs.set(installs.get() + 1);
        })
        .detach();
        let waiter = super::language_ready_waiter(cx).expect("init installs the waiter");
        let theme = HighlightTheme::default_dark();
        assert!(highlight_code_block(&lang, &"[1]".into(), true, &theme, Some(&waiter)).is_empty());
    });
    cx.run_until_parked();

    // Code blocks ask their highlighter again (see
    // `refresh_code_block_highlights`), and the defaults the application
    // installed stay as they are.
    assert_eq!(installs.get(), 0);
}

#[cfg(feature = "tree-sitter-languages")]
mod differential;

/// The job of the block of `code`, as `highlight_in_background` keeps it.
fn background_job(code: &str) -> std::sync::Arc<std::sync::Mutex<super::BlockJob>> {
    super::BACKGROUND_BLOCKS.with(|blocks| {
        blocks
            .borrow()
            .blocks
            .iter()
            .find(|job| job.lock().unwrap().wanted.as_ref() == code)
            .cloned()
            .expect("a job for the code")
    })
}

#[gpui::test]
fn a_block_is_highlighted_off_the_calling_thread(cx: &mut gpui::TestAppContext) {
    use crate::highlighter::compiled::Waiter;

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    let theme = HighlightTheme::default_dark();
    let language = compiled_language("json").unwrap();
    let chunks = ["[1, ", "2, \"three\"", ", {\"four\": 4}]\n"];

    // Chunks that arrive while nothing ran yet are taken in one go.
    let mut code = String::new();
    for chunk in chunks {
        code.push_str(chunk);
        let styles =
            highlight_code_block("json", &code.clone().into(), false, &theme, Some(&waiter));
        assert!(styles.is_pending());
        assert!(styles.is_empty(), "nothing is highlighted yet");
    }
    let job = background_job(&code);
    assert!(
        job.lock().unwrap().highlighter.is_none(),
        "nothing parsed here"
    );

    cx.run_until_parked();
    let styles = highlight_code_block("json", &code.clone().into(), false, &theme, Some(&waiter));
    assert!(!styles.is_pending());
    assert_eq!(
        per_byte(code.len(), &styles),
        from_scratch("json", &code, &theme)
    );
    let job = job.lock().unwrap();
    assert_eq!(job.highlighter.as_ref().unwrap().parses, 1);
    assert!(std::sync::Arc::ptr_eq(&job.language, &language));
}

#[gpui::test]
fn a_streamed_block_shows_its_last_highlights_until_the_next_are_made(
    cx: &mut gpui::TestAppContext,
) {
    use crate::highlighter::compiled::Waiter;

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    let theme = HighlightTheme::default_dark();
    compiled_language("json").unwrap();
    let first = SharedString::from("[1, 2");
    highlight_code_block("json", &first, false, &theme, Some(&waiter));
    cx.run_until_parked();
    let made = highlight_code_block("json", &first, false, &theme, Some(&waiter));
    assert!(!made.is_pending());

    let longer = SharedString::from("[1, 2, 3");
    let shown = highlight_code_block("json", &longer, false, &theme, Some(&waiter));
    assert!(shown.is_pending());
    assert_eq!(
        shown.to_vec(),
        made.to_vec(),
        "the chunk is plain meanwhile"
    );
    cx.run_until_parked();
    let styles = highlight_code_block("json", &longer, false, &theme, Some(&waiter));
    assert_eq!(
        per_byte(longer.len(), &styles),
        from_scratch("json", &longer, &theme)
    );
}

/// A capped block shown whole: its first lines were highlighted complete,
/// and the whole code extends them.
#[gpui::test]
fn a_complete_block_that_grows_is_parsed_once_more(cx: &mut gpui::TestAppContext) {
    use crate::highlighter::compiled::Waiter;

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    let theme = HighlightTheme::default_dark();
    compiled_language("json").unwrap();
    let items = (0..200)
        .map(|ix| format!("  {{\"id\": {ix}, \"shown\": true}},\n"))
        .collect::<String>();
    let code = format!("[\n{items}  {{}}\n]\n");
    let first_lines: SharedString = code
        .split_inclusive('\n')
        .take(60)
        .collect::<String>()
        .into();
    highlight_code_block("json", &first_lines, true, &theme, Some(&waiter));
    cx.run_until_parked();

    let whole = SharedString::from(code.clone());
    highlight_code_block("json", &whole, true, &theme, Some(&waiter));
    cx.run_until_parked();
    let styles = highlight_code_block("json", &whole, true, &theme, Some(&waiter));
    assert!(!styles.is_pending());
    assert_eq!(
        per_byte(code.len(), &styles),
        from_scratch("json", &code, &theme)
    );
    let job = background_job(&code);
    assert_eq!(
        job.lock().unwrap().highlighter.as_ref().unwrap().parses,
        2,
        "the first lines, then the whole code from scratch"
    );
}

/// A theme switch recolors a block highlighted in the background in the
/// frame it's asked for, rather than showing the previous theme's colors on
/// the new background until a task is done.
#[gpui::test]
fn a_theme_switch_recolors_a_block_at_once(cx: &mut gpui::TestAppContext) {
    use crate::highlighter::compiled::Waiter;

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    let (dark, light) = (
        HighlightTheme::default_dark(),
        HighlightTheme::default_light(),
    );
    compiled_language("json").unwrap();
    let code = SharedString::from("{\"theme\": [1, true, \"two\"]}\n");
    highlight_code_block("json", &code, true, &dark, Some(&waiter));
    cx.run_until_parked();
    let in_dark = highlight_code_block("json", &code, true, &dark, Some(&waiter));
    assert!(!in_dark.is_pending());

    let in_light = highlight_code_block("json", &code, true, &light, Some(&waiter));
    assert!(!in_light.is_pending());
    assert_ne!(in_light.to_vec(), in_dark.to_vec());
    assert_eq!(
        per_byte(code.len(), &in_light),
        from_scratch("json", &code, &light)
    );
    let job = background_job(&code);
    assert_eq!(job.lock().unwrap().highlighter.as_ref().unwrap().parses, 1);
}

/// An application can register a language at any time, such as when it
/// loads a grammar the first time a block needs it.
#[gpui::test]
fn a_block_keeps_its_highlights_while_another_language_is_registered(
    cx: &mut gpui::TestAppContext,
) {
    use crate::highlighter::compiled::Waiter;

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    let theme = HighlightTheme::default_dark();
    compiled_language("json").unwrap();
    let code = SharedString::from("[1, \"two\"]");
    highlight_code_block("json", &code, true, &theme, Some(&waiter));
    cx.run_until_parked();
    let made = highlight_code_block("json", &code, true, &theme, Some(&waiter));
    assert!(!made.is_pending());
    assert!(!made.is_empty());

    register_fresh_json("registered-beside-test");
    let shown = highlight_code_block("json", &code, true, &theme, Some(&waiter));
    assert!(!shown.is_pending(), "nothing is highlighted again");
    assert_eq!(shown.to_vec(), made.to_vec());
}

/// A pattern can capture a node by what a later line looks like, which a
/// chunk can miss. A block that streamed in is highlighted from scratch
/// once more when it is complete.
#[cfg(feature = "tree-sitter-languages")]
#[gpui::test]
fn a_complete_block_is_highlighted_as_if_it_arrived_whole(cx: &mut gpui::TestAppContext) {
    use crate::highlighter::compiled::Waiter;

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    let theme = HighlightTheme::default_dark();
    let cases = [
        (
            "elixir",
            "defmodule A do\n  defp helper(\n         a,\n         b\n       ),\n       do: a + b\n  def map, do: %{a: 1, \"b\" => :atom}\nend\n",
            3,
        ),
        (
            "haskell",
            "module Main where\nf ::\n  Int\n  -> Int\nf x =\n  x + 1\nmain = print (f 1)\n",
            12,
        ),
    ];
    for (lang, code, chunk) in cases {
        compiled_language(lang).unwrap();
        let mut end = 0;
        while end < code.len() {
            end = (end + chunk).min(code.len());
            let prefix = SharedString::from(code[..end].to_string());
            highlight_code_block(lang, &prefix, false, &theme, Some(&waiter));
            cx.run_until_parked();
        }
        let code = SharedString::from(code);
        let streamed = highlight_code_block(lang, &code, false, &theme, Some(&waiter));
        assert!(!streamed.is_pending());
        assert!(
            background_job(&code)
                .lock()
                .unwrap()
                .highlighter
                .as_ref()
                .unwrap()
                .incremental
        );

        let complete = highlight_code_block(lang, &code, true, &theme, Some(&waiter));
        assert!(complete.is_pending(), "{lang}: highlighted once more");
        cx.run_until_parked();
        let complete = highlight_code_block(lang, &code, true, &theme, Some(&waiter));
        assert!(!complete.is_pending());
        assert_eq!(
            per_byte(code.len(), &complete),
            from_scratch(lang, &code, &theme),
            "{lang}"
        );
        assert!(
            !background_job(&code)
                .lock()
                .unwrap()
                .highlighter
                .as_ref()
                .unwrap()
                .incremental
        );
    }
}

/// Markdown's inline text over a long block is parsed in groups in the
/// background, where it used to be parsed again as a whole with every chunk
/// and dropped when that took too long.
#[cfg(feature = "tree-sitter-languages")]
#[gpui::test]
fn a_long_markdown_block_keeps_its_inline_highlights_in_the_background(
    cx: &mut gpui::TestAppContext,
) {
    use crate::highlighter::compiled::{Waiter, layer_query};

    let waiter = Waiter::new(cx.background_executor.clone(), None);
    let theme = HighlightTheme::default_dark();
    compiled_language("markdown").unwrap();
    layer_query("markdown_inline").unwrap();
    let section = |ix: usize| {
        format!("## Section {ix}\n\nSome *text* with `code {ix}` and a [link](http://x/{ix}).\n\n")
    };
    let mut code = (0..400).map(section).collect::<String>();
    highlight_code_block(
        "markdown",
        &code.clone().into(),
        false,
        &theme,
        Some(&waiter),
    );
    cx.run_until_parked();
    for ix in 400..404 {
        code.push_str(&section(ix));
        highlight_code_block(
            "markdown",
            &code.clone().into(),
            false,
            &theme,
            Some(&waiter),
        );
        cx.run_until_parked();
    }
    let styles = highlight_code_block(
        "markdown",
        &code.clone().into(),
        false,
        &theme,
        Some(&waiter),
    );
    assert!(!styles.is_pending());
    let bytes = per_byte(code.len(), &styles);
    let first_code = code.find("`code 0`").unwrap() + 1;
    let last_code = code.rfind("`code 403`").unwrap() + 1;
    assert_ne!(bytes[first_code], HighlightStyle::default());
    assert_eq!(
        bytes[last_code], bytes[first_code],
        "the last code span too"
    );

    let job = background_job(&code);
    let job = job.lock().unwrap();
    let inline_layers = job
        .highlighter
        .as_ref()
        .unwrap()
        .layers
        .iter()
        .filter(|layer| layer.language_name.as_ref() == "markdown_inline")
        .count();
    assert!(inline_layers > 4, "{inline_layers} inline layers");
}
