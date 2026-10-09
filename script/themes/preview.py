"""Write a self-contained HTML page that previews every theme as a small mock app."""

from __future__ import annotations

import html
import json
from pathlib import Path

import checks
import colors as C
import kit
import validate

# Each code line is a list of (capture, text). The capture names match the kit's highlighter.
CODE = [
    [("keyword", "use"), (None, " std::collections::"), ("type", "HashMap"), ("punctuation.delimiter", ";")],
    [],
    [("comment.doc", "/// Counts how often each word appears.")],
    [("attribute", "#[must_use]")],
    [("keyword", "pub fn "), ("function", "count_words"), ("punctuation.bracket", "("), ("variable", "text"),
     ("punctuation.delimiter", ": "), ("operator", "&"), ("type", "str"), ("punctuation.delimiter", ", "),
     ("variable", "limit"), ("punctuation.delimiter", ": "), ("type", "usize"), ("punctuation.bracket", ")"),
     ("operator", " -> "), ("type", "HashMap"), ("punctuation.bracket", "<"), ("type", "String"),
     ("punctuation.delimiter", ", "), ("type", "u32"), ("punctuation.bracket", ">"), (None, " "),
     ("punctuation.bracket", "{")],
    [(None, "    "), ("keyword", "let mut "), ("variable", "counts"), ("operator", " = "), ("type", "HashMap"),
     ("punctuation.delimiter", "::"), ("function", "new"), ("punctuation.bracket", "()"),
     ("punctuation.delimiter", ";")],
    [(None, "    "), ("keyword", "for "), ("variable", "word"), ("keyword", " in "), ("variable", "text"),
     ("punctuation.delimiter", "."), ("function", "split_whitespace"), ("punctuation.bracket", "()"),
     ("punctuation.delimiter", "."), ("function", "take"), ("punctuation.bracket", "("), ("variable", "limit"),
     ("punctuation.bracket", ")"), (None, " "), ("punctuation.bracket", "{")],
    [(None, "        "), ("operator", "*"), ("variable", "counts"), ("punctuation.delimiter", "."),
     ("function", "entry"), ("punctuation.bracket", "("), ("variable", "word"), ("punctuation.bracket", ")"),
     ("punctuation.delimiter", "."), ("function", "or_insert"), ("punctuation.bracket", "("), ("number", "0"),
     ("punctuation.bracket", ")"), ("operator", " += "), ("number", "1"), ("punctuation.delimiter", ";")],
    [(None, "    "), ("punctuation.bracket", "}")],
    [(None, "    "), ("function", "println!"), ("punctuation.bracket", "("), ("string", '"{} words'),
     ("string.escape", "\\n"), ("string", '"'), ("punctuation.delimiter", ", "), ("variable", "counts"),
     ("punctuation.delimiter", "."), ("property", "len"), ("punctuation.bracket", "()"),
     ("punctuation.bracket", ")"), ("punctuation.delimiter", "; "), ("comment", "// TODO: sort")],
    [(None, "    "), ("variable", "counts")],
    [("punctuation.bracket", "}")],
]
ACTIVE_LINE = 6  # zero-based: the `for` line
SELECTED = (6, "split_whitespace")
MATCHES = [(5, "counts", True), (7, "counts", False), (9, "counts", False), (10, "counts", False)]


def css(c) -> str:
    r, g, b, a = c
    if a >= 0.999:
        return C.to_hex(c)
    return f"rgba({round(r * 255)},{round(g * 255)},{round(b * 255)},{a:.3f})"


def bg_css(tok: kit.Tok) -> str:
    if len(tok.stops) == 2 and tok.stops[0] != tok.stops[1]:
        return f"linear-gradient(180deg,{css(tok.stops[0])},{css(tok.stops[1])})"
    return css(tok.stops[0] if tok.stops else tok.color)


def syntax_style(theme: dict, name: str) -> dict:
    raw = ((theme.get("highlight") or {}).get("syntax")) or {}
    while True:
        key = "comment_doc" if name == "comment.doc" else name
        if key in raw and isinstance(raw[key], dict) and raw[key].get("color"):
            return raw[key]
        if "." not in name:
            return {}
        name = name.split(".")[0]


def theme_data(theme: dict, file_name: str, family: str, new: bool) -> dict:
    r = kit.resolve_theme(theme)
    t, hl = r.tokens, r.highlight
    dark = r.mode == "dark"
    look = lambda ref: checks.lookup(r, ref)  # noqa: E731
    fg = t["foreground"].color
    editor_bg = look("@editor_bg")[0]
    syn = {}
    for _, line in enumerate(CODE):
        for cap, _ in line:
            if cap and cap not in syn:
                c = kit.syntax_color(hl["syntax"], cap)
                style = syntax_style(theme, cap) if c else {}
                syn[cap] = {
                    "c": css(c or fg),
                    "i": style.get("font_style") == "italic",
                    "u": style.get("font_style") == "underline",
                    "b": (style.get("font_weight") or 400) >= 600,
                }
    results = checks.evaluate(r)
    summary = checks.summarize(results)
    fails = [
        {"l": res.check.level, "r": round(res.ratio, 2), "m": res.minimum, "fg": res.fg_hex, "bg": res.bg_hex,
         "t": res.check.label}
        for res in sorted(results, key=lambda x: (checks.LEVEL_ORDER.index(x.check.level), x.ratio))
        if not res.ok
    ]
    v = {
        "bg": bg_css(t["background"]), "fg": css(fg), "muted": css(t["muted.foreground"].color),
        "border": css(t["border"].color),
        "title": bg_css(t["title_bar.background"]), "titleBorder": css(t["title_bar.border"].color),
        "status": bg_css(t["status_bar.background"]), "statusBorder": css(t["status_bar.border"].color),
        "side": bg_css(t["sidebar.background"]), "sideFg": css(t["sidebar.foreground"].color),
        "sideBorder": css(t["sidebar.border"].color), "sideActive": bg_css(t["sidebar.accent.background"]),
        "sideActiveFg": css(t["sidebar.accent.foreground"].color),
        "listHover": bg_css(t["list.hover.background"]), "listActive": bg_css(t["list.active.background"]),
        "tabBar": bg_css(t["tab_bar.background"]), "tab": bg_css(t["tab.background"]),
        "tabFg": css(t["tab.foreground"].color), "tabActive": bg_css(t["tab.active.background"]),
        "tabActiveFg": css(t["tab.active.foreground"].color),
        "editor": css(editor_bg), "gutter": css(look("@gutter")[0]), "gutterBorder": css(look("@gutter_border")[0]),
        "activeLine": css(look("@active_line")[0]) if look("@active_line") else "transparent",
        "selection": css(look("@selection")[0]), "search": css(look("@search")[0]),
        "caret": css(t["caret"].color),
        "pop": bg_css(t["popover.background"]), "popFg": css(t["popover.foreground"].color),
        "popRing": css(C.with_alpha(fg, 0.1)), "hover": bg_css(t["accent.background"]),
        "hoverFg": css(t["accent.foreground"].color),
        "input": css(look("@input_bg")[0]), "inputBorder": css(t["input.border"].color),
        "ring": css(t["ring"].color),
        "btn": bg_css(t["button.background"]), "btnFg": css(t["button.foreground"].color),
        "primary": bg_css(t["button.primary.background"]), "primaryFg": css(t["button.primary.foreground"].color),
        "secondary": bg_css(t["button.secondary.background"]),
        "secondaryFg": css(t["button.secondary.foreground"].color),
        "danger": bg_css(t["button.danger.background"]), "dangerFg": css(t["button.danger.foreground"].color),
        "red": css(t["danger.background"].color), "yellow": css(t["warning.background"].color),
        "green": css(t["success.background"].color), "link": css(t["link"].color),
        "thumb": bg_css(t["scrollbar.thumb.background"]), "invisible": css(look("@invisible")[0]),
        "shadow": theme.get("shadow", True),
    }
    return {
        "name": r.name, "mode": r.mode, "file": file_name, "family": family, "new": new,
        "dark": dark, "v": v, "syn": syn, "summary": summary, "fails": fails,
    }


def collect(files: list[Path], new_names: set[str]) -> list[dict]:
    out = []
    for path in files:
        data, err = validate.load(path)
        if err or not isinstance(data, dict):
            continue
        for theme in data.get("themes") or []:
            out.append(theme_data(theme, path.name, data.get("name", ""), path.name in new_names))
    out.sort(key=lambda d: (not d["new"], d["family"].lower() != "macos", d["family"].lower(), d["file"]))
    return out


def render(themes: list[dict]) -> str:
    code = [[[cap, text] for cap, text in line] for line in CODE]
    payload = json.dumps({
        "themes": themes, "code": code, "active": ACTIVE_LINE, "selected": SELECTED,
        "matches": MATCHES, "levels": checks.LEVELS,
    }, separators=(",", ":"))
    n_new = sum(t["new"] for t in themes)
    new_fail = sum(1 for t in themes if t["new"] and t["fails"])
    old = [t for t in themes if not t["new"]]
    old_fail = sum(1 for t in old if t["fails"])
    intro = (
        f"{n_new} new themes ({'all passing' if not new_fail else f'{new_fail} failing'}) and "
        f"{len(old)} existing ones ({old_fail} with failing checks)."
    )
    return TEMPLATE.replace("/*DATA*/", payload.replace("</", "<\\/")).replace("{{INTRO}}", html.escape(intro))


def cmd_preview(args, files: list[Path]) -> int:
    new_names = set(args.new or [])
    if args.new is None:
        new_names = {p.name for p in files if _is_generated(p)}
    themes = collect(files, new_names)
    Path(args.out).write_text(render(themes))
    print(f"wrote {args.out} ({len(themes)} themes)")
    return 0


def _is_generated(path: Path) -> bool:
    data, err = validate.load(path)
    return not err and isinstance(data, dict) and data.get("author") == "Mesa Hills Research"


TEMPLATE = r"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Theme Preview</title>
<style>
:root {
  --page: #f4f4f5; --card: #ffffff; --ink: #18181b; --ink-2: #52525b; --line: #e4e4e7;
  --chip: #ffffff; --chip-on: #18181b; --chip-on-ink: #ffffff; --ok: #15803d; --bad: #b91c1c;
}
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --page: #111113; --card: #1b1b1f; --ink: #ececef; --ink-2: #a1a1aa; --line: #2e2e33;
    --chip: #1b1b1f; --chip-on: #ececef; --chip-on-ink: #111113; --ok: #4ade80; --bad: #f87171;
  }
}
:root[data-theme="dark"] {
  --page: #111113; --card: #1b1b1f; --ink: #ececef; --ink-2: #a1a1aa; --line: #2e2e33;
  --chip: #1b1b1f; --chip-on: #ececef; --chip-on-ink: #111113; --ok: #4ade80; --bad: #f87171;
}
* { box-sizing: border-box; }
html { -webkit-text-size-adjust: 100%; }
body { margin: 0; background: var(--page); color: var(--ink);
  font: 14px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; }
header { padding: 24px 16px 8px; max-width: 1240px; margin: 0 auto; }
h1 { font-size: 22px; margin: 0 0 4px; letter-spacing: -0.01em; }
header p { margin: 0 0 12px; color: var(--ink-2); }
.filters { display: flex; flex-wrap: wrap; gap: 6px; }
.filters button { font: inherit; font-size: 13px; padding: 4px 12px; border-radius: 999px;
  border: 1px solid var(--line); background: var(--chip); color: var(--ink); cursor: pointer; }
.filters button[aria-pressed="true"] { background: var(--chip-on); color: var(--chip-on-ink); border-color: var(--chip-on); }
.filters .gap { width: 8px; }
main { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 540px), 1fr));
  gap: 16px; padding: 12px 16px 40px; max-width: 1240px; margin: 0 auto; }
.card { background: var(--card); border: 1px solid var(--line); border-radius: 12px; overflow: hidden; min-width: 0; }
.card[hidden] { display: none; }
.head { display: flex; align-items: baseline; gap: 8px; padding: 10px 12px; flex-wrap: wrap; }
.head h2 { font-size: 15px; margin: 0; }
.head .meta { color: var(--ink-2); font-size: 12px; }
.head .tag { margin-left: auto; font-size: 12px; font-weight: 600; }
.ok { color: var(--ok); } .bad { color: var(--bad); }
.sum { padding: 8px 12px 12px; font-size: 12px; color: var(--ink-2); }
.sum .lv { display: inline-block; margin-right: 10px; white-space: nowrap; }
.sum details { margin-top: 6px; }
.sum summary { cursor: pointer; color: var(--ink); }
.sum ul { margin: 6px 0 0; padding: 0; list-style: none; max-height: 220px; overflow: auto; }
.sum li { display: flex; gap: 6px; align-items: center; padding: 2px 0; }
.sw { display: inline-flex; align-items: center; justify-content: center; min-width: 30px; height: 18px; border-radius: 4px;
  font: 600 11px/1 ui-monospace, Menlo, monospace; border: 1px solid var(--line); flex: none; }
.sum li .why { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

/* The mock app. Every color comes from the theme. */
.app { position: relative; display: grid; grid-template-rows: 30px 1fr 22px; height: 360px;
  background: var(--bg); color: var(--fg); font-size: 12px; border-top: 1px solid var(--line);
  border-bottom: 1px solid var(--line); overflow: hidden; }
.app .title { display: flex; align-items: center; gap: 6px; padding: 0 10px; background: var(--title);
  border-bottom: 1px solid var(--titleBorder); }
.app .dot { width: 10px; height: 10px; border-radius: 50%; }
.app .title .name { flex: 1; text-align: center; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.app .title .muted { color: var(--muted); }
.app .body { display: grid; grid-template-columns: clamp(92px, 27%, 150px) 1fr; min-height: 0; }
.app .side { background: var(--side); color: var(--sideFg); border-right: 1px solid var(--sideBorder); padding: 8px 6px; overflow: hidden; }
.app .side .label { color: var(--muted); font-size: 10px; font-weight: 600; letter-spacing: .06em; padding: 2px 6px 4px; }
.app .row { padding: 3px 6px; border-radius: 5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.app .row.active { background: var(--sideActive); color: var(--sideActiveFg); }
.app .row.hover { background: var(--listHover); }
.app .row .ic { display: inline-block; width: 8px; height: 8px; border-radius: 2px; margin-right: 6px; vertical-align: 0; }
.app .mainp { display: grid; grid-template-rows: 26px 1fr auto; min-width: 0; min-height: 0; }
.app .tabs { display: flex; background: var(--tabBar); border-bottom: 1px solid var(--border); }
.app .tabs div { padding: 0 12px; display: flex; align-items: center; background: var(--tab); color: var(--tabFg);
  border-right: 1px solid var(--border); white-space: nowrap; }
.app .tabs div.on { background: var(--tabActive); color: var(--tabActiveFg); }
.app .ed { position: relative; background: var(--editor); font: 11.5px/17px ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  padding: 4px 0; overflow: hidden; white-space: pre; }
.app .ln { display: flex; }
.app .ln.cur { background: var(--activeLine); }
.app .gut { width: 30px; flex: none; text-align: right; padding-right: 7px; margin-right: 7px; color: var(--muted);
  background: var(--gutter); border-right: 1px solid var(--gutterBorder); }
.app .ln.cur .gut { color: var(--fg); }
.app .sel { background: var(--selection); }
.app .m { background: var(--search); }
.app .m.cur { background: var(--search); box-shadow: inset 0 0 0 100px var(--selection); }
.app .caret { display: inline-block; width: 2px; height: 14px; background: var(--caret); vertical-align: -3px; margin-left: -1px; }
.app .thumb { position: absolute; right: 2px; top: 30px; width: 6px; height: 60px; border-radius: 3px; background: var(--thumb); }
.app .panel { border-top: 1px solid var(--border); padding: 8px 10px; display: flex; flex-wrap: wrap; gap: 6px; align-items: center; background: var(--bg); }
.app .btn { padding: 3px 10px; border-radius: 6px; border: 1px solid transparent; white-space: nowrap; }
.app .btn.def { background: var(--btn); color: var(--btnFg); border-color: var(--inputBorder); }
.app .btn.pri { background: var(--primary); color: var(--primaryFg); border-color: var(--primary); }
.app .btn.sec { background: var(--secondary); color: var(--secondaryFg); border-color: var(--border); }
.app .btn.dan { background: var(--danger); color: var(--dangerFg); }
.app.shadow .btn.def, .app.shadow .inp { box-shadow: 0 1px 2px rgba(0,0,0,.12); }
.app .inp { flex: 1 1 110px; min-width: 90px; padding: 3px 8px; border-radius: 6px; background: var(--input);
  border: 1px solid var(--inputBorder); color: var(--muted); white-space: nowrap; overflow: hidden; }
.app .inp.focus { border-color: var(--ring); box-shadow: 0 0 0 2px color-mix(in srgb, var(--ring) 50%, transparent); color: var(--fg); }
.app .link { color: var(--link); text-decoration: underline; }
.app .status { display: flex; align-items: center; gap: 12px; padding: 0 10px; background: var(--status);
  border-top: 1px solid var(--statusBorder); color: var(--muted); font-size: 11px; white-space: nowrap; overflow: hidden; }
.app .status .r { margin-left: auto; }
.app .pop { position: absolute; left: 10px; bottom: 30px; width: 150px; background: var(--pop); color: var(--popFg);
  border-radius: 8px; padding: 4px; box-shadow: 0 0 0 1px var(--popRing), 0 6px 18px rgba(0,0,0,.18); }
.app .pop div { display: flex; justify-content: space-between; padding: 3px 8px; border-radius: 5px; }
.app .pop div span:last-child { color: var(--muted); }
.app .pop div.h { background: var(--hover); color: var(--hoverFg); }
.app .pop hr { border: 0; border-top: 1px solid var(--border); margin: 4px 2px; }
@media (max-width: 480px) {
  .app { height: 400px; }
  .app .pop { width: 128px; left: auto; right: 6px; bottom: auto; top: 112px; }
}
</style>
</head>
<body>
<header>
  <h1>Theme preview</h1>
  <p>{{INTRO}} Each card is drawn from the colors the kit resolves for the theme, with its contrast checks below.</p>
  <div class="filters" role="toolbar" aria-label="Filter themes">
    <button data-f="set" data-v="all" aria-pressed="true">All</button>
    <button data-f="set" data-v="new" aria-pressed="false">New</button>
    <button data-f="set" data-v="old" aria-pressed="false">Existing</button>
    <span class="gap"></span>
    <button data-f="mode" data-v="all" aria-pressed="true">Light and dark</button>
    <button data-f="mode" data-v="light" aria-pressed="false">Light</button>
    <button data-f="mode" data-v="dark" aria-pressed="false">Dark</button>
  </div>
</header>
<main id="grid"></main>
<script>
const D = /*DATA*/;
const esc = s => String(s).replace(/[&<>"]/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;"}[c]));
function codeLine(theme, i) {
  const line = D.code[i];
  let out = "";
  const sel = D.selected[0] === i ? D.selected[1] : null;
  const match = D.matches.find(m => m[0] === i);
  let caretDone = false;
  for (const [cap, text] of line) {
    const s = cap ? theme.syn[cap] : null;
    let style = s ? `color:${s.c};${s.i ? "font-style:italic;" : ""}${s.b ? "font-weight:700;" : ""}${s.u ? "text-decoration:underline;" : ""}` : "";
    let t = esc(text);
    if (sel && text === sel) { t = `<span class="sel">${t}</span>`; }
    if (match && text === match[1]) { t = `<span class="m${match[2] ? " cur" : ""}">${t}</span>`; }
    out += `<span style="${style}">${t}</span>`;
    if (sel && text === sel && !caretDone) { out += '<span class="caret"></span>'; caretDone = true; }
  }
  const cur = i === D.active ? " cur" : "";
  return `<div class="ln${cur}"><span class="gut">${i + 1}</span><span>${out || " "}</span></div>`;
}
function card(t) {
  const vars = Object.entries(t.v).filter(([k]) => k !== "shadow").map(([k, v]) => `--${k}:${v}`).join(";");
  const lines = D.code.map((_, i) => codeLine(t, i)).join("");
  const levels = Object.entries(t.summary).map(([lv, s]) =>
    `<span class="lv ${s.failed ? "bad" : ""}">${lv} ${s.checks - s.failed}/${s.checks}</span>`).join("");
  const fails = t.fails.length ? `<details><summary>${t.fails.length} failing check${t.fails.length > 1 ? "s" : ""}</summary><ul>${
    t.fails.map(f => `<li><span class="sw" style="background:${f.bg};color:${f.fg}">Aa</span><b>${f.r.toFixed(2)}</b>
      <span>/ ${f.m}</span><span class="why">${esc(f.l)} · ${esc(f.t)}</span></li>`).join("")}</ul></details>` : "";
  const tag = t.fails.length ? `<span class="tag bad">${t.fails.length} failing</span>` : `<span class="tag ok">All checks pass</span>`;
  return `<section class="card" data-new="${t.new}" data-mode="${t.mode}">
  <div class="head"><h2>${esc(t.name)}</h2><span class="meta">${esc(t.file)} · ${t.mode}${t.new ? " · new" : ""}</span>${tag}</div>
  <div class="app${t.v.shadow ? " shadow" : ""}" style="${vars}">
    <div class="title"><span class="dot" style="background:#ff5f57"></span><span class="dot" style="background:#febc2e"></span><span class="dot" style="background:#28c840"></span>
      <span class="name">count_words <span class="muted">— main.rs</span></span></div>
    <div class="body">
      <div class="side"><div class="label">PROJECT</div>
        <div class="row"><span class="ic" style="background:var(--muted)"></span>Cargo.toml</div>
        <div class="row"><span class="ic" style="background:var(--link)"></span>src</div>
        <div class="row active"><span class="ic" style="background:var(--yellow)"></span>main.rs</div>
        <div class="row hover"><span class="ic" style="background:var(--yellow)"></span>words.rs</div>
        <div class="row"><span class="ic" style="background:var(--muted)"></span>README.md</div>
      </div>
      <div class="mainp">
        <div class="tabs"><div class="on">main.rs</div><div>words.rs</div></div>
        <div class="ed">${lines}<div class="thumb"></div></div>
        <div class="panel"><span class="btn def">Cancel</span><span class="btn pri">Save</span><span class="btn sec">Share</span>
          <span class="btn dan">Delete</span><span class="inp focus">counts</span><span class="inp">Search…</span>
          <span class="link">Docs</span></div>
      </div>
    </div>
    <div class="status"><span style="color:var(--green)">●</span><span>main</span><span style="color:var(--yellow)">▲ 1</span>
      <span style="color:var(--red)">✕ 0</span><span class="r">Ln 7, Col 31</span><span>UTF-8</span></div>
    <div class="pop"><div><span>Cut</span><span>⌘X</span></div><div class="h"><span>Copy</span><span>⌘C</span></div>
      <div><span>Paste</span><span>⌘V</span></div><hr><div><span>Rename…</span><span>F2</span></div></div>
  </div>
  <div class="sum">${levels}${fails}</div>
</section>`;
}
const grid = document.getElementById("grid");
grid.innerHTML = D.themes.map(card).join("");
const state = { set: "all", mode: "all" };
function apply() {
  for (const el of grid.children) {
    const isNew = el.dataset.new === "true";
    const okSet = state.set === "all" || (state.set === "new") === isNew;
    const okMode = state.mode === "all" || el.dataset.mode === state.mode;
    el.hidden = !(okSet && okMode);
  }
}
document.querySelectorAll(".filters button").forEach(b => b.addEventListener("click", () => {
  state[b.dataset.f] = b.dataset.v;
  document.querySelectorAll(`.filters button[data-f="${b.dataset.f}"]`).forEach(x => x.setAttribute("aria-pressed", x === b));
  apply();
}));
</script>
</body>
</html>
"""
