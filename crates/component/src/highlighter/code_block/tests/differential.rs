//! Streams code in every bundled language through a [`CodeBlockHighlighter`]
//! in many chunkings and compares every step with a highlight of the same
//! text from scratch. A resolver that gives every highlight name its own
//! color tells apart names that a theme colors the same.

use std::{collections::HashMap, ops::Range, sync::Mutex};

use gpui::{HighlightStyle, Hsla, SharedString};
use gpui_base::input::HighlightStyleResolver;
use ropey::Rope;

use super::super::{CodeBlockHighlighter, KEPT_BLOCKS, highlight_code_block};
use crate::highlighter::{
    HighlightTheme, SyntaxHighlighter,
    compiled::compiled_language,
    highlighter::{UnbudgetedInjectionParses, merge_highlight_style},
};

/// Gives every highlight name its own color, so a different name is a
/// different style even where a real theme colors both the same.
#[derive(Default)]
struct Unique(Mutex<HashMap<String, usize>>);

impl HighlightStyleResolver for Unique {
    fn style(&self, name: &str) -> Option<HighlightStyle> {
        let mut map = self.0.lock().unwrap();
        let next = map.len();
        let id = *map.entry(name.to_string()).or_insert(next);
        Some(HighlightStyle {
            color: Some(Hsla {
                h: id as f32 / 4096.,
                s: 0.5,
                l: 0.5,
                a: 1.,
            }),
            ..Default::default()
        })
    }
}

fn per_byte(len: usize, styles: &[(Range<usize>, HighlightStyle)]) -> Vec<HighlightStyle> {
    let mut bytes = vec![HighlightStyle::default(); len];
    for (range, style) in styles {
        for byte in &mut bytes[range.clone()] {
            *byte = *style;
        }
    }
    bytes
}

fn reference(lang: &str, code: &str, r: &dyn HighlightStyleResolver) -> Vec<HighlightStyle> {
    let rope = Rope::from_str(code);
    let mut h = SyntaxHighlighter::new(lang);
    h.update(None, &rope, None);
    per_byte(code.len(), &h.styles(&(0..code.len()), r))
}

/// The incremental highlighter's styles with resolver `r`, checking the
/// segment invariants on the way.
fn incremental(
    h: &CodeBlockHighlighter,
    r: &dyn HighlightStyleResolver,
) -> Result<Vec<HighlightStyle>, String> {
    let mut bytes = vec![HighlightStyle::default(); h.len()];
    let mut last = 0;
    for seg in &h.segments {
        if seg.range.start != last || seg.range.end < seg.range.start {
            return Err(format!(
                "segments not contiguous at {last}: {:?}",
                seg.range
            ));
        }
        last = seg.range.end;
        let mut style = HighlightStyle::default();
        for name in &h.stacks[seg.stack as usize] {
            if let Some(s) = r.style(name.name().as_ref()) {
                merge_highlight_style(&mut style, &s);
            }
        }
        for b in &mut bytes[seg.range.clone()] {
            *b = style;
        }
    }
    if last != h.len() {
        return Err(format!("segments end at {last} of {}", h.len()));
    }
    Ok(bytes)
}

fn describe(
    lang: &str,
    code: &str,
    expected: &[HighlightStyle],
    actual: &[HighlightStyle],
) -> Option<String> {
    let ix = expected.iter().zip(actual).position(|(e, a)| e != a)?;
    let mut start = ix.saturating_sub(30);
    while !code.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (ix + 30).min(code.len());
    while !code.is_char_boundary(end) {
        end += 1;
    }
    Some(format!(
        "{lang}: byte {ix} of {} differs (expected color {:?}, actual {:?}) around {:?}",
        code.len(),
        expected[ix].color.map(|c| (c.h * 4096.).round()),
        actual[ix].color.map(|c| (c.h * 4096.).round()),
        &code[start..end]
    ))
}

/// Streams `code` through the chunk ends `ends` and compares every step
/// with a highlight from scratch, with unique colors per name and with
/// the dark theme. Returns the first difference.
fn stream_check(lang: &str, code: &str, ends: &[usize]) -> Result<usize, String> {
    let _budgets = UnbudgetedInjectionParses::new();
    let unique = Unique::default();
    let dark = HighlightTheme::default_dark();
    let language = compiled_language(lang).ok_or(format!("{lang}: no grammar"))?;
    let mut h: Option<CodeBlockHighlighter> = None;
    let mut steps = 0;
    for &end in ends {
        let prefix = SharedString::from(code[..end].to_string());
        let h = match &mut h {
            Some(h) => {
                h.update(&prefix);
                h
            }
            None => h.insert(
                CodeBlockHighlighter::new(language.clone(), &prefix, None).ok_or("no parser")?,
            ),
        };
        steps += 1;
        let actual = incremental(h, &unique).map_err(|e| format!("{lang}: {e}"))?;
        let expected = reference(lang, &prefix, &unique);
        if let Some(d) = describe(lang, &prefix, &expected, &actual) {
            return Err(format!("step {steps} (end {end}): {d}"));
        }
        // And the real theme through the public path.
        let real = per_byte(end, &h.styles(&dark));
        let expected_real = reference(lang, &prefix, &*dark);
        if let Some(d) = describe(lang, &prefix, &expected_real, &real) {
            return Err(format!("step {steps} (end {end}) [dark theme]: {d}"));
        }
    }
    Ok(steps)
}

fn random_ends(code: &str, seed: u64, max: usize) -> Vec<usize> {
    let mut state = seed;
    let mut ends = Vec::new();
    let mut end = 0;
    while end < code.len() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        end = (end + 1 + (state >> 33) as usize % max).min(code.len());
        while !code.is_char_boundary(end) {
            end += 1;
        }
        ends.push(end);
    }
    ends
}

fn char_ends(code: &str) -> Vec<usize> {
    code.char_indices()
        .skip(1)
        .map(|(ix, _)| ix)
        .chain([code.len()])
        .collect()
}

/// Ends right after every `\r` and every `\n`, plus random ones.
fn crlf_ends(code: &str, seed: u64) -> Vec<usize> {
    let mut ends: Vec<usize> = code
        .match_indices(['\r', '\n'])
        .map(|(ix, _)| ix + 1)
        .chain(random_ends(code, seed, 30))
        .collect();
    ends.sort();
    ends.dedup();
    ends
}

fn run_cases(cases: &[(&str, String)], ends: impl Fn(&str) -> Vec<Vec<usize>>) -> Vec<String> {
    let mut failures = Vec::new();
    for (lang, code) in cases {
        for e in ends(code) {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                stream_check(lang, code, &e)
            })) {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => {
                    failures.push(err);
                    break;
                }
                Err(p) => {
                    failures.push(format!(
                        "{lang}: PANIC {:?}",
                        p.downcast_ref::<String>()
                            .cloned()
                            .or(p.downcast_ref::<&str>().map(|s| s.to_string()))
                    ));
                    break;
                }
            }
        }
    }
    failures
}

const KOTLIN: &str = "// comment\npackage demo\n/** doc */\ndata class Point(val x: Int, val y: Int = 0)\nfun main(args: Array<String>) {\n    val p = Point(1)\n    println(\"x=${p.x} ${'$'}\")\n    when (p.x) { 1 -> println(\"one\") else -> {} }\n}\n";
const JAVA: &str = "package demo;\nimport java.util.*;\n/** Doc. */\n@SuppressWarnings(\"unused\")\npublic class Main<T> {\n  private static final int N = 0x1F;\n  public static void main(String[] args) {\n    // comment\n    var list = new ArrayList<String>();\n    String s = \"\"\"\n      text block\n      \"\"\";\n    for (int i = 0; i < N; i++) { list.add(\"x\" + i); }\n  }\n}\n";
const SWIFT: &str = "import Foundation\n/// Doc\nstruct Point: Codable { var x: Int; let y = 2.5 }\nfunc greet(_ name: String) -> String {\n    // comment\n    return \"Hello, \\(name)!\"\n}\nlet multi = \"\"\"\n    multi \\(1 + 2)\n    \"\"\"\nprint(greet(\"world\"), multi)\n";
const SCALA: &str = "package demo\n// line comment\n/* block\n comment */\nobject Main extends App {\n  /** doc */\n  case class Point(x: Int, y: Int = 0)\n  val s = s\"x = ${Point(1).x}\"\n  def f[A](xs: List[A]): Int = xs match { case Nil => 0; case _ :: t => 1 + f(t) }\n  println(s)\n}\n";
const ZIG: &str = "const std = @import(\"std\");\n// comment\npub fn main() !void {\n    const stdout = std.io.getStdOut().writer();\n    var i: u32 = 0;\n    while (i < 10) : (i += 1) {\n        try stdout.print(\"{d}\\n\", .{i});\n    }\n    const s =\n        \\\\multiline\n        \\\\string\n    ;\n    _ = s;\n}\n";
const ELIXIR: &str = "defmodule Greeter do\n  @moduledoc \"\"\"\n  Docs here.\n  \"\"\"\n  # comment\n  def greet(name) when is_binary(name), do: \"Hello, #{name}!\"\n  defp private(x), do: x * 2\n  def sigil, do: ~r/ab+c/i\n  def map, do: %{a: 1, \"b\" => :atom}\nend\n";
const CSHARP: &str = "using System;\nnamespace Demo {\n  /// <summary>Doc</summary>\n  public record Point(int X, int Y);\n  class Program {\n    static void Main(string[] args) {\n      // comment\n      var s = $\"x = {args.Length}\";\n      var raw = @\"C:\\path\";\n      Console.WriteLine(s + raw);\n    }\n  }\n}\n";
const CMAKE: &str = "cmake_minimum_required(VERSION 3.20)\nproject(demo LANGUAGES CXX)\n# comment\nset(SOURCES main.cpp util.cpp)\nif(WIN32)\n  add_definitions(-DWIN)\nendif()\nadd_executable(${PROJECT_NAME} ${SOURCES})\n";
const MAKE: &str = "CC := gcc\nCFLAGS = -O2 -Wall\n# comment\n.PHONY: all clean\nall: main\nmain: main.o util.o\n\t$(CC) $(CFLAGS) -o $@ $^\n%.o: %.c\n\t$(CC) -c $< -o $@\nclean:\n\trm -f *.o main\n";
const GRAPHQL: &str = "# comment\nquery GetUser($id: ID!) {\n  user(id: $id) {\n    name\n    ... on Admin { level }\n    posts(first: 10) @include(if: true) { edges { node { title } } }\n  }\n}\ntype User { id: ID! name: String \"\"\"doc\"\"\" age: Int }\n";
const PROTO: &str = "syntax = \"proto3\";\npackage demo;\n// comment\nmessage User {\n  string name = 1;\n  repeated int32 ids = 2 [packed = true];\n  oneof kind { string a = 3; bytes b = 4; }\n}\nservice Svc { rpc Get(User) returns (User); }\n";
const SVELTE: &str = "<script>\n  let count = 0;\n  // comment\n  function inc() { count += 1; }\n</script>\n\n<button on:click={inc}>Clicked {count} {count === 1 ? 'time' : 'times'}</button>\n{#if count > 3}<p>Big</p>{/if}\n<style>\n  button { color: red; }\n</style>\n";
const ASTRO: &str = "---\nconst title = \"Hello\";\nconst items = [1, 2, 3];\n---\n<html>\n  <head><title>{title}</title></head>\n  <body>\n    <ul>{items.map((i) => <li>{i}</li>)}</ul>\n    <script>console.log(\"hi\");</script>\n    <style>ul { margin: 0; }</style>\n  </body>\n</html>\n";
const EJS: &str = "<ul>\n<% users.forEach(function(user) { %>\n  <li class=\"user\"><%= user.name %></li>\n<% }); %>\n</ul>\n<%# comment %>\n<p><%- html %></p>\n";
const ERB: &str = "<ul>\n<% @users.each do |user| %>\n  <li class=\"user\"><%= user.name %></li>\n<% end %>\n</ul>\n<%# comment %>\n<p><%= link_to \"Home\", root_path %></p>\n";
const TSX: &str = "import React from 'react';\n// comment\ntype Props = { name: string };\nexport function Hello({ name }: Props) {\n  const [n, setN] = React.useState<number>(0);\n  return (\n    <div className=\"hello\" onClick={() => setN(n + 1)}>\n      Hello, {name}! {n}\n    </div>\n  );\n}\n";
const DART: &str = "// comment\nimport 'dart:math';\n/// Doc\nclass Point {\n  final int x;\n  final int y;\n  const Point(this.x, [this.y = 0]);\n  String greet() => 'hi $x ${y + 1}';\n}\nvoid main() async {\n  var p = Point(1);\n  await Future.delayed(Duration(seconds: 1));\n  print(p.greet());\n}\n";
const ELM: &str = "module Main exposing (main)\n-- comment\nimport Html exposing (text)\n{-| Doc -}\ntype Msg = Inc | Dec\nupdate : Msg -> Int -> Int\nupdate msg n =\n    case msg of\n        Inc -> n + 1\n        Dec -> n - 1\nmain = text \"hi\"\n";
const ERLANG: &str = "% comment\n-module(hello).\n-export([greet/1, loop/0]).\n%% @doc Greets.\ngreet(Name) when is_list(Name) ->\n    io:format(\"Hello, ~s~n\", [Name]);\ngreet(_) -> {error, badarg}.\nloop() ->\n    receive\n        {msg, M} -> io:format(\"~p~n\", [M]), loop();\n        stop -> ok\n    end.\n";
const FSHARP: &str = "// comment\nmodule Demo\n/// Doc\ntype Point = { X: int; Y: int }\nlet square x = x * x\nlet rec fact n = if n = 0 then 1 else n * fact (n - 1)\n(* block *)\nlet main () =\n    let p = { X = 1; Y = 2 }\n    printfn \"%d %s\" (square p.X) $\"interp {p.Y}\"\n";
const GLSL: &str = "#version 330 core\n// comment\nuniform vec4 color;\nin vec2 uv;\nout vec4 frag;\nvoid main() {\n  vec3 c = texture(tex, uv).rgb * 2.0;\n  if (c.r > 0.5) { discard; }\n  frag = vec4(c, 1.0) * color;\n}\n";
const HASKELL: &str = "{-# LANGUAGE OverloadedStrings #-}\nmodule Main where\n-- comment\nimport Data.List (sort)\n-- | Doc\ndata Point = Point { px :: Int, py :: Int } deriving (Show, Eq)\nmain :: IO ()\nmain = do\n  let xs = [3, 1, 2] :: [Int]\n  putStrLn $ \"sorted: \" ++ show (sort xs)\n  where helper x = x * 2\n";
const HCL: &str = "# comment\nterraform {\n  required_version = \">= 1.0\"\n}\nvariable \"region\" { default = \"us-east-1\" }\nresource \"aws_instance\" \"web\" {\n  count = 2\n  ami   = \"ami-${var.region}\"\n  tags  = { Name = \"web-${count.index}\" }\n  /* block */\n}\n";
const JULIA: &str = "# comment\nmodule Demo\n\"\"\"\nDoc string\n\"\"\"\nfunction add(x::Int, y)\n    return x + y\nend\nstruct Point{T}\n    x::T\nend\nprintln(\"sum: $(add(1, 2)) \", [i^2 for i in 1:10])\nend\n";
const OBJC: &str = "// comment\n#import <Foundation/Foundation.h>\n@interface Greeter : NSObject\n@property (nonatomic, copy) NSString *name;\n- (void)greet;\n@end\n@implementation Greeter\n- (void)greet {\n  NSLog(@\"Hello, %@\", self.name);\n}\n@end\n";
const OCAML: &str = "(* comment *)\nmodule M = struct\n  type t = { x : int; y : int }\n  let rec fact n = if n = 0 then 1 else n * fact (n - 1)\nend\nlet () =\n  let p = M.{ x = 1; y = 2 } in\n  Printf.printf \"%d %s\\n\" (M.fact p.x) \"done\"\n";
const PERL: &str = "#!/usr/bin/perl\n# comment\nuse strict;\nuse warnings;\nmy $name = \"world\";\nmy @list = (1, 2, 3);\nsub greet { my ($n) = @_; return \"Hello, $n!\"; }\nif ($name =~ /^w(or)ld$/i) { print greet($name), \"\\n\"; }\n";
const POWERSHELL: &str = "# comment\nparam([string]$Name = \"world\")\n$greeting = \"Hello, $Name\"\nfunction Get-Thing {\n    [CmdletBinding()]\n    param($x)\n    Write-Host $greeting -ForegroundColor Green\n    foreach ($i in 1..3) { $i * 2 }\n}\n<# block #>\nGet-Thing -x 1 | Out-Null\n";
const R: &str = "# comment\nlibrary(stats)\nsquare <- function(x) { x^2 }\ndf <- data.frame(a = 1:3, b = c(\"x\", \"y\", \"z\"))\nfor (i in seq_len(nrow(df))) {\n  if (i %% 2 == 0) print(square(i)) else cat(\"odd\\n\")\n}\n";
const XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- comment -->\n<!DOCTYPE note>\n<root xmlns:a=\"urn:a\">\n  <a:item id=\"1\" enabled=\"true\">Text &amp; more</a:item>\n  <![CDATA[ raw <data> ]]>\n  <empty/>\n</root>\n";

fn all_cases() -> Vec<(&'static str, String)> {
    let v: Vec<(&str, &str)> = vec![
        ("rust", super::RUST),
        ("typescript", super::TYPESCRIPT),
        ("python", super::PYTHON),
        ("html", super::HTML),
        ("markdown", super::MARKDOWN),
        ("json", super::JSON),
        ("bash", super::BASH),
        ("css", super::CSS),
        ("go", super::GO),
        ("yaml", super::YAML),
        ("php", super::PHP),
        ("lua", super::LUA),
        ("c", super::C),
        ("cpp", super::C),
        ("ruby", super::RUBY),
        ("sql", super::SQL),
        ("toml", super::TOML),
        ("diff", super::DIFF),
        ("kotlin", KOTLIN),
        ("java", JAVA),
        ("swift", SWIFT),
        ("scala", SCALA),
        ("zig", ZIG),
        ("elixir", ELIXIR),
        ("csharp", CSHARP),
        ("cmake", CMAKE),
        ("make", MAKE),
        ("graphql", GRAPHQL),
        ("proto", PROTO),
        ("svelte", SVELTE),
        ("astro", ASTRO),
        ("ejs", EJS),
        ("erb", ERB),
        ("tsx", TSX),
        ("dart", DART),
        ("elm", ELM),
        ("erlang", ERLANG),
        ("fsharp", FSHARP),
        ("glsl", GLSL),
        ("haskell", HASKELL),
        ("hcl", HCL),
        ("julia", JULIA),
        ("objc", OBJC),
        ("ocaml", OCAML),
        ("perl", PERL),
        ("powershell", POWERSHELL),
        ("r", R),
        ("xml", XML),
    ];
    v.into_iter().map(|(l, c)| (l, c.to_string())).collect()
}

#[test]
fn every_language_random_chunks_unique_names() {
    let failures = run_cases(&all_cases(), |code| {
        (1..=4u64).map(|seed| random_ends(code, seed, 48)).collect()
    });
    println!("random chunks: {} failures", failures.len());
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty());
}

#[test]
fn every_language_char_by_char_unique_names() {
    let failures = run_cases(&all_cases(), |code| vec![char_ends(code)]);
    println!("char by char: {} failures", failures.len());
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty());
}

#[test]
fn crlf() {
    let cases: Vec<(&str, String)> = all_cases()
        .into_iter()
        .map(|(l, c)| (l, c.replace('\n', "\r\n")))
        .collect();
    let failures = run_cases(&cases, |code| vec![crlf_ends(code, 5), char_ends(code)]);
    println!("crlf: {} failures", failures.len());
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty());
}

/// A loaded machine can hold a thread up while one of the two highlights
/// compared parses Markdown's inline text, which then runs out of its time
/// and is left out of that highlight alone. The comparison doesn't depend on
/// the time a parse takes.
#[test]
fn a_thread_held_up_while_a_layer_parses_compares_the_same() {
    use crate::highlighter::highlighter::{
        injection_parse_stall_pending, stall_next_injection_parse,
    };

    let code = super::MARKDOWN.replace('\n', "\r\n");
    stall_next_injection_parse("markdown_inline", std::time::Duration::from_secs(1));
    let compared = stream_check("markdown", &code, &char_ends(&code));
    assert!(
        !injection_parse_stall_pending(),
        "a parse of the inline text was held up"
    );
    compared.unwrap();
}

const DENSE_MULTIBYTE_PY: &str = "# 注释 — ünïcödé 👩‍👩‍👧‍👦 é (e\u{301})\ndef 函数(名字: str) -> str:\n    \"\"\"文档 🎉\"\"\"\n    s = f\"你好, {名字}! 🚀 {'ü' * 3}\"\n    return s + '日本語' + r'\\u00e9'\nprint(函数(\"世界\"), b\"bytes\", 'ß', \"\u{2028}sep\u{2029}\")\n";
const DENSE_MULTIBYTE_MD: &str = "# 标题 🎉\n\n文本 *强调* **粗体** `代码 ü` [链接](https://例子.com/路径).\n\n- 项目 👩‍💻\n- 另一个 _项目_\n\n```python\nprint(\"你好 🚀\")\n```\n\n> 引用 — ünïcödé\n\n| 表 | 头 |\n|---|---|\n| 单元 🎉 | é |\n";
const DENSE_MULTIBYTE_HTML: &str = "<p title=\"标题 🎉\">文本 &amp; ünïcödé</p>\n<script>const s = \"你好 🚀\"; // 注释 é\nconsole.log(`${s} 日本`);</script>\n<style>.类 { content: \"✓\"; }</style>\n";
const DENSE_MULTIBYTE_JSON: &str =
    "{\"名字\": \"世界 🌍\", \"emoji\": [\"👩‍👩‍👧‍👦\", \"\\u00e9\", \"é\"], \"ß\": {\"日本\": true}}\n";

#[test]
fn multibyte_char_by_char_and_random() {
    let cases: Vec<(&str, String)> = vec![
        ("python", DENSE_MULTIBYTE_PY.to_string()),
        ("markdown", DENSE_MULTIBYTE_MD.to_string()),
        ("html", DENSE_MULTIBYTE_HTML.to_string()),
        ("json", DENSE_MULTIBYTE_JSON.to_string()),
        ("rust", super::MULTIBYTE.to_string()),
        (
            "yaml",
            "名前: 世界 🌍\nリスト:\n  - ü\n  - \"é 🚀\"\n# コメント\n".to_string(),
        ),
    ];
    let failures = run_cases(&cases, |code| {
        let mut v = vec![char_ends(code)];
        v.extend((1..=3u64).map(|s| random_ends(code, s, 7)));
        v
    });
    println!("multibyte: {} failures", failures.len());
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty());
}

#[test]
fn very_long_lines() {
    let mut json = String::from("[");
    for i in 0..300 {
        json.push_str(&format!(
            "{{\"id\": {i}, \"name\": \"n{i}\", \"ok\": true}}, "
        ));
    }
    json.push_str("null]\n");
    let mut js = String::new();
    for i in 0..160 {
        js.push_str(&format!("var a{i}=function(b){{return b*{i}+\"s{i}\"}};"));
    }
    js.push('\n');
    let mut rust = String::from("fn main() { let v = vec![");
    for i in 0..300 {
        rust.push_str(&format!("{i}, "));
    }
    rust.push_str("]; }\n");
    let cases = vec![("json", json), ("javascript", js), ("rust", rust)];
    let failures = run_cases(&cases, |code| vec![random_ends(code, 9, 48)]);
    println!("long lines: {} failures", failures.len());
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty());
}

#[test]
fn markdown_fences_change_close_reopen_and_nest() {
    let cases: Vec<(&str, String)> = vec![
        // The info string is typed: r → ru → rus → rust (R, then nothing, then Rust).
        ("markdown", "Intro\n\n```rust\nfn main() { let s = \"x\"; }\n```\n\nThen:\n\n```python\ndef f(): return 1\n```\n\n```\nplain\n```\n\n```js\nconst a = `t ${1}`;\n```\n".to_string()),
        // A fence that closes, text, the same fence reopened in another language, never closed.
        ("markdown", "```json\n{\"a\": 1}\n```\ntext *a*\n```json5\n{a: 1}\n``\n`\n```yaml\nk: v\n".to_string()),
        // Nested: Markdown in a Markdown fence with a Rust fence, HTML block with script.
        ("markdown", "# Doc\n\n````markdown\n# Inner\n\n```rust\nfn inner() {}\n```\n````\n\n<div>\n<script>\nlet x = 1;\n</script>\n</div>\n\n---\n\nEnd `code` here.\n".to_string()),
        // Front matter, then a long list.
        ("markdown", "---\ntitle: \"T\"\ntags: [a, b]\n---\n\n- one *1*\n- two `2`\n  - nested **3**\n1. first\n2. second\n".to_string()),
        // Rust with macros (Rust injected into Rust), nested macros.
        ("rust", "fn main() {\n    println!(\"{}\", vec![1, 2, format!(\"{}\", 3).len()].len());\n    assert_eq!(m!(a, b), n!{ c => d });\n    macro_rules! m { ($a:expr, $b:expr) => { $a + $b }; }\n}\n".to_string()),
        // HTML with script containing a template literal with HTML, style.
        ("html", "<html><body>\n<script type=\"module\">\nconst t = `<b>${1}</b>`;\n/* c */ function f() { return 1 }\n</script>\n<style>\n@media (x) { a { b: c } }\n</style>\n<script>\nlet y = 2;\n</script>\n</body></html>\n".to_string()),
        // PHP mixing HTML and PHP repeatedly.
        ("php", "<?php $a = 1; ?>\n<div><?= $a ?></div>\n<?php if ($a): ?>\n<p>yes</p>\n<?php else: ?>\n<p>no <?php echo \"x\"; ?></p>\n<?php endif; ?>\n<script>let z = <?= json_encode($a) ?>;</script>\n".to_string()),
    ];
    let failures = run_cases(&cases, |code| {
        let mut v = vec![char_ends(code)];
        v.extend((1..=4u64).map(|s| random_ends(code, s, 20)));
        v
    });
    println!("fences/nesting: {} failures", failures.len());
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty());
}

#[test]
fn code_that_never_stops_being_an_error_tree() {
    let rust = super::RUST.repeat(8);
    let cases: Vec<(&str, String)> = vec![
        // A stray closer first: error recovery for the rest.
        ("rust", format!("}}}}) ]\n{rust}")),
        // Unclosed raw string near the start, then code.
        (
            "rust",
            format!("let s = r#\"open\n{}", super::RUST.repeat(3)),
        ),
        (
            "python",
            format!("    def broken(:\n{}", super::PYTHON.repeat(8)),
        ),
        (
            "html",
            format!("</div></span><<<\n{}", super::HTML.repeat(4)),
        ),
        (
            "cpp",
            format!("template<typename T\n{}", super::C.repeat(10)),
        ),
        (
            "typescript",
            format!("))}}{{\n{}", super::TYPESCRIPT.repeat(4)),
        ),
        (
            "json",
            format!("{{\"a\": [1, 2,, {{ \"b\" \n{}", super::JSON.repeat(10)),
        ),
        ("markdown", format!("```\n{}", super::MARKDOWN.repeat(4))),
    ];
    let failures = run_cases(&cases, |code| {
        (1..=3u64).map(|s| random_ends(code, s, 48)).collect()
    });
    println!("error trees: {} failures", failures.len());
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty());
}

// ------------------------------------------------------------------
// Block matching through the thread-local kept blocks.
// ------------------------------------------------------------------

fn clear_kept() {
    KEPT_BLOCKS.with(|k| k.borrow_mut().blocks.clear());
}

fn check_call(lang: &str, code: &str) -> Option<String> {
    let _budgets = UnbudgetedInjectionParses::new();
    let dark = HighlightTheme::default_dark();
    let code_s = SharedString::from(code.to_string());
    let got = highlight_code_block(lang, &code_s, true, &dark, None);
    let actual = per_byte(code.len(), &got);
    let expected = reference(lang, code, &*dark);
    describe(lang, code, &expected, &actual)
}

#[test]
fn two_blocks_with_one_prefix_and_an_edited_block() {
    clear_kept();
    let a = "fn a() {\n    let s = \"a\";\n}\n";
    let b_full = format!("{a}fn b() {{\n    /* open comment\n    let t = 1;\n}}\n");
    let mut failures = Vec::new();
    // A is complete and drawn every frame, and B streams and starts with A.
    for end in random_ends(&b_full, 3, 6) {
        for code in [a, &b_full[..end]] {
            if let Some(f) = check_call("rust", code) {
                failures.push(f);
            }
        }
    }
    // A block whose middle is edited, then appended to.
    let edited = [
        "fn main() {\n    let x = 1;\n",
        "fn main() {\n    let y = \"two\";\n",
        "fn main() {\n    let y = \"two\";\n    /* c */\n}\n",
        "fn main() {\n    let /* x */ y = \"two\";\n    /* c */\n}\n",
    ];
    for code in edited {
        if let Some(f) = check_call("rust", code) {
            failures.push(f);
        }
    }
    // A shorter block (a fence that closed, dropping a partial "``").
    for code in ["fn x() {}\n``", "fn x() {}\n"] {
        if let Some(f) = check_call("rust", code) {
            failures.push(f);
        }
    }
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty());
}

#[test]
fn eviction_keeps_the_block_in_use_and_results_stay_right() {
    clear_kept();
    let mut failures = Vec::new();
    for i in 0..80 {
        let code = format!("fn f{i}() {{ let s = \"{i}\"; }}\n");
        if let Some(f) = check_call("rust", &code) {
            failures.push(f);
        }
    }
    let n = KEPT_BLOCKS.with(|k| k.borrow().blocks.len());
    println!("kept after 80 blocks: {n}");
    assert!(n <= 64);
    // The first is gone, and drawing it again is right, from scratch.
    if let Some(f) = check_call("rust", "fn f0() { let s = \"0\"; }\n") {
        failures.push(f);
    }
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty());
}

/// Streams `code` in every chunk size from 1 to 12 bytes and returns the
/// first step whose highlights differ from a highlight from scratch.
fn first_stale_step(lang: &str, code: &str) -> Option<String> {
    let _budgets = UnbudgetedInjectionParses::new();
    let unique = Unique::default();
    let language = compiled_language(lang)?;
    for chunk in 1..=12usize {
        let mut highlighter: Option<CodeBlockHighlighter> = None;
        let mut end = 0;
        while end < code.len() {
            end = (end + chunk).min(code.len());
            while !code.is_char_boundary(end) {
                end += 1;
            }
            let prefix = SharedString::from(code[..end].to_string());
            let highlighter = match &mut highlighter {
                Some(highlighter) => {
                    highlighter.update(&prefix);
                    highlighter
                }
                None => {
                    highlighter.insert(CodeBlockHighlighter::new(language.clone(), &prefix, None)?)
                }
            };
            let actual = incremental(highlighter, &unique).ok()?;
            let expected = reference(lang, &prefix, &unique);
            if let Some(difference) = describe(lang, &prefix, &expected, &actual) {
                return Some(format!("chunks of {chunk}, at {end}: {difference}"));
            }
        }
    }
    None
}

/// A pattern can capture a node by what a later sibling's descendant looks
/// like, on another line, as Elixir's `defp` by the `arguments` that follow.
#[test]
fn a_capture_that_depends_on_a_later_line_is_highlighted_again() {
    let long_args: String = (0..60).map(|i| format!("a{i}, ")).collect();
    let cases: Vec<(&str, String)> = vec![
        ("elixir", "defmodule A do\n  def greet(name)\n      when is_binary(name) do\n    \"hi\"\n  end\nend\n".into()),
        ("elixir", format!("defmodule A do\n  def greet({long_args}name) when is_binary(name) do\n    \"hi\"\n  end\nend\n")),
        ("elixir", "defmodule A do\n  defp helper(\n         a,\n         b\n       ),\n       do: a + b\n  def map, do: %{a: 1, \"b\" => :atom}\nend\n".into()),
        ("typescript", "const handler =\n  (event: Event) =>\n    console.log(event);\nconst other = function\n  (x) { return x; };\n".into()),
        ("javascript", format!("const handler = ({}) => 1;\n", (0..60).map(|i| format!("p{i}")).collect::<Vec<_>>().join(", "))),
        ("rust", format!("fn main() {{\n    let f = |{}| x;\n    foo\n        .bar()\n        .baz::<u8>(1);\n}}\n", (0..60).map(|i| format!("a{i}: u8")).collect::<Vec<_>>().join(", "))),
        ("python", "@decorator\ndef f(\n    a,\n    b,\n):\n    return (\n        a\n        + b\n    )\nclass C(\n    Base,\n):\n    pass\n".into()),
        ("ruby", "def greet(\n  name,\n  greeting: \"hi\"\n)\n  puts \"#{greeting} #{name}\"\nend\nfoo\n  .bar\n  .baz(1)\n".into()),
        ("haskell", "module Main where\nf ::\n  Int\n  -> Int\nf x =\n  x + 1\nmain = print (f 1)\n".into()),
        ("ocaml", "let rec fact n =\n  if n = 0\n  then 1\n  else n * fact (n - 1)\nlet () =\n  print_int\n    (fact 5)\n".into()),
        ("fsharp", "let square\n    x =\n    x * x\ntype T =\n    { A: int }\n".into()),
        ("scala", "object Main {\n  def f[A](\n    xs: List[A]\n  ): Int =\n    xs.size\n}\n".into()),
        ("kotlin", "fun greet(\n    name: String,\n): String =\n    \"hi $name\"\nval f = { x: Int ->\n    x * 2\n}\n".into()),
        ("go", "func (g *Greeter)\nGreet(\n\tname string,\n) string {\n\treturn name\n}\n".into()),
        ("cpp", "template <typename T>\nstd::vector<T>\nmake(\n    T value)\n{\n    return {value};\n}\n".into()),
        ("css", ".a,\n.b\n  > .c:hover,\n#id\n{\n  color: red;\n}\n".into()),
    ];
    let stale = cases
        .iter()
        .filter_map(|(lang, code)| first_stale_step(lang, code))
        .collect::<Vec<_>>();
    assert!(stale.is_empty(), "{stale:#?}");
}
