"""Build theme files from palette specs.

A spec (`palettes/*.toml`) names a few colors per theme: the background, the text, an accent,
the semantic colors and a syntax palette. `expand` derives every other role from them (hover and
active steps, borders, muted text, selection and so on), `build` writes all the keys the kit
reads, and `tune` then runs the contrast checks and nudges the OKLCH lightness of whichever role
fails, a small step at a time, until every check passes. Hue and chroma stay as designed, and
only roles that fail move.
"""

from __future__ import annotations

import json
import tomllib
from pathlib import Path

import checks
import colors as C
import kit

PALETTES = Path(__file__).resolve().parent / "palettes"
SCHEMA_URL = "https://github.com/longbridge/gpui-kit/raw/refs/heads/main/.theme-schema.json"
STEP = 0.004
STEP_GROWTH = 0.01
MAX_STEP = 0.3
MAX_ROUNDS = 500

# Roles the tuner leaves alone: the surfaces that define a theme, and the text placed on fills
# (chosen once as light or dark). The foreground moves only when nothing else can.
FIXED = {
    "background", "editor", "sidebar", "title_bar", "status_bar", "tab_bar", "popover",
    "primary_fg", "danger_fg", "success_fg", "warning_fg", "info_fg", "knob",
}
STICKY = {"foreground"}

# Checks of text over a selection or a search match move the selection, not the text.
PREFER_BG = ("editor.selection.text", "editor.search.text")

DEFAULT_SEMANTIC = {
    "light": {
        "red": "oklch(0.55 0.19 27)", "orange": "oklch(0.62 0.17 50)", "yellow": "oklch(0.70 0.15 85)",
        "green": "oklch(0.55 0.14 145)", "cyan": "oklch(0.58 0.10 210)", "blue": "oklch(0.55 0.17 255)",
        "magenta": "oklch(0.55 0.19 330)",
    },
    "dark": {
        "red": "oklch(0.70 0.17 25)", "orange": "oklch(0.76 0.15 55)", "yellow": "oklch(0.85 0.15 90)",
        "green": "oklch(0.77 0.15 150)", "cyan": "oklch(0.78 0.11 210)", "blue": "oklch(0.72 0.14 255)",
        "magenta": "oklch(0.72 0.16 330)",
    },
}

# syntax key -> (role, extra style)
SYNTAX_MAP = {
    "attribute": ("attribute", {}), "boolean": ("constant", {}), "comment": ("comment", "comment"),
    "comment_doc": ("comment_doc", "comment"), "constant": ("constant", {}),
    "constructor": ("type", {}), "embedded": ("variable", {}),
    "emphasis": ("variable", {"font_style": "italic"}),
    "emphasis.strong": ("variable", {"font_weight": 700}), "enum": ("type", {}),
    "function": ("function", {}), "hint": ("muted", {}), "keyword": ("keyword", "keyword"),
    "label": ("special", {}), "link_text": ("link", {}), "link_uri": ("link", {"font_style": "underline"}),
    "number": ("number", {}), "operator": ("operator", {}), "predictive": ("muted", {}),
    "preproc": ("preproc", {}), "primary": ("variable", {}), "property": ("property", {}),
    "punctuation": ("punctuation", {}), "punctuation.bracket": ("punctuation", {}),
    "punctuation.delimiter": ("punctuation", {}), "punctuation.list_marker": ("keyword", {}),
    "punctuation.special": ("special", {}), "string": ("string", {}), "string.escape": ("escape", {}),
    "string.regex": ("regex", {}), "string.special": ("special", {}),
    "string.special.symbol": ("constant", {}), "tag": ("tag", {}), "tag.doctype": ("preproc", {}),
    "text.code.span": ("string", {}), "text.literal": ("string", {}),
    "title": ("title", {"font_weight": 700}), "type": ("type", {}), "variable": ("variable", {}),
    "variable.special": ("special", {}), "variant": ("constant", {}),
}


def _c(value) -> C.RGBA:
    return C.parse_css(value) if isinstance(value, str) else value


def toward(a: C.RGBA, b: C.RGBA, t: float) -> C.RGBA:
    return C.mix(a, b, t)


def text_on(fill: C.RGBA, light: C.RGBA, dark: C.RGBA, force: str | None) -> C.RGBA:
    if force == "light":
        return light
    if force == "dark":
        return dark
    return C.best_text_on(fill, light, dark)


def away(c: C.RGBA, ink: C.RGBA, d: float) -> C.RGBA:
    """Shift `c` by `d` in OKLCH lightness, away from `ink` (for hover and pressed fills)."""
    sign = -1 if C.luminance(ink) > C.luminance(c) else 1
    return C.shift_l(c, sign * d)


# ---------------------------------------------------------------------------
# Spec -> roles


def expand(spec: dict, opts: dict) -> dict:
    """Fill in every role a theme needs from the few colors its spec names."""
    mode = spec["mode"]
    dark = mode == "dark"
    g = lambda key, default=None: _c(spec[key]) if key in spec else default  # noqa: E731
    bg, fg = _c(spec["background"]), _c(spec["foreground"])
    accent = _c(spec["accent"])
    tf = lambda c, t: toward(c, fg, t)  # noqa: E731

    r = {"background": bg, "foreground": fg, "accent": accent}
    r["editor"] = g("editor", bg)
    r["sidebar"] = g("sidebar", tf(bg, 0.045 if dark else 0.035))
    r["title_bar"] = g("title_bar", tf(bg, 0.035 if dark else 0.025))
    r["status_bar"] = g("status_bar", r["title_bar"])
    r["tab_bar"] = g("tab_bar", tf(bg, 0.05))
    r["popover"] = g("popover", tf(bg, 0.07) if dark else bg)
    r["control"] = g("control", tf(bg, 0.11) if dark else toward(bg, C.WHITE, 0.7))
    r["muted_bg"] = g("muted_bg", tf(bg, 0.07 if dark else 0.055))
    r["secondary"] = g("secondary", tf(bg, 0.12 if dark else 0.08))
    r["hover"] = g("hover", tf(bg, 0.09 if dark else 0.065))
    r["list_hover"] = g("list_hover", tf(bg, 0.065 if dark else 0.05))
    r["list_even"] = g("list_even", tf(bg, 0.025))
    r["list_head"] = g("list_head", tf(bg, 0.035))
    r["sidebar_active"] = g("sidebar_active", toward(r["sidebar"], fg, 0.13 if dark else 0.11))
    r["group"] = g("group", tf(bg, 0.035))
    r["desc_label"] = g("desc_label", tf(bg, 0.045))
    r["border"] = g("border", tf(bg, 0.15 if dark else 0.13))
    r["input_border"] = g("input_border", tf(bg, 0.24 if dark else 0.25))
    r["scrollbar"] = g("scrollbar", tf(bg, 0.32))
    r["scrollbar_hover"] = g("scrollbar_hover", tf(bg, 0.48))
    r["switch_off"] = g("switch_off", tf(bg, 0.24 if dark else 0.2))
    r["knob"] = g("knob", toward(fg, C.WHITE, 0.5) if dark else C.WHITE)
    r["muted"] = g("muted", toward(fg, bg, 0.42))
    r["control_step"] = float(spec.get("control_step", 0.045))
    r["secondary_step"] = float(spec.get("secondary_step", 0.055))
    r["primary_step"] = float(spec.get("primary_step", 0.035))

    r["primary"] = g("primary", accent)
    r["ring"] = g("ring", accent)
    r["caret"] = g("caret", r["ring"])
    r["link"] = g("link", accent)
    r["selection"] = g("selection", accent)
    r["list_active"] = g("list_active", accent)

    sem = {k: _c(v) for k, v in DEFAULT_SEMANTIC[mode].items()}
    sem.update({k: _c(v) for k, v in (spec.get("semantic") or {}).items()})
    for k, v in sem.items():
        r[k] = v
    r["danger"] = sem.get("danger", sem["red"])
    r["warning"] = sem.get("warning", sem["yellow"])
    r["success"] = sem.get("success", sem["green"])
    r["info"] = sem.get("info", sem["cyan"])

    # Text on accent and semantic fills. Semantic colors double as text on the background, so
    # they end up dark in a light theme and light in a dark one, and their fills take the
    # opposite ink. The accent takes white in a light theme unless it is too pale for it.
    light_ink = g("on_fill_light", C.WHITE)
    dark_ink = g("on_fill_dark", bg if dark else fg)
    force = spec.get("primary_text", opts.get("primary_text"))
    if force is None and not dark:
        force = "dark" if C.wcag(light_ink, r["primary"]) < 3.0 else "light"
    r["primary_fg"] = text_on(r["primary"], light_ink, dark_ink, force)
    for s in ("danger", "warning", "success", "info"):
        ink = spec.get(f"{s}_text") or ("dark" if dark else "light")
        r[f"{s}_fg"] = text_on(r[s], light_ink, dark_ink, ink)
        # Text on the tinted danger, success, warning and info buttons, tuned on its own because
        # a tint behind it needs a little more contrast than the bare background.
        r[f"{s}_ink"] = r[s]

    active_line = spec.get("active_line", opts.get("active_line", "neutral"))
    if "active_line_color" in spec:
        r["active_line"] = _c(spec["active_line_color"])
    elif active_line == "accent":
        r["active_line"] = toward(r["editor"], accent, 0.09 if dark else 0.06)
    else:
        r["active_line"] = toward(r["editor"], fg, 0.045 if dark else 0.035)
    r["gutter_border"] = g("gutter_border", toward(r["editor"], fg, 0.13 if dark else 0.1))

    syn = {k: _c(v) for k, v in (spec.get("syntax") or {}).items()}
    s = lambda key, default: syn.get(key, default)  # noqa: E731
    r["keyword"] = s("keyword", r["magenta"])
    r["function"] = s("function", r["blue"])
    r["type"] = s("type", r["cyan"])
    r["string"] = s("string", r["green"])
    r["number"] = s("number", r["orange"])
    r["constant"] = s("constant", r["number"])
    r["comment"] = s("comment", r["muted"])
    r["comment_doc"] = s("comment_doc", r["comment"])
    r["variable"] = s("variable", fg)
    r["property"] = s("property", r["variable"])
    r["attribute"] = s("attribute", r["type"])
    r["tag"] = s("tag", r["keyword"])
    r["operator"] = s("operator", fg)
    r["punctuation"] = s("punctuation", toward(fg, r["editor"], 0.3))
    r["escape"] = s("escape", r["constant"])
    r["regex"] = s("regex", r["escape"])
    r["special"] = s("special", r["constant"])
    r["preproc"] = s("preproc", r["keyword"])
    r["title"] = s("title", r["keyword"])
    r["link_syntax"] = s("link", r["link"])
    return r


# ---------------------------------------------------------------------------
# Roles -> theme JSON


def build(name: str, mode: str, r: dict, opts: dict) -> tuple[dict, dict]:
    """Write every key the kit reads. Returns (theme, provenance: key -> role)."""
    dark = mode == "dark"
    colors: dict[str, str] = {}
    hl: dict = {}
    src: dict[str, str] = {}

    def put(key, value, role=None, alpha=None):
        c = value if alpha is None else C.with_alpha(value, alpha)
        colors[key] = C.to_hex(c)
        if role:
            src["c:" + key] = role

    def puth(key, value, role=None, alpha=None):
        c = value if alpha is None else C.with_alpha(value, alpha)
        hl[key] = C.to_hex(c)
        if role:
            src["h:" + key] = role

    fg, bg = r["foreground"], r["background"]
    put("background", bg, "background")
    put("foreground", fg, "foreground")
    put("border", r["border"], "border")
    put("input.border", r["input_border"], "input_border")
    put("ring", r["ring"], "ring")
    put("caret", r["caret"], "caret")
    put("muted.background", r["muted_bg"], "muted_bg")
    put("muted.foreground", r["muted"], "muted")
    put("accent.background", r["hover"], "hover")
    put("accent.foreground", fg, "foreground")
    put("accordion.background", bg, "background")

    # Buttons. Hover and pressed fills step away from the button's text so they never lose contrast.
    put("primary.background", r["primary"], "primary")
    put("primary.foreground", r["primary_fg"], "primary_fg")
    ps, ss, cs = r["primary_step"], r["secondary_step"], r["control_step"]
    put("primary.hover.background", away(r["primary"], r["primary_fg"], ps), "primary")
    put("primary.active.background", away(r["primary"], r["primary_fg"], 2 * ps), "primary")
    put("secondary.background", r["secondary"], "secondary")
    put("secondary.foreground", fg, "foreground")
    put("secondary.hover.background", toward(r["secondary"], fg, ss), "secondary")
    put("secondary.active.background", toward(r["secondary"], fg, 2 * ss), "secondary")
    put("button.background", r["control"], "control")
    put("button.foreground", fg, "foreground")
    put("button.hover.background", toward(r["control"], fg, cs), "control")
    put("button.active.background", toward(r["control"], fg, 2 * cs), "control")
    for key, step in (("primary", "primary_step"), ("secondary", "secondary_step"), ("button", "control_step")):
        src[f"step:c:{key}.hover.background"] = step
    for k in ("background", "foreground", "hover.background", "active.background"):
        colors[f"button.primary.{k}"] = colors[f"primary.{k}"]
        src[f"c:button.primary.{k}"] = src[f"c:primary.{k}"]
        colors[f"button.secondary.{k}"] = colors[f"secondary.{k}"]
        src[f"c:button.secondary.{k}"] = src[f"c:secondary.{k}"]
    src["step:c:button.primary.hover.background"] = "primary_step"
    src["step:c:button.secondary.hover.background"] = "secondary_step"

    tint = (0.12, 0.16, 0.20) if dark else (0.07, 0.11, 0.15)
    for s in ("danger", "success", "warning", "info"):
        put(f"{s}.background", r[s], s)
        put(f"{s}.foreground", r[f"{s}_fg"], f"{s}_fg")
        put(f"{s}.hover.background", away(r[s], r[f"{s}_fg"], 0.035), s)
        put(f"{s}.active.background", away(r[s], r[f"{s}_fg"], 0.07), s)
        put(f"button.{s}.background", r[s], s, tint[0])
        put(f"button.{s}.foreground", r[f"{s}_ink"], f"{s}_ink")
        put(f"button.{s}.hover.background", r[s], s, tint[1])
        put(f"button.{s}.active.background", r[s], s, tint[2])

    put("group_box.background", r["group"], "group")
    put("group_box.foreground", fg, "foreground")
    put("group_box.title.foreground", fg, "foreground")
    put("chart.1", r["ring"], "ring")
    put("chart.2", r["success"], "success")
    put("chart.3", r["warning"], "warning")
    put("chart.4", r["magenta"], "magenta")
    put("chart.5", r["cyan"], "cyan")
    put("chart.bullish", r["success"], "success")
    put("chart.bearish", r["danger"], "danger")
    put("chart.grid", r["border"], "border")
    put("description_list.label.background", r["desc_label"], "desc_label")
    put("description_list.label.foreground", r["muted"], "muted")
    put("drag.border", r["ring"], "ring", 0.65)
    put("drop_target.background", r["ring"], "ring", 0.15)
    put("link", r["link"], "link")
    put("link.hover", C.shift_l(r["link"], 0.06 if dark else -0.06), "link")
    put("link.active", r["link"], "link")
    put("list.background", bg, "background")
    put("list.active.background", r["list_active"], "list_active", 0.2 if dark else 0.16)
    put("list.active.border", r["ring"], "ring")
    put("list.even.background", r["list_even"], "list_even")
    put("list.head.background", r["list_head"], "list_head")
    put("list.hover.background", r["list_hover"], "list_hover")
    put("popover.background", r["popover"], "popover")
    put("popover.foreground", fg, "foreground")
    put("progress.bar.background", r["primary"], "primary")
    put("scrollbar.background", bg, "background", 0.0)
    put("scrollbar.thumb.background", r["scrollbar"], "scrollbar")
    put("scrollbar.thumb.hover.background", r["scrollbar_hover"], "scrollbar_hover")
    put("selection.background", r["selection"], "selection", 0.3)
    put("sidebar.background", r["sidebar"], "sidebar")
    put("sidebar.foreground", fg, "foreground")
    put("sidebar.accent.background", r["sidebar_active"], "sidebar_active")
    put("sidebar.accent.foreground", fg, "foreground")
    put("sidebar.border", r["border"], "border")
    put("sidebar.primary.background", r["primary"], "primary")
    put("sidebar.primary.foreground", r["primary_fg"], "primary_fg")
    put("skeleton.background", r["muted_bg"], "muted_bg")
    put("slider.background", r["primary"], "primary")
    put("slider.thumb.background", r["knob"], "knob")
    put("switch.background", r["switch_off"], "switch_off")
    put("switch.thumb.background", r["knob"], "knob")
    put("tab.background", r["tab_bar"], "tab_bar")
    put("tab.active.background", r["editor"], "editor")
    put("tab.foreground", r["muted"], "muted")
    put("tab.active.foreground", fg, "foreground")
    put("tab_bar.background", r["tab_bar"], "tab_bar")
    put("tab_bar.segmented.background", r["secondary"], "secondary")
    put("table.background", bg, "background")
    put("table.active.background", r["list_active"], "list_active", 0.2 if dark else 0.16)
    put("table.active.border", r["ring"], "ring")
    put("table.even.background", r["list_even"], "list_even")
    put("table.head.background", r["list_head"], "list_head")
    put("table.head.foreground", r["muted"], "muted")
    put("table.foot.background", r["list_head"], "list_head")
    put("table.foot.foreground", r["muted"], "muted")
    put("table.hover.background", r["list_hover"], "list_hover")
    put("table.row.border", r["border"], "border")
    put("title_bar.background", r["title_bar"], "title_bar")
    put("title_bar.border", r["border"], "border")
    put("status_bar.background", r["status_bar"], "status_bar")
    put("status_bar.border", r["border"], "border")
    put("overlay", C.BLACK, None, 0.45 if dark else 0.18)
    put("window.border", r["border"], "border")
    for hue, role in (("blue", "blue"), ("cyan", "cyan"), ("green", "success"), ("magenta", "magenta"),
                      ("red", "danger"), ("yellow", "warning")):
        put(f"base.{hue}", r[role], role)
        put(f"base.{hue}.light", C.shift_l(r[role], 0.1), role)

    colors = {k: colors[k] for k in kit.COLOR_KEYS if k in colors}

    ed = r["editor"]
    puth("editor.background", ed, "editor")
    puth("editor.foreground", fg, "foreground")
    puth("editor.active_line.background", r["active_line"], "active_line")
    puth("editor.line_number", r["muted"], "muted")
    puth("editor.active_line_number", fg, "foreground")
    puth("editor.invisible", r["muted"], "muted", 0.45)
    puth("editor.gutter.background", ed, "editor")
    puth("editor.gutter.border", r["gutter_border"], "gutter_border")
    for kind, role in (("error", "danger"), ("warning", "warning"), ("info", "info"),
                       ("success", "success"), ("hint", "cyan")):
        puth(kind, r[role], role)
        puth(f"{kind}.background", toward(ed, r[role], 0.16 if dark else 0.12), role)
        puth(f"{kind}.border", r[role], role)

    styles = opts.get("styles", {})
    syntax = {}
    for key in kit.SYNTAX_KEYS:
        role, extra = SYNTAX_MAP[key]
        if isinstance(extra, str):
            extra = dict(styles.get(extra, {}))
        role_key = "link_syntax" if role == "link" else role
        entry = {"color": C.to_hex(r[role_key])}
        entry.update(extra)
        syntax[key] = entry
        src["s:" + key] = role_key
    hl["syntax"] = syntax

    theme = {"name": name, "mode": mode}
    for key in ("font.size", "font.family", "mono_font.family", "mono_font.size", "radius", "radius.lg", "shadow"):
        if key in opts:
            theme[key] = opts[key]
    theme["colors"] = colors
    theme["highlight"] = hl
    return theme, src


# ---------------------------------------------------------------------------
# Tuning


def _src_key(ref: str, resolved: kit.Resolved) -> str | None:
    """Which theme key a check reference reads."""
    if ref.startswith("fg:"):
        return "c:" + ref[3:]
    if ref.startswith("syn:"):
        name = ref[4:]
        syntax = resolved.highlight["syntax"]
        while True:
            key = "comment_doc" if name == "comment.doc" else name
            if key in syntax:
                return "s:" + key
            if "." not in name:
                return None
            name = name.split(".")[0]
    if ref.startswith("hl:"):
        return "h:" + ref[3:]
    if ref.startswith("@"):
        name = ref[1:]
        fixed = {
            "editor_bg": "h:editor.background", "active_line": "h:editor.active_line.background",
            "gutter": "h:editor.gutter.background", "gutter_border": "h:editor.gutter.border",
            "selection": "c:selection.background",
            "search": "c:selection.background", "invisible": "h:editor.invisible",
            "ghost_hover": "c:accent.background",
        }
        if name in fixed:
            return fixed[name]
        if name == "input_bg":
            return "c:input.border" if resolved.mode == "dark" else "c:background"
        if name.startswith("alert:"):
            return f"c:{name[6:]}.background"
        if name.startswith("status:"):
            return "h:" + name[7:]
        return None
    return "c:" + ref


def _priority(role: str) -> int | None:
    if role in FIXED:
        return None
    return 1 if role in STICKY else 0


def _can_move(value, sign: int) -> bool:
    if isinstance(value, float):
        return value < MAX_STEP
    L = C.to_oklch(value)[0]
    return L < 0.999 if sign > 0 else L > 0.001


def _move(value, sign: int):
    if isinstance(value, float):  # a hover step only ever grows
        return min(MAX_STEP, value + STEP_GROWTH)
    return C.shift_l(value, sign * STEP)


def tune(name: str, mode: str, roles: dict, opts: dict, levels: dict, log: list) -> tuple[dict, dict, list]:
    roles = dict(roles)
    start = dict(roles)
    mode_sign = 1 if mode == "dark" else -1
    for _ in range(MAX_ROUNDS):
        theme, src = build(name, mode, roles, opts)
        resolved = kit.resolve_theme(theme)
        results = checks.evaluate(resolved)
        failures = [res for res in results if res.ratio + 1e-9 < levels[res.check.level]]
        if not failures:
            break
        moves: dict[str, set] = {}
        for res in failures:
            fy = C.luminance(C.parse_hex(res.fg_hex))
            by = C.luminance(C.parse_hex(res.bg_hex))
            options = []
            for side, ref, mine, other in ((0, res.check.fg[-1], fy, by), (1, res.check.bg[-1], by, fy)):
                key = _src_key(ref, resolved)
                if key is None:
                    continue
                # A hover fill that sits too close to its resting fill takes a bigger step.
                role = src.get("step:" + key) if res.check.level == "hover" else None
                role = role or src.get(key)
                if role is None or _priority(role) is None:
                    continue
                if abs(mine - other) < 1e-6:
                    sign = mode_sign if side == 0 else -mode_sign
                else:
                    sign = 1 if mine > other else -1
                options.append((_priority(role), side, role, sign))
            prefer_bg = res.check.id.startswith(PREFER_BG) or res.check.id.endswith(".selection")
            options.sort(key=lambda o: (o[0], -o[1] if prefer_bg else o[1]))
            for _, _, role, sign in options:
                if _can_move(roles[role], sign):
                    moves.setdefault(role, set()).add(sign)
                    break
        applied = False
        for role, signs in moves.items():
            if len(signs) == 1:
                roles[role] = _move(roles[role], signs.pop())
                applied = True
        if not applied:
            break
    theme, src = build(name, mode, roles, opts)
    resolved = kit.resolve_theme(theme)
    results = checks.evaluate(resolved)
    failures = [res for res in results if res.ratio + 1e-9 < levels[res.check.level]]
    for role, value in roles.items():
        before = start.get(role)
        if isinstance(value, float):
            if before is not None and abs(value - before) > 1e-9:
                log.append(f"    {role}: {before:.3f} -> {value:.3f}")
        elif before is not None and C.to_hex(before) != C.to_hex(value):
            dl = C.to_oklch(value)[0] - C.to_oklch(before)[0]
            log.append(f"    {role}: {C.to_hex(before)} -> {C.to_hex(value)} (L {dl:+.3f})")
    return theme, roles, failures


# ---------------------------------------------------------------------------
# Families


def load_spec(path: Path) -> dict:
    with open(path, "rb") as f:
        return tomllib.load(f)


def generate_family(spec: dict, verbose: bool = False) -> tuple[dict, list[str], list]:
    """Build one theme file from a family spec. Returns (file, log lines, failures)."""
    family_opts = dict(spec.get("options") or {})
    levels = dict(checks.LEVELS)
    levels.update(spec.get("levels") or {})
    themes, log, failures = [], [], []
    variants: list[tuple[dict, dict]] = []
    for t in spec["themes"]:
        variants.append((t, {}))
    for accent in spec.get("accents") or []:
        for t in spec["themes"]:
            variants.append((t, accent))
    for t, accent in variants:
        t = dict(t)
        opts = dict(family_opts)
        opts.update(t.pop("options", {}) or {})
        name = t["name"]
        if accent:
            name = t["accent_name"].format(accent=accent["name"])
            for key in ("accent", "primary", "ring", "caret", "selection", "list_active", "primary_text"):
                t.pop(key, None)
            t["accent"] = accent[t["mode"]]
            for key, value in (accent.get(t["mode"] + "_overrides") or {}).items():
                t[key] = value
        roles = expand(t, opts)
        tlog: list[str] = []
        theme, roles, fails = tune(name, t["mode"], roles, opts, levels, tlog)
        themes.append(theme)
        status = "ok" if not fails else f"{len(fails)} check(s) still fail"
        log.append(f"  {name}: {status}, {len(tlog)} role(s) adjusted")
        if verbose:
            log.extend(tlog)
        for res in fails:
            failures.append((name, res))
            log.append(f"    FAIL {res.check.level} {res.ratio:.2f} {res.fg_hex} on {res.bg_hex} {res.check.label}")
    out = {"$schema": SCHEMA_URL, "name": spec["family"], "author": spec.get("author", "Mesa Hills Research")}
    if spec.get("url"):
        out["url"] = spec["url"]
    out["themes"] = themes
    return out, log, failures


def dump(data: dict) -> str:
    return json.dumps(data, indent=2, ensure_ascii=False) + "\n"


def cmd_generate(args) -> int:
    specs = [Path(p) for p in args.specs] or sorted(PALETTES.glob("*.toml"))
    out_dir = Path(args.out)
    status = 0
    for path in specs:
        spec = load_spec(path)
        data, log, failures = generate_family(spec, args.verbose)
        target = out_dir / spec["file"]
        text = dump(data)
        if args.check:
            current = target.read_text() if target.exists() else None
            if current != text:
                print(f"out of date: {target} (run `python3 script/themes generate`)")
                status = 1
            continue
        target.write_text(text)
        print(f"wrote {target} ({len(data['themes'])} themes)")
        for line in log:
            print(line)
        if failures:
            status = 1
    return status
