"""The contrast checks: every foreground and background pair the kit draws.

A check compares two stacks of layers, each painted bottom first over the window. For text the
upper stack is the lower one plus the text color, so translucent text and translucent fills are
composited exactly as they appear. Layers are references into a resolved theme (see `lookup`).
"""

from __future__ import annotations

import itertools
from dataclasses import dataclass

import colors as C
import kit

# Minimum WCAG 2 contrast ratio per level. See docs/themes.md for the reasoning.
LEVELS = {
    "text": 4.5,  # body text, labels, button text, syntax tokens, links, status text
    "ui": 3.0,  # focus ring, caret, accent fills, active-row border, status and chart colors
    "subtle": 3.0,  # deliberately quiet text: line numbers, punctuation, selected code
    "control": 1.4,  # edges that outline a control: input border, scrollbar thumb, switch track
    "highlight": 1.2,  # fills that mark a selection: text selection, active list row, sidebar item
    "divider": 1.12,  # separators and borders, whitespace markers
    "hover": 1.05,  # hover fills and search matches, visible but quiet
}

# APCA Lc a pair should reach, reported alongside WCAG 2 but not enforced.
APCA_TARGETS = {"text": 60.0, "subtle": 45.0, "ui": 45.0}

LEVEL_ORDER = list(LEVELS)


@dataclass
class Check:
    id: str
    level: str
    fg: list  # stack of refs, bottom first
    bg: list
    label: str


@dataclass
class Result:
    check: Check
    ratio: float
    apca: float | None
    fg_hex: str
    bg_hex: str

    @property
    def minimum(self) -> float:
        return LEVELS[self.check.level]

    @property
    def ok(self) -> bool:
        return self.ratio + 1e-9 >= self.minimum


SUBTLE_SYNTAX = {"punctuation", "punctuation.bracket", "punctuation.delimiter", "predictive", "hint"}


def lookup(r: kit.Resolved, ref: str):
    """Resolve a reference to a list of alternative colors (two for a gradient), or None."""
    t, hl = r.tokens, r.highlight
    dark = r.mode == "dark"
    if ref.startswith("fg:"):  # a token's solid color, as used for text
        tok = t.get(ref[3:])
        return [tok.color] if tok else None
    if ref.startswith("syn:"):
        c = kit.syntax_color(hl["syntax"], ref[4:])
        return [c] if c else None
    if ref.startswith("hl:"):
        c = hl.get(ref[3:])
        return [c] if c else None
    if ref.startswith("@"):
        name = ref[1:]
        if name == "input_bg":
            return [C.opacity(t["input.border"].color, 0.3)] if dark else [t["background"].color]
        if name == "editor_bg":
            c = hl.get("editor.background")
            return [c] if c else lookup(r, "@input_bg")
        if name == "gutter":
            c = hl.get("editor.gutter.background")
            return [c] if c else [C.TRANSPARENT]
        if name == "gutter_border":  # the separator between line numbers and text
            c = hl.get("editor.gutter.border")
            return [c] if c else [t["border"].color]
        if name == "active_line":
            c = hl.get("editor.active_line.background")
            return [c] if c else None
        if name == "selection":
            return list(t["selection.background"].stops)
        if name == "search":
            sel = t["selection.background"].color
            return [C.hsl_set_saturation(sel, 0.1)]
        if name == "invisible":
            c = hl.get("editor.invisible")
            return [c] if c else [t["muted.foreground"].color]
        if name == "ghost_hover":
            tok = t["accent.background"]
            return [C.opacity(s, 0.5) for s in tok.stops] if dark else list(tok.stops)
        if name.startswith("alert:"):
            return [C.opacity(t[name[6:] + ".background"].color, 0.04)]
        if name.startswith("status:"):
            kind = name[7:]
            c = hl.get(kind)
            if c is None:
                base = {"error": "red", "warning": "yellow", "info": "blue", "success": "green", "hint": "cyan"}[kind]
                c = t["base." + base].color
            return [c]
        raise KeyError(ref)
    tok = t.get(ref)
    return list(tok.stops) if tok else None


def build_checks(r: kit.Resolved) -> list[Check]:
    out: list[Check] = []

    def text(id_, label, fg, layers, level="text"):
        out.append(Check(id_, level, ["background", *layers, fg], ["background", *layers], label))

    def fill(id_, label, fills, layers, level):
        out.append(Check(id_, level, ["background", *layers, *fills], ["background", *layers], label))

    def versus(id_, label, a, b, level):
        out.append(Check(id_, level, ["background", *a], ["background", *b], label))

    # Text on surfaces
    text("text", "Text on the background", "fg:foreground", [])
    text("muted", "Muted text on the background", "fg:muted.foreground", [])
    text("muted.fill", "Muted text on a muted fill (kbd, skeleton)", "fg:muted.foreground", ["muted.background"])
    text("title_bar", "Title bar text", "fg:foreground", ["title_bar.background"])
    text("status_bar", "Status bar text", "fg:muted.foreground", ["status_bar.background"])
    text("sidebar", "Sidebar text", "fg:sidebar.foreground", ["sidebar.background"])
    text("sidebar.muted", "Sidebar muted text", "fg:muted.foreground", ["sidebar.background"])
    text("sidebar.active", "Sidebar active item text", "fg:sidebar.accent.foreground", ["sidebar.background", "sidebar.accent.background"])
    text("sidebar.primary", "Sidebar primary item text", "fg:sidebar.primary.foreground", ["sidebar.background", "sidebar.primary.background"])
    text("popover", "Popover and menu text", "fg:popover.foreground", ["popover.background"])
    text("popover.muted", "Menu shortcut and muted text", "fg:muted.foreground", ["popover.background"])
    text("menu.hover", "Menu item under the pointer", "fg:accent.foreground", ["popover.background", "accent.background"])
    text("list", "List row text", "fg:foreground", ["list.background"])
    text("list.even", "Striped list row text", "fg:foreground", ["list.background", "list.even.background"])
    text("list.hover", "Hovered list row text", "fg:foreground", ["list.background", "list.hover.background"])
    text("list.active", "Selected list row text", "fg:foreground", ["list.background", "list.active.background"])
    text("table.head", "Table header text", "fg:table.head.foreground", ["table.background", "table.head.background"])
    text("table.active", "Selected table row text", "fg:foreground", ["table.background", "table.active.background"])
    text("table.hover", "Hovered table row text", "fg:foreground", ["table.background", "table.hover.background"])
    text("tab", "Tab text", "fg:tab.foreground", ["tab_bar.background", "tab.background"])
    text("tab.active", "Active tab text", "fg:tab.active.foreground", ["tab_bar.background", "tab.active.background"])
    text("input", "Input text", "fg:foreground", ["@input_bg"])
    text("input.placeholder", "Input placeholder", "fg:muted.foreground", ["@input_bg"])
    text("group_box", "Group box text", "fg:group_box.foreground", ["group_box.background"])
    text("description_list", "Description list label", "fg:description_list.label.foreground", ["description_list.label.background"])
    text("link", "Link", "fg:link", [])
    text("link.hover", "Link under the pointer", "fg:link.hover", [])

    # Buttons in every state
    for variant in ("", "primary", "secondary", "danger", "success", "warning", "info"):
        prefix = "button." + (variant + "." if variant else "")
        for state in ("", "hover.", "active."):
            name = (variant or "default") + (" " + state.rstrip(".") if state else "")
            text(f"{prefix}{state}text", f"Button {name}", f"fg:{prefix}foreground", [f"{prefix}{state}background"])
    text("button.ghost.hover", "Ghost button under the pointer", "fg:accent.foreground", ["@ghost_hover"])

    # Semantic colors as fills (tags, badges, checkbox) and as text (alerts, outline tags)
    for s in ("primary", "danger", "success", "warning", "info"):
        text(f"{s}.fill", f"Text on a {s} fill", f"fg:{s}.foreground", [f"{s}.background"])
    for s in ("danger", "success", "warning", "info"):
        text(f"{s}.text", f"{s.capitalize()} text in an alert", f"fg:{s}.background", [f"@alert:{s}"])

    # Indicators
    text("ring", "Focus ring", "fg:ring", [], "ui")
    text("ring.input", "Focus ring on an input", "fg:ring", ["@input_bg"], "ui")
    text("primary.ui", "Accent fill: checked controls, sliders, progress, outline buttons", "fg:primary.background", [], "ui")
    text("table.active.border", "Active row border", "fg:table.active.border", ["table.background"], "ui")
    for name in ("red", "yellow", "green", "blue", "magenta", "cyan"):
        text(f"base.{name}", f"Base {name}", f"fg:base.{name}", [], "ui")
    for key in ("chart.1", "chart.2", "chart.3", "chart.4", "chart.5", "chart.bullish", "chart.bearish"):
        text(key, f"Chart series {key[6:]}", f"fg:{key}", [], "ui")

    # Edges and fills
    text("input.border", "Input border", "fg:input.border", [], "control")
    fill("scrollbar", "Scrollbar thumb", ["scrollbar.thumb.background"], ["scrollbar.background"], "control")
    fill("switch", "Switch track when off", ["switch.background"], [], "control")
    fill("list.active.fill", "Selected list row", ["list.active.background"], ["list.background"], "highlight")
    fill("table.active.fill", "Selected table row", ["table.active.background"], ["table.background"], "highlight")
    fill("sidebar.active.fill", "Sidebar active item", ["sidebar.accent.background"], ["sidebar.background"], "highlight")
    fill("list.hover.fill", "Hovered list row", ["list.hover.background"], ["list.background"], "hover")
    fill("menu.hover.fill", "Hovered menu item", ["accent.background"], ["popover.background"], "hover")
    for variant in ("", "primary", "secondary"):
        prefix = "button." + (variant + "." if variant else "")
        versus(f"{prefix}hover.fill", f"Button {variant or 'default'} hover against rest",
               [f"{prefix}hover.background"], [f"{prefix}background"], "hover")
    text("border", "Border", "fg:border", [], "divider")
    text("title_bar.border", "Title bar border", "fg:title_bar.border", ["title_bar.background"], "divider")
    text("sidebar.border", "Sidebar border", "fg:sidebar.border", ["sidebar.background"], "divider")
    text("table.row.border", "Table row border", "fg:table.row.border", ["table.background"], "divider")

    # Editor
    ed = ["@editor_bg"]
    text("editor.text", "Editor text", "fg:foreground", ed)
    text("editor.active_line.text", "Editor text on the active line", "fg:foreground", ed + ["@active_line"])
    text("editor.selection.text", "Selected text", "fg:foreground", ed + ["@selection"])
    text("editor.search.text", "Text on the current search match", "fg:foreground", ed + ["@search", "@selection"])
    text("editor.line_number", "Line numbers", "fg:muted.foreground", ed + ["@gutter"], "subtle")
    text("editor.gutter.border", "Gutter separator", "@gutter_border", ed + ["@gutter"], "divider")
    text("editor.caret", "Caret", "fg:caret", ed, "ui")
    fill("editor.selection", "Selection", ["@selection"], ed, "highlight")
    fill("editor.search.current", "Current search match", ["@search", "@selection"], ed, "highlight")
    fill("editor.search", "Search match", ["@search"], ed, "hover")
    text("editor.invisible", "Whitespace markers", "@invisible", ed, "divider")
    for kind in ("error", "warning", "info", "hint"):
        text(f"diagnostic.{kind}", f"Diagnostic {kind} underline", f"@status:{kind}", ed, "ui")

    # Syntax: one set of checks per distinct color, naming every capture that uses it
    fg = r.tokens["foreground"].color
    groups: dict[tuple, list[str]] = {}
    for name in kit.HIGHLIGHT_NAMES:
        c = kit.syntax_color(r.highlight["syntax"], name)
        if c is None or C.to_hex(c) == C.to_hex(fg):
            continue
        level = "subtle" if name in SUBTLE_SYNTAX else "text"
        groups.setdefault((C.to_hex(c), level), []).append(name)
    for (_, level), names in groups.items():
        ref = "syn:" + names[0]
        label = ", ".join(names)
        text(f"syntax.{names[0]}", f"Syntax: {label}", ref, ed, level)
        text(f"syntax.{names[0]}.active_line", f"Syntax on the active line: {label}", ref, ed + ["@active_line"], level)
        text(f"syntax.{names[0]}.selection", f"Selected syntax: {label}", ref, ed + ["@selection"], "subtle")
    return out


def _flatten(stack: list, choice: dict, base: C.RGBA) -> C.RGBA:
    return C.flatten([choice[ref] for ref in stack], base)


def evaluate(r: kit.Resolved, checks: list[Check] | None = None) -> list[Result]:
    checks = build_checks(r) if checks is None else checks
    base = C.BLACK if r.mode == "dark" else C.WHITE
    results = []
    for chk in checks:
        refs = list(dict.fromkeys(chk.fg + chk.bg))
        alts = {}
        for ref in refs:
            v = lookup(r, ref)
            if v is None:
                break
            alts[ref] = v
        else:
            worst = None
            for combo in itertools.product(*(alts[ref] for ref in refs)):
                choice = dict(zip(refs, combo))
                f, b = _flatten(chk.fg, choice, base), _flatten(chk.bg, choice, base)
                ratio = C.wcag(f, b)
                if worst is None or ratio < worst[0]:
                    worst = (ratio, f, b)
            ratio, f, b = worst
            lc = C.apca(f, b) if chk.level in APCA_TARGETS else None
            results.append(Result(chk, ratio, lc, C.to_hex(f), C.to_hex(b)))
    return results


def summarize(results: list[Result]) -> dict:
    by_level = {lvl: [0, 0] for lvl in LEVEL_ORDER}
    for res in results:
        by_level[res.check.level][0] += 1
        if not res.ok:
            by_level[res.check.level][1] += 1
    return {lvl: {"checks": n, "failed": f} for lvl, (n, f) in by_level.items() if n}
