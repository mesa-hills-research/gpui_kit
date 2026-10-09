"""How gpui-component reads a theme file.

`resolve()` replays `ThemeColor::apply_config` (crates/component/src/theme/schema.rs): it fills
every color the kit draws with, applying the same fallbacks, the same derived hover and active
colors and the same alpha clamps. The checker and the preview work from its output, so a theme
is judged by the colors that end up on screen, not only by the keys it sets.
"""

from __future__ import annotations

import json
import math
import re
from dataclasses import dataclass, field
from pathlib import Path

import colors as C

ROOT = Path(__file__).resolve().parents[2]
THEME_DIR = ROOT / "crates" / "component" / "src" / "theme"
REGISTRY_RS = ROOT / "crates" / "component" / "src" / "highlighter" / "registry.rs"

# Keys of `ThemeConfigColors`, in the order schema.rs declares them.
COLOR_KEYS = [
    "accent.background", "accent.foreground", "accordion.background", "background", "border",
    "button.background", "button.active.background", "button.foreground", "button.hover.background",
    "button.danger.background", "button.danger.active.background", "button.danger.foreground",
    "button.danger.hover.background", "button.info.background", "button.info.active.background",
    "button.info.foreground", "button.info.hover.background", "button.primary.background",
    "button.primary.active.background", "button.primary.foreground", "button.primary.hover.background",
    "button.secondary.background", "button.secondary.active.background", "button.secondary.foreground",
    "button.secondary.hover.background", "button.success.background", "button.success.active.background",
    "button.success.foreground", "button.success.hover.background", "button.warning.background",
    "button.warning.active.background", "button.warning.foreground", "button.warning.hover.background",
    "group_box.background", "group_box.foreground", "group_box.title.foreground", "caret",
    "chart.1", "chart.2", "chart.3", "chart.4", "chart.5", "chart.bullish", "chart.bearish", "chart.grid",
    "danger.background", "danger.active.background", "danger.foreground", "danger.hover.background",
    "description_list.label.background", "description_list.label.foreground", "drag.border",
    "drop_target.background", "foreground", "info.background", "info.active.background",
    "info.foreground", "info.hover.background", "input.border", "link", "link.active", "link.hover",
    "list.background", "list.active.background", "list.active.border", "list.even.background",
    "list.head.background", "list.hover.background", "muted.background", "muted.foreground",
    "popover.background", "popover.foreground", "primary.background", "primary.active.background",
    "primary.foreground", "primary.hover.background", "progress.bar.background", "ring",
    "scrollbar.background", "scrollbar.thumb.background", "scrollbar.thumb.hover.background",
    "secondary.background", "secondary.active.background", "secondary.foreground",
    "secondary.hover.background", "selection.background", "sidebar.background",
    "sidebar.accent.background", "sidebar.accent.foreground", "sidebar.border", "sidebar.foreground",
    "sidebar.primary.background", "sidebar.primary.foreground", "skeleton.background",
    "slider.background", "slider.thumb.background", "success.background", "success.foreground",
    "success.hover.background", "success.active.background", "switch.background",
    "switch.thumb.background", "tab.background", "tab.active.background", "tab.active.foreground",
    "tab_bar.background", "tab_bar.segmented.background", "tab.foreground", "table.background",
    "table.active.background", "table.active.border", "table.even.background", "table.head.background",
    "table.head.foreground", "table.foot.background", "table.foot.foreground", "table.hover.background",
    "table.row.border", "title_bar.background", "title_bar.border", "status_bar.background",
    "status_bar.border", "warning.background", "warning.active.background", "warning.hover.background",
    "warning.foreground", "overlay", "window.border", "base.blue", "base.blue.light", "base.cyan",
    "base.cyan.light", "base.green", "base.green.light", "base.magenta", "base.magenta.light",
    "base.red", "base.red.light", "base.yellow", "base.yellow.light",
]

# Keys of `HighlightThemeStyle`, with `StatusColors` flattened into it.
HIGHLIGHT_KEYS = [
    "editor.background", "editor.foreground", "editor.active_line.background", "editor.line_number",
    "editor.active_line_number", "editor.invisible", "editor.gutter.background", "editor.gutter.border",
    "error", "error.background", "error.border", "warning", "warning.background", "warning.border",
    "info", "info.background", "info.border", "success", "success.background", "success.border",
    "hint", "hint.background", "hint.border", "syntax",
]

# Keys of `SyntaxColors`. `comment_doc` has no serde rename, so a file has to spell it with an
# underscore even though the highlighter looks it up as `comment.doc`.
SYNTAX_KEYS = [
    "attribute", "boolean", "comment", "comment_doc", "constant", "constructor", "embedded",
    "emphasis", "emphasis.strong", "enum", "function", "hint", "keyword", "label", "link_text",
    "link_uri", "number", "operator", "predictive", "preproc", "primary", "property", "punctuation",
    "punctuation.bracket", "punctuation.delimiter", "punctuation.list_marker", "punctuation.special",
    "string", "string.escape", "string.regex", "string.special", "string.special.symbol", "tag",
    "tag.doctype", "text.code.span", "text.literal", "title", "type", "variable", "variable.special",
    "variant",
]

# The capture names the highlighter asks the theme for (`HIGHLIGHT_NAMES` in registry.rs).
HIGHLIGHT_NAMES = [
    "attribute", "boolean", "comment", "comment.doc", "constant", "constructor", "embedded",
    "emphasis", "emphasis.strong", "enum", "function", "hint", "keyword", "label", "link_text",
    "link_uri", "number", "operator", "predictive", "preproc", "primary", "property", "punctuation",
    "punctuation.bracket", "punctuation.delimiter", "punctuation.list_marker", "punctuation.special",
    "string", "string.escape", "string.regex", "string.special", "string.special.symbol", "tag",
    "tag.doctype", "text.code.span", "text.literal", "title", "type", "variable", "variable.special",
    "variant",
]

THEME_KEYS = [
    "is_default", "name", "mode", "font.size", "font.family", "mono_font.family", "mono_font.size",
    "radius", "radius.lg", "shadow", "colors", "highlight",
]


# ---------------------------------------------------------------------------
# Parsing theme color values


def _named_colors() -> dict:
    return json.loads((THEME_DIR / "default-colors.json").read_text())


_NAMED = None


def parse_color(value: str) -> C.RGBA:
    """`try_parse_color`: hex, or a named color such as `neutral-200` or `blue-500/50`."""
    global _NAMED
    value = value.strip()
    if value.startswith("#"):
        return C.parse_hex(value)
    m = re.match(r"^([a-zA-Z]+)(?:-(\d+))?(?:/([0-9.]+))?$", value)
    if not m:
        raise ValueError(f"invalid color: {value!r}")
    name, scale, op = m.group(1).lower(), m.group(2), m.group(3)
    if _NAMED is None:
        _NAMED = _named_colors()
    if name not in ("white", "black") and name not in (
        "neutral", "gray", "red", "orange", "amber", "yellow", "lime", "green", "emerald", "teal",
        "cyan", "sky", "blue", "indigo", "violet", "purple", "fuchsia", "pink", "rose",
    ):
        raise ValueError(f"unknown color name: {value!r}")
    entry = _NAMED[name]
    if isinstance(entry, list):
        by_scale = {e["scale"]: e for e in entry}
        entry = by_scale.get(int(scale) if scale else 500, by_scale[500])
    h, s, l = (float(p.rstrip("%")) for p in entry["hslChannel"].split())
    c = C.hsl_to_rgb(h / 360, s / 100, l / 100, 1.0)
    if op is not None:
        o = float(op)
        if o > 100:
            raise ValueError(f"invalid opacity in {value!r}")
        c = C.opacity(c, o / 100)
    return c


def _split_top(s: str) -> list[str]:
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def parse_gradient(value: str) -> list[C.RGBA]:
    """The two color stops of a `linear-gradient(...)` value."""
    value = value.strip()
    if not (value.startswith("linear-gradient(") and value.endswith(")")):
        raise ValueError(f"not a gradient: {value!r}")
    parts = _split_top(value[len("linear-gradient(") : -1])
    if parts and (re.match(r"^-?[0-9.]+deg$", parts[0]) or parts[0].startswith("to ")):
        parts = parts[1:]
    if len(parts) != 2:
        raise ValueError(f"expected two color stops: {value!r}")
    return [parse_color(p.split()[0]) for p in parts]


@dataclass
class Tok:
    """A resolved color: `color` is the solid color, `stops` what a background paints."""

    color: C.RGBA
    stops: list = field(default_factory=list)

    @staticmethod
    def solid(c: C.RGBA) -> "Tok":
        return Tok(c, [c])

    def opacity(self, f: float) -> "Tok":
        return Tok(C.opacity(self.color, f), [C.opacity(s, f) for s in self.stops])


def parse_token(value: str) -> Tok:
    try:
        return Tok.solid(parse_color(value))
    except ValueError:
        stops = parse_gradient(value)
        return Tok(stops[0], stops)


# ---------------------------------------------------------------------------
# apply_config


# The contrast the kit gives text it derives (`TEXT_CONTRAST` in schema.rs).
TEXT_CONTRAST = 4.5


def oklch(L: float, Ch: float, h: float) -> C.RGBA:
    """The kit's `oklch()`: out-of-gamut channels are clipped one by one."""
    hr = math.radians(h)
    return C.from_oklab(L, Ch * math.cos(hr), Ch * math.sin(hr))


def readable_text(color: C.RGBA, backgrounds: list, minimum: float) -> C.RGBA:
    """`readable_text` in color.rs: `color` as text reaching `minimum` contrast on every one of
    `backgrounds`, made darker or lighter in steps of 0.01 OKLCH lightness when it falls short."""

    def reads(c):
        return all(C.wcag(c, b) >= minimum for b in backgrounds)

    def worst(c):
        return min(C.wcag(c, b) for b in backgrounds)

    color = C.blend(backgrounds[0], color)
    if reads(color):
        return color
    darker = worst(C.BLACK) >= worst(C.WHITE)
    L, Ch, h = C.to_oklch(color)
    step = -0.01 if darker else 0.01
    for i in range(1, 101):
        l = max(0.0, min(1.0, L + step * i))
        candidate = oklch(l, Ch, h)
        if reads(candidate):
            return candidate
        if l in (0.0, 1.0):
            break
    return C.BLACK if darker else C.WHITE


def resolve(colors: dict, mode: str, default: dict | None) -> dict:
    """Resolve a theme's `colors` the way `ThemeColor::apply_config` does.

    `default` is the resolved built-in theme of the same mode (`None` while resolving the
    built-in theme itself, where the fallback is gpui's transparent black).
    """
    dark = mode == "dark"
    t: dict[str, Tok] = {}
    zero = Tok.solid(C.TRANSPARENT)

    def dflt(key: str) -> Tok:
        return default[key] if default else zero

    def col(key: str, fb=None):
        """apply_color!: a solid color."""
        v = colors.get(key)
        fallback = dflt(key).color if fb is None else (fb.color if isinstance(fb, Tok) else fb)
        c = fallback
        if isinstance(v, str):
            try:
                c = parse_color(v)
            except ValueError:
                c = fallback
        t[key] = Tok.solid(c)
        return c

    def bg(key: str, fb=None):
        """apply_background_color!: a color or a gradient."""
        v = colors.get(key)
        if fb is None:
            fallback = dflt(key)
        elif isinstance(fb, Tok):
            fallback = fb
        else:
            fallback = Tok.solid(fb)
        tok = fallback
        if isinstance(v, str):
            try:
                tok = parse_token(v)
            except ValueError:
                tok = fallback
        t[key] = tok
        return tok.color

    def mixt(c: C.RGBA, f: float) -> C.RGBA:  # `c.mix_oklab(transparent, f)`
        return C.opacity(c, f)

    def tinted_button(name: str, c: C.RGBA) -> None:
        """apply_tinted_button!: the color at 20%, 30% and 40%, with text that reads on all three."""
        prefix = f"button.{name}."
        bg(prefix + "background", mixt(c, 0.2))
        bg(prefix + "hover.background", mixt(c, 0.3))
        bg(prefix + "active.background", mixt(c, 0.4))
        states = ("background", "hover.background", "active.background")
        fills = [C.blend(background, t[prefix + state].color) for state in states]
        col(prefix + "foreground", readable_text(c, fills, TEXT_CONTRAST))

    background = bg("background")
    for name in ("red", "green", "blue", "magenta", "yellow", "cyan"):
        base = col(f"base.{name}")
        col(f"base.{name}.light", C.blend(background, C.opacity(base, 0.8)))
    border = col("border")
    foreground = col("foreground")
    inp = col("input.border", border)
    muted = bg("muted.background")
    col("muted.foreground", C.blend(muted, C.opacity(foreground, 0.7)))

    active_darken = 0.2 if dark else 0.1
    button_bg = mixt(inp, 0.3) if dark else background
    bg("button.background", button_bg)
    col("button.foreground", foreground)
    bg("button.hover.background", mixt(inp, 0.5))
    bg("button.active.background", mixt(inp, 0.7))
    primary = bg("primary.background")
    primary_fg = col("primary.foreground", foreground)
    bg("primary.hover.background", C.blend(background, C.opacity(primary, 0.9)))
    bg("primary.active.background", C.hsl_darken(primary, active_darken))
    bg("button.primary.background", t["primary.background"])
    col("button.primary.foreground", primary_fg)
    bg("button.primary.hover.background", t["primary.hover.background"])
    bg("button.primary.active.background", t["primary.active.background"])
    secondary = bg("secondary.background")
    secondary_fg = col("secondary.foreground", foreground)
    bg("secondary.hover.background", C.blend(background, C.opacity(secondary, 0.9)))
    bg("secondary.active.background", C.hsl_darken(secondary, active_darken))
    bg("button.secondary.background", t["secondary.background"])
    col("button.secondary.foreground", secondary_fg)
    bg("button.secondary.hover.background", t["secondary.hover.background"])
    bg("button.secondary.active.background", t["secondary.active.background"])

    for name, base_key, hover_dark in (
        ("success", "base.green", None),
        ("info", "base.cyan", None),
        ("warning", "base.yellow", "warning"),
    ):
        c = bg(f"{name}.background", t[base_key].color)
        col(f"{name}.foreground", primary_fg)
        bg(f"{name}.hover.background", C.blend(background, C.opacity(c, 0.9)))
        if name == "warning":
            bg(f"{name}.active.background", C.blend(background, C.hsl_darken(c, active_darken)))
        else:
            bg(f"{name}.active.background", C.hsl_darken(c, active_darken))
        tinted_button(name, c)

    accent = bg("accent.background", t["secondary.background"])
    col("accent.foreground", foreground)
    bg("accordion.background", t["background"])
    bg("group_box.background", C.blend(background, C.opacity(secondary, 0.3 if dark else 0.4)))
    col("group_box.foreground", foreground)
    col("group_box.title.foreground", foreground)  # parsed, never applied by the kit
    col("caret", primary)
    blue = t["base.blue"].color
    col("chart.1", C.hsl_lighten(blue, 0.4))
    col("chart.2", C.hsl_lighten(blue, 0.2))
    col("chart.3", blue)
    col("chart.4", C.hsl_darken(blue, 0.2))
    col("chart.5", C.hsl_darken(blue, 0.4))
    col("chart.bullish", t["base.green"].color)
    col("chart.bearish", t["base.red"].color)
    col("chart.grid", C.opacity(border, 0.6))
    danger = bg("danger.background", t["base.red"].color)
    bg("danger.active.background", C.hsl_darken(danger, active_darken))
    col("danger.foreground", primary_fg)
    bg("danger.hover.background", C.blend(background, C.opacity(danger, 0.9)))
    tinted_button("danger", danger)
    bg("description_list.label.background", C.blend(background, C.opacity(border, 0.2)))
    col("description_list.label.foreground", t["muted.foreground"].color)
    col("drag.border", C.opacity(primary, 0.65))
    bg("drop_target.background", C.opacity(primary, 0.2))
    link = col("link", primary)
    col("link.active", link)
    col("link.hover", link)
    bg("list.background", t["background"])
    bg("list.active.background", C.blend(background, C.opacity(primary, 0.1)))
    col("list.active.border", C.blend(background, C.opacity(primary, 0.6)))
    bg("list.even.background", t["list.background"])
    bg("list.head.background", t["list.background"])
    bg("list.hover.background", C.opacity(accent, 0.6))
    bg("popover.background", t["background"])
    col("popover.foreground", foreground)
    bg("progress.bar.background", t["primary.background"])
    col("ring", blue)
    bg("scrollbar.background", t["background"])
    bg("scrollbar.thumb.background", t["accent.background"])
    bg("scrollbar.thumb.hover.background", t["scrollbar.thumb.background"])
    bg("selection.background", t["primary.background"])
    bg("sidebar.background", C.blend(background, C.opacity(border, 0.15)))
    bg("sidebar.accent.background", t["accent.background"])
    col("sidebar.accent.foreground", t["accent.foreground"].color)
    col("sidebar.border", border)
    col("sidebar.foreground", foreground)
    bg("sidebar.primary.background", t["primary.background"])
    col("sidebar.primary.foreground", primary_fg)
    bg("skeleton.background", t["secondary.background"])
    bg("slider.background", t["primary.background"])
    bg("slider.thumb.background", primary_fg)
    bg("switch.background", t["secondary.active.background"])
    bg("switch.thumb.background", t["background"])
    bg("tab.background", t["background"])
    bg("tab.active.background", t["background"])
    col("tab.active.foreground", foreground)
    bg("tab_bar.background", t["background"])
    bg("tab_bar.segmented.background", t["secondary.background"])
    col("tab.foreground", foreground)
    bg("table.background", t["list.background"])
    bg("table.active.background", t["list.active.background"])
    col("table.active.border", t["list.active.border"].color)
    bg("table.even.background", t["list.even.background"])
    bg("table.head.background", t["list.head.background"])
    col("table.head.foreground", t["muted.foreground"].color)
    bg("table.foot.background", t["list.head.background"])
    col("table.foot.foreground", t["muted.foreground"].color)
    bg("table.hover.background", t["list.hover.background"])
    col("table.row.border", border)
    bg("title_bar.background", t["background"])
    col("title_bar.border", border)
    bg("status_bar.background", t["title_bar.background"])
    col("status_bar.border", t["title_bar.border"].color)
    bg("overlay")
    col("window.border", border)

    # list.active, table.active and selection are capped at 20%, 20% and 30% opacity.
    for key, cap in (("list.active.background", 0.2), ("table.active.background", 0.2), ("selection.background", 0.3)):
        tok = t[key]
        base_a = tok.color[3]
        target = min(base_a, cap)
        color = C.with_alpha(tok.color, target)
        raw = colors.get(key)
        stops = None
        if isinstance(raw, str):
            try:
                parsed = parse_token(raw)
                stops = [C.with_alpha(s, min(s[3], cap)) for s in parsed.stops]
            except ValueError:
                stops = None
        if stops is None:
            factor = target / base_a if base_a > 0 else 1.0
            stops = [C.opacity(s, factor) for s in tok.stops]
        t[key] = Tok(color, stops)
    return t


# ---------------------------------------------------------------------------
# Highlight section


def parse_highlight(hl: dict | None) -> dict:
    """The highlight values the kit reads: editor and status colors plus a syntax map."""
    out = {"syntax": {}}
    if not hl:
        return out
    for key in HIGHLIGHT_KEYS:
        if key == "syntax":
            continue
        v = hl.get(key)
        if isinstance(v, str):
            try:
                out[key] = C.parse_hex(v)
            except ValueError:
                pass
    for key, style in (hl.get("syntax") or {}).items():
        if key in SYNTAX_KEYS and isinstance(style, dict) and isinstance(style.get("color"), str):
            try:
                out["syntax"][key] = C.parse_hex(style["color"])
            except ValueError:
                pass
    return out


def syntax_color(syntax: dict, name: str):
    """`SyntaxColors::style`: the color for a capture name, falling back to its prefix."""
    key = "comment_doc" if name == "comment.doc" else name
    if key in syntax:
        return syntax[key]
    if "." in name:
        return syntax_color(syntax, name.split(".")[0])
    return None


# ---------------------------------------------------------------------------
# Built-in default themes


_DEFAULTS = None


def defaults() -> dict:
    """The kit's built-in light and dark themes, resolved: {mode: (tokens, highlight)}."""
    global _DEFAULTS
    if _DEFAULTS is None:
        data = json.loads((THEME_DIR / "default-theme.json").read_text())
        _DEFAULTS = {}
        for theme in data["themes"]:
            mode = theme.get("mode", "light")
            _DEFAULTS[mode] = (resolve(theme.get("colors", {}), mode, None), parse_highlight(theme.get("highlight")))
    return _DEFAULTS


@dataclass
class Resolved:
    name: str
    mode: str
    tokens: dict
    highlight: dict
    has_highlight: bool
    config: dict


def resolve_theme(theme: dict) -> Resolved:
    mode = theme.get("mode", "light")
    default_tokens, default_hl = defaults()[mode]
    tokens = resolve(theme.get("colors") or {}, mode, default_tokens)
    hl = theme.get("highlight")
    return Resolved(
        name=theme.get("name", ""),
        mode=mode,
        tokens=tokens,
        highlight=parse_highlight(hl) if hl else default_hl,
        has_highlight=bool(hl),
        config=theme,
    )


# ---------------------------------------------------------------------------
# Key lists read from the Rust sources, to notice when the tables above fall behind.


def _struct_keys(source: str, struct: str) -> list[str]:
    m = re.search(r"pub struct " + struct + r"\s*\{(.*?)\n\}", source, re.S)
    if not m:
        return []
    keys, rename = [], None
    for line in m.group(1).splitlines():
        line = line.strip()
        r = re.match(r'#\[serde\(rename = "([^"]+)"\)\]', line)
        if r:
            rename = r.group(1)
            continue
        if line.startswith("#[serde(flatten)]"):
            rename = "<flatten>"
            continue
        f = re.match(r"(?:pub(?:\(crate\))?\s+)?([a-z_0-9]+)\s*:", line)
        if f:
            if rename != "<flatten>":
                keys.append(rename or f.group(1))
            rename = None
    return keys


def keys_from_source() -> dict[str, list[str]]:
    schema = (THEME_DIR / "schema.rs").read_text()
    registry = REGISTRY_RS.read_text()
    hl = _struct_keys(registry, "HighlightThemeStyle") + _struct_keys(registry, "StatusColors")
    return {
        "colors": _struct_keys(schema, "ThemeConfigColors"),
        "highlight": hl,
        "syntax": _struct_keys(registry, "SyntaxColors"),
    }
