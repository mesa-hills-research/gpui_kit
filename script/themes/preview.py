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
    raw = kit.with_aliases(((theme.get("highlight") or {}).get("syntax")) or {}, kit.SYNTAX_ALIASES)
    while True:
        key = "comment_doc" if name == "comment.doc" else name
        if key in raw and isinstance(raw[key], dict) and raw[key].get("color"):
            return raw[key]
        if "." not in name:
            return {}
        name = name.split(".")[0]


GROUP_ORDER = [
    "Text", "Window", "Sidebar", "Menus", "Lists and tables", "Tabs", "Inputs", "Buttons",
    "Status colors", "Focus and accent", "Charts", "Controls and borders", "Editor", "Syntax",
]

BUTTON_NAMES = {"": "Default", "primary": "Primary", "secondary": "Secondary", "danger": "Danger",
                "success": "Success", "warning": "Warning", "info": "Info"}


def describe(check: checks.Check) -> tuple[str, str]:
    """The UI element a check belongs to, and a plain description of what it measures."""
    i = check.id
    if i.startswith("button."):
        parts = i.split(".")[1:]
        variant = parts[0] if parts[0] in BUTTON_NAMES and parts[0] else ""
        rest = parts[1:] if variant else parts
        name = BUTTON_NAMES[variant] + " button"
        if rest and rest[0] == "ghost":
            return "Buttons", "Ghost button, hover: text on its fill"
        state = rest[0] if rest and rest[0] in ("hover", "active") else ""
        state_name = {"hover": "hover", "active": "pressed"}.get(state, "")
        if rest and rest[-1] == "fill":
            return "Buttons", f"{name}: hover fill against the resting fill"
        return "Buttons", f"{name}{', ' + state_name if state_name else ''}: text on its fill"
    if i.startswith("syntax."):
        return "Syntax", check.label.replace("Syntax: ", "Syntax on the editor background: ")
    if i.startswith(("editor.", "diagnostic.")):
        return "Editor", check.label
    if i.startswith("sidebar"):
        return "Sidebar", check.label
    if i.startswith(("popover", "menu.")):
        return "Menus", check.label
    if i.startswith(("list", "table")):
        return ("Focus and accent" if i == "table.active.border" else "Lists and tables"), check.label
    if i.startswith("tab"):
        return "Tabs", check.label
    if i.startswith(("input.placeholder", "ring.input")) or i == "input":
        return "Inputs", check.label
    if i in ("title_bar", "status_bar", "title_bar.border"):
        return "Window", check.label
    if i.startswith(("danger.", "success.", "warning.", "info.", "base.")) or i == "primary.fill":
        return "Status colors", check.label
    if i in ("ring", "primary.ui"):
        return "Focus and accent", check.label
    if i.startswith("chart."):
        return "Charts", check.label
    if i in ("input.border", "scrollbar", "switch", "border", "sidebar.border", "table.row.border"):
        return "Controls and borders", check.label
    return "Text", check.label


def _key_for(ref: str, r: kit.Resolved):
    """The theme key a check reference reads: ("colors" | "highlight" | "syntax", key)."""
    if ref.startswith("fg:"):
        return ("colors", ref[3:])
    if ref.startswith("syn:"):
        return ("syntax", ref[4:])
    if ref.startswith("@"):
        name = ref[1:]
        table = {
            "editor_bg": ("highlight", "editor.background"),
            "editor_fg": ("highlight", "editor.foreground"),
            "line_number": ("highlight", "editor.line_number"),
            "active_line_number": ("highlight", "editor.active_line_number"),
            "active_line": ("highlight", "editor.active_line.background"),
            "gutter": ("highlight", "editor.gutter.background"),
            "gutter_border": ("highlight", "editor.gutter.border"),
            "selection": ("colors", "selection.background"), "search": ("colors", "selection.background"),
            "invisible": ("highlight", "editor.invisible"),
            "ghost_hover": ("colors", "accent.background"),
        }
        if name in table:
            return table[name]
        if name == "input_bg":
            return ("colors", "input.border" if r.mode == "dark" else "background")
        if name.startswith("alert:"):
            return ("colors", name[6:] + ".background")
        if name.startswith("status:"):
            return ("highlight", name[7:])
        return None
    return ("colors", ref)


def _sets(theme: dict, where: tuple) -> bool:
    kind, key = where
    hl = theme.get("highlight")
    if kind == "colors":
        colors = theme.get("colors") or {}
        value = colors.get(key)
        if value is None:
            value = next((colors[a] for a, k in kit.COLOR_ALIASES.items() if k == key and a in colors), None)
        if not isinstance(value, str):
            return False
        try:
            kit.parse_token(value)
            return True
        except ValueError:
            return False
    if kind == "highlight":
        return bool(hl) and isinstance(hl.get(key), str)
    # A syntax color always comes from the theme's own syntax map unless it has no highlight section.
    return bool(hl)


def from_kit_default(check: checks.Check, theme: dict, r: kit.Resolved) -> bool:
    """True when the failing pair uses a kit fallback for a key the theme leaves unset."""
    for stack in (check.fg, check.bg):
        refs = list(stack)
        while refs:
            ref = refs.pop()
            if ref == "@gutter" and not _sets(theme, ("highlight", "editor.gutter.background")):
                continue  # an unset gutter paints nothing, so the layer below decides
            where = _key_for(ref, r)
            if where and not _sets(theme, where):
                return True
            break
    return False


def theme_data(theme: dict, file_name: str, family: str, new: bool) -> dict:
    r = kit.resolve_theme(theme)
    t, hl = r.tokens, r.highlight
    dark = r.mode == "dark"
    look = lambda ref: checks.lookup(r, ref)  # noqa: E731
    fg = t["foreground"].color
    editor_fg = look("@editor_fg")[0]
    editor_bg = look("@editor_bg")[0]
    syn = {}
    for _, line in enumerate(CODE):
        for cap, _ in line:
            if cap and cap not in syn:
                c = kit.syntax_color(hl["syntax"], cap)
                style = syntax_style(theme, cap) if c else {}
                syn[cap] = {
                    "c": css(c or editor_fg),
                    "i": style.get("font_style") == "italic",
                    "u": style.get("font_style") == "underline",
                    "b": (style.get("font_weight") or 400) >= 600,
                }
    results = checks.evaluate(r)
    summary = checks.summarize(results)
    fails = []
    for res in sorted(results, key=lambda x: (GROUP_ORDER.index(describe(x.check)[0]), x.ratio)):
        if res.ok:
            continue
        group, label = describe(res.check)
        fails.append({
            "g": group, "t": label, "r": round(res.ratio, 3), "m": res.minimum,
            "fg": res.fg_hex, "bg": res.bg_hex, "k": from_kit_default(res.check, theme, r),
        })
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
        "editor": css(editor_bg), "editorFg": css(editor_fg), "gutter": css(look("@gutter")[0]),
        "gutterBorder": css(look("@gutter_border")[0]), "lineNumber": css(look("@line_number")[0]),
        "activeLineNumber": css(look("@active_line_number")[0]),
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
    issues = sum(len(t["fails"]) for t in old)
    kit_issues = sum(1 for t in old for f in t["fails"] if f["k"])
    new_part = (f"{n_new} new themes, all passing every check." if not new_fail
                else f"{n_new} new themes, {new_fail} with contrast issues.")
    old_part = (f" {len(old)} existing themes, {old_fail} with contrast issues "
                f"({issues} in all, {kit_issues} of them from kit defaults)." if old else "")
    intro = new_part + old_part
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
.sum { padding: 10px 12px 12px; font-size: 13px; color: var(--ink-2); }
.sum .pass { margin: 0; color: var(--ok); }
.sum summary { cursor: pointer; color: var(--ink); font-weight: 600; padding: 4px 0; min-height: 28px; }
.sum .legend { margin: 6px 0 4px; font-size: 12px; }
.sum h3 { font-size: 12px; margin: 10px 0 2px; color: var(--ink); text-transform: uppercase; letter-spacing: .04em; }
.sum ul { margin: 0; padding: 0; list-style: none; }
.sum li { display: grid; grid-template-columns: auto 1fr auto; grid-template-areas: "sw what num" "sw what src";
  column-gap: 8px; align-items: center; padding: 5px 0; border-top: 1px solid var(--line); }
.sw { grid-area: sw; display: inline-flex; align-items: center; justify-content: center; width: 38px; height: 26px; border-radius: 5px;
  font: 600 13px/1 ui-monospace, Menlo, monospace; border: 1px solid var(--line); }
.sum .what { grid-area: what; color: var(--ink); min-width: 0; overflow-wrap: anywhere; }
.sum .hex { display: block; color: var(--ink-2); font: 11px/1.4 ui-monospace, Menlo, monospace; }
.sum .hex i { display: inline-block; width: 9px; height: 9px; border-radius: 2px; border: 1px solid var(--line); margin: 0 3px 0 0; vertical-align: -1px; }
.sum .num { grid-area: num; text-align: right; white-space: nowrap; }
.src { grid-area: src; justify-self: end; font-size: 11px; padding: 1px 6px; border-radius: 999px; white-space: nowrap; border: 1px solid var(--line); }
.src.kit { color: var(--ink-2); border-style: dashed; }
.src.theme { color: var(--bad); border-color: currentColor; }

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
.app .ed { position: relative; background: var(--editor); color: var(--editorFg); font: 11.5px/17px ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  padding: 4px 0; overflow: hidden; white-space: pre; }
.app .ln { display: flex; }
.app .ln.cur { background: var(--activeLine); }
.app .gut { width: 30px; flex: none; text-align: right; padding-right: 7px; margin-right: 7px; color: var(--lineNumber);
  background: var(--gutter); border-right: 1px solid var(--gutterBorder); }
.app .ln.cur .gut { color: var(--activeLineNumber); }
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
  <p>{{INTRO}} Each card is drawn from the colors the kit resolves for the theme. Tap a card's contrast line to see each issue.</p>
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
// Two decimals, or three when two would round a failing ratio up to its minimum.
const ratio = f => parseFloat(f.r.toFixed(2)) >= f.m ? f.r.toFixed(3) : f.r.toFixed(2);
function card(t) {
  const vars = Object.entries(t.v).filter(([k]) => k !== "shadow").map(([k, v]) => `--${k}:${v}`).join(";");
  const lines = D.code.map((_, i) => codeLine(t, i)).join("");
  const total = Object.values(t.summary).reduce((a, s) => a + s.checks, 0);
  const n = t.fails.length, kitN = t.fails.filter(f => f.k).length;
  const plural = n === 1 ? "issue" : "issues";
  const headline = n === 0 ? `No contrast issues in ${total} checks`
    : `${n} contrast ${plural}` + (kitN === n ? ", all from kit defaults" : kitN ? `, ${kitN} from kit defaults` : "");
  let body = "";
  if (n) {
    const groups = [];
    for (const f of t.fails) {
      let g = groups.find(x => x.name === f.g);
      if (!g) { g = { name: f.g, items: [] }; groups.push(g); }
      g.items.push(f);
    }
    body = `<p class="legend"><span class="src theme">theme</span> a color the theme sets ·
      <span class="src kit">kit default</span> the kit's fallback for a key the theme leaves out</p>` +
      groups.map(g => `<h3>${esc(g.name)}</h3><ul>${g.items.map(f => `<li>
        <span class="sw" style="background:${f.bg};color:${f.fg}">Aa</span>
        <span class="what">${esc(f.t)}<span class="hex"><i style="background:${f.fg}"></i>${f.fg}
          on <i style="background:${f.bg}"></i>${f.bg}</span></span>
        <span class="num"><b>${ratio(f)}</b> of ${f.m}</span>
        <span class="src ${f.k ? "kit" : "theme"}">${f.k ? "kit default" : "theme"}</span></li>`).join("")}</ul>`).join("");
  }
  const fails = n ? `<details><summary>${esc(headline)}</summary>${body}</details>`
    : `<p class="pass">${esc(headline)}</p>`;
  const tag = n ? `<span class="tag bad">${n} contrast ${plural}</span>` : `<span class="tag ok">All checks pass</span>`;
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
  <div class="sum">${fails}</div>
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
