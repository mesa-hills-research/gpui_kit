"""Fix the contrast of existing theme files.

`fix` runs the contrast checks on a theme and moves colors until every check passes. For each pair
that fails it works out the smallest change of OKLCH lightness, hue and chroma kept, that makes
the pair pass: for the color in front and for the one behind it, darker or lighter. It takes the
cheapest of these, where the color in front costs least and a fill behind it a little more, and a
move that would break a pair that passes costs more again. A surface such as the background or the
editor moves only when nothing else can. It repeats until nothing fails. Only colors that take part in a failing
pair move, and a color the file leaves to a fallback is written into the file when it moves. A
translucent fill that no lightness makes visible, or that would lose most of its color on the way,
gets a little more opacity instead.
"""

from __future__ import annotations

import copy
import json
import re
from contextlib import contextmanager
from pathlib import Path

import checks
import colors as C
import generate
import kit

MAX_ROUNDS = 60
MAX_REVERSALS = 2  # how often a color may turn around, so moves can't go back and forth forever
COARSE = 0.04  # lightness step of the first scan, refined by bisection
FINE = 0.002

# How much a lightness change costs per color role: the text or fill in front, a fill behind it,
# and the surfaces a whole window or panel is painted with, which move only when nothing else can.
FRONT, BEHIND, SURFACE = 1.0, 1.5, 8.0

# The opacity the kit caps these fills at.
ALPHA_CAPS = {"c:list.active.background": 0.2, "c:table.active.background": 0.2, "c:selection.background": 0.3}
SURFACES = {
    "c:background", "c:popover.background", "c:sidebar.background", "c:title_bar.background",
    "c:status_bar.background", "c:tab_bar.background", "c:tab.background", "c:tab.active.background",
    "c:list.background", "c:table.background", "c:scrollbar.background", "c:accordion.background",
    "c:group_box.background", "h:editor.background", "h:editor.gutter.background",
}

# Where each highlight key falls back to when a file leaves it out, as a check reference.
HIGHLIGHT_FALLBACKS = {
    "editor.foreground": "fg:foreground",
    "editor.line_number": "fg:muted.foreground",
    "editor.active_line_number": "@editor_fg",
    "editor.gutter.border": "fg:border",
    "editor.invisible": "fg:muted.foreground",
    "error": "fg:base.red",
    "warning": "fg:base.yellow",
    "info": "fg:base.blue",
    "success": "fg:base.green",
    "hint": "fg:base.cyan",
}


class Theme:
    """One theme of a file: reads and writes the color behind a check's key."""

    def __init__(self, theme: dict):
        self.data = theme
        self.lowercase = _mostly_lowercase(theme)
        self.cases: dict[str, bool] = {}  # whether each key is written in lowercase, as it was

    @property
    def colors(self) -> dict:
        return self.data.get("colors") or {}

    @property
    def highlight(self) -> dict:
        return self.data.get("highlight") or {}

    @property
    def syntax(self) -> dict:
        return self.highlight.get("syntax") or {}

    def _color_slot(self, key: str) -> str:
        """The name a color is written under: its own, or the alias the file uses for it."""
        if key in self.colors:
            return key
        return next((a for a, k in kit.COLOR_ALIASES.items() if k == key and a in self.colors), key)

    def _syntax_slot(self, key: str) -> str:
        if key in self.syntax:
            return key
        return next((a for a, k in kit.SYNTAX_ALIASES.items() if k == key and a in self.syntax), key)

    def get(self, key: str, resolved: kit.Resolved) -> Value | None:
        """The color behind a key, or None when it can't move."""
        kind, name = key[:2], key[2:]
        if kind == "c:":
            raw = self.colors.get(self._color_slot(name))
            if isinstance(raw, str):
                return Value.parse(raw)
            tok = resolved.tokens.get(name)
            return Value(tok.color) if tok and len(set(tok.stops)) <= 1 else None
        if kind == "h:":
            raw = self.highlight.get(name)
            if isinstance(raw, str):
                return Value(C.parse_hex(raw))
            fallback = HIGHLIGHT_FALLBACKS.get(name)
            return Value(checks.lookup(resolved, fallback)[0]) if fallback and self.highlight else None
        if kind == "s:":
            style = self.syntax.get(self._syntax_slot(name)) or {}
            return Value(C.parse_hex(style["color"])) if isinstance(style.get("color"), str) else None
        return None

    def _slot(self, key: str) -> tuple[dict, str]:
        kind, name = key[:2], key[2:]
        if kind == "c:":
            return self.data.setdefault("colors", {}), self._color_slot(name)
        if kind == "h:":
            return self.data["highlight"], name
        return self.data["highlight"]["syntax"][self._syntax_slot(name)], "color"

    def set(self, key: str, origin: Value, lightness: float, alpha: float | None = None) -> None:
        """Write `origin` moved to `lightness` (and `alpha`) under `key`, in the case it had."""
        table, slot = self._slot(key)
        if key not in self.cases:
            letters = "".join(HEX.findall(str(table.get(slot, ""))))
            upper, lower = any(c in "ABCDEF" for c in letters), any(c in "abcdef" for c in letters)
            self.cases[key] = lower and not upper if upper or lower else self.lowercase
        table[slot] = origin.at(lightness, self.cases[key], alpha)

    @contextmanager
    def trial(self, key: str, origin: Value, lightness: float, alpha: float | None = None):
        """Move a color for the length of a `with` block."""
        table, slot = self._slot(key)
        had, old = slot in table, table.get(slot)
        self.set(key, origin, lightness, alpha)
        try:
            yield
        finally:
            if had:
                table[slot] = old
            else:
                del table[slot]


HEX = re.compile(r"#[0-9a-fA-F]{3,8}\b")


class Value:
    """A color a theme sets: one solid color, or a gradient whose hex stops move together."""

    def __init__(self, color: C.RGBA, raw: str | None = None):
        self.color = color
        self.raw = raw
        self.lightness = C.to_oklch(color)[0]

    @staticmethod
    def parse(raw: str):
        try:
            return Value(kit.parse_color(raw))
        except ValueError:
            pass
        try:
            stops = kit.parse_gradient(raw)
        except ValueError:
            return None
        if len(HEX.findall(raw)) != len(stops):
            return None  # a named color among the stops
        return Value(stops[0], raw)

    def at(self, lightness: float, lowercase: bool, alpha: float | None = None):
        """This value with its lightness, or a gradient's first stop's, moved to `lightness`, and
        a solid color's opacity set to `alpha`."""
        if self.raw is None:
            color = _with_lightness(self.color, lightness)
            return _hex(color if alpha is None else C.with_alpha(color, alpha), lowercase)
        shift = lightness - self.lightness

        def move(m):
            stop = C.parse_hex(m.group(0))
            return _hex(_with_lightness(stop, C.to_oklch(stop)[0] + shift), lowercase)

        return HEX.sub(move, self.raw)


def _hex(color: C.RGBA, lowercase: bool) -> str:
    value = C.to_hex(color)
    return value.lower() if lowercase else value


def _mostly_lowercase(theme: dict) -> bool:
    text = " ".join(HEX.findall(json.dumps(theme)))
    lower = sum(c in "abcdef" for c in text)
    return lower * 2 > lower + sum(c in "ABCDEF" for c in text)


def _with_lightness(origin: C.RGBA, lightness: float) -> C.RGBA:
    """`origin` at another OKLCH lightness, with its own hue, chroma and alpha."""
    _, chroma, hue = C.to_oklch(origin)
    return C.from_oklch(lightness, chroma, hue, origin[3])


class Fixer:
    def __init__(self, theme: dict, levels: dict):
        self.data = copy.deepcopy(theme)
        self.theme = Theme(self.data)
        self.before = Theme(copy.deepcopy(theme))
        self.levels = levels
        self.origin: dict[str, Value] = {}  # each moved key's value before it moved
        self.direction: dict[str, int] = {}  # the way each key last moved
        self.reversals: dict[str, int] = {}  # how often each key turned around
        self.alpha: dict[str, float] = {}  # the opacity a translucent fill was raised to

    def failing(self) -> set:
        resolved = kit.resolve_theme(self.data)
        return _subjects(r for r in checks.evaluate(resolved) if r.ratio + 1e-9 < self.levels[r.check.level])

    def passes(self, check: checks.Check) -> bool:
        results = checks.evaluate(kit.resolve_theme(self.data), [check])
        return bool(results) and results[0].ratio + 1e-9 >= self.levels[check.level]

    def nearest(self, check, try_at, now: float, end: float) -> float | None:
        """The value between `now` and `end` nearest `now` at which `check` passes."""
        with try_at(end):
            if not self.passes(check):
                return None
        fail, sign = now, 1 if end > now else -1
        while True:
            probe = min(end, fail + COARSE) if sign > 0 else max(end, fail - COARSE)
            with try_at(probe):
                if self.passes(check):
                    ok = probe
                    break
            fail = probe
        while abs(ok - fail) > FINE:
            mid = (ok + fail) / 2
            with try_at(mid):
                if self.passes(check):
                    ok = mid
                else:
                    fail = mid
        return ok

    def options(self, res: checks.Result, resolved: kit.Resolved, failing: set, seen: dict) -> list[tuple]:
        """Every way to make one failing pair pass, cheapest first, as
        (cost, key, sign, lightness, opacity or None). The cost is the change, weighted by the
        color's role, plus one for each pair that passes now and would fail after the move."""
        out = []
        dark = resolved.mode == "dark"
        for side, ref in ((0, res.check.fg[-1]), (1, res.check.bg[-1])):
            key = generate._src_key(ref, resolved)
            value = self.theme.get(key, resolved) if key else None
            if value is None:
                continue
            origin = self.origin.get(key, value)
            alpha = self.alpha.get(key)
            weight = SURFACE if key in SURFACES else (FRONT if side == 0 else BEHIND)
            away = _away(res, side, dark)
            for sign in (away, -away):
                if self.direction.get(key, sign) != sign and self.reversals.get(key, 0) >= MAX_REVERSALS:
                    continue
                end = 1.0 if sign > 0 else 0.0
                target = self.nearest(res.check, lambda l: self.theme.trial(key, origin, l, alpha), value.lightness, end)
                if target is None or (side == 0 and _tint_fades(origin, target)):
                    continue
                cost = abs(target - value.lightness) * weight
                out.append((cost + self.breaks(key, origin, target, alpha, failing, seen), key, sign, target, alpha))
            # A translucent fill that no lightness can make visible takes a little more opacity, up to
            # the most the kit allows it.
            cap = ALPHA_CAPS.get(key, 1.0)
            if not out and side == 0 and origin.raw is None and value.color[3] < cap:
                now = value.color[3]
                target = self.nearest(
                    res.check, lambda a: self.theme.trial(key, origin, value.lightness, a), now, cap
                )
                if target is not None:
                    cost = (target - now) * 2 * weight
                    out.append((cost + self.breaks(key, origin, value.lightness, target, failing, seen),
                                key, 0, value.lightness, target))
                else:
                    # Not even the most opacity is enough: take it, and move the lightness too.
                    sign = _away(res, side, dark)
                    lightness = self.nearest(
                        res.check, lambda l: self.theme.trial(key, origin, l, cap), value.lightness,
                        1.0 if sign > 0 else 0.0,
                    )
                    if lightness is not None:
                        cost = ((cap - now) * 2 + abs(lightness - value.lightness)) * weight
                        out.append((cost + self.breaks(key, origin, lightness, cap, failing, seen),
                                    key, sign, lightness, cap))
        # A surface moves only when nothing else can.
        if any(o[1] not in SURFACES for o in out):
            out = [o for o in out if o[1] not in SURFACES]
        return sorted(out, key=lambda o: o[0])

    def breaks(self, key, origin, lightness, alpha, failing: set, seen: dict) -> int:
        """How many pairs that pass now would fail after a move."""
        probe = (key, round(lightness, 3), alpha and round(alpha, 3))
        if probe not in seen:
            with self.theme.trial(key, origin, lightness, alpha):
                seen[probe] = len(self.failing() - failing)
        return seen[probe]

    def run(self) -> list[checks.Result]:
        failures = []
        for _ in range(MAX_ROUNDS):
            resolved = kit.resolve_theme(self.data)
            results = checks.evaluate(resolved)
            failures = [r for r in results if r.ratio + 1e-9 < self.levels[r.check.level]]
            if not failures:
                break
            failing = _subjects(failures)
            seen: dict = {}
            options = [self.options(res, resolved, failing, seen) for res in failures]
            # A key that two failures pull opposite ways is left to the failures' next options.
            banned: set[str] = set()
            for _ in range(5):
                chosen = [next((o for o in opts if o[1] not in banned), None) for opts in options]
                signs: dict[str, set] = {}
                for o in chosen:
                    if o:
                        signs.setdefault(o[1], set()).add(o[2])
                torn = {k for k, s in signs.items() if len(s) > 1}
                if not torn:
                    break
                banned |= torn
            moves: dict[str, tuple] = {}
            for o in chosen:
                if not o or o[1] in banned:
                    continue
                _, key, sign, target, alpha = o
                if key in moves:
                    # Two failures move one key the same way: go as far as the one that needs more.
                    old_sign, old_target, old_alpha = moves[key]
                    if not sign:
                        sign, target = old_sign, old_target
                    elif old_sign:
                        target = max(target, old_target) if sign > 0 else min(target, old_target)
                    alpha = max((a for a in (alpha, old_alpha) if a is not None), default=None)
                moves[key] = (sign, target, alpha)
            if not moves:
                break
            for key, (sign, target, alpha) in moves.items():
                self.origin.setdefault(key, self.theme.get(key, resolved))
                if sign:
                    if self.direction.get(key, sign) != sign:
                        self.reversals[key] = self.reversals.get(key, 0) + 1
                    self.direction[key] = sign
                if alpha is not None:
                    self.alpha[key] = alpha
                self.theme.set(key, self.origin[key], target, self.alpha.get(key))
        return failures

    def changes(self) -> dict[str, tuple]:
        """{key: (old, new)} for every color that moved, as the strings a file holds, the old one
        as the original theme had it (set or derived)."""
        before_resolved = kit.resolve_theme(self.before.data)
        resolved = kit.resolve_theme(self.data)
        out = {}
        for key in self.origin:
            old = self.before.get(key, before_resolved)
            new = self.theme.get(key, resolved)
            old, new = (v.at(v.lightness, False) if v else "?" for v in (old, new))
            if old.upper() != new.upper():
                out[key] = (old, new)
        return out


def _tint_fades(origin: Value, lightness: float) -> bool:
    """Whether moving a translucent fill to `lightness` would cost it most of its color, which
    the gamut takes near black and white. More opacity keeps its hue and chroma instead."""
    if origin.raw is not None or origin.color[3] >= 1:
        return False
    chroma = C.to_oklch(origin.color)[1]
    return chroma > 0.03 and C.to_oklch(_with_lightness(origin.color, lightness))[1] < chroma / 2


def _subjects(failures) -> set:
    """What failing checks are about: a syntax check per capture name it covers, since moving one
    syntax color regroups the checks of the others; every other check by its id."""
    out = set()
    for res in failures:
        cid = res.check.id
        if cid.startswith("syntax."):
            where = ".selection" if cid.endswith(".selection") else ".active_line" if cid.endswith(".active_line") else ""
            names = res.check.label.split(": ", 1)[1].split(", ")
            out.update(("syntax", name, where) for name in names)
        else:
            out.add(cid)
    return out


def _away(res: checks.Result, side: int, dark: bool) -> int:
    """The way the color on `side` (0 in front, 1 behind) moves away from the other one."""
    pair = (res.fg_hex, res.bg_hex) if side == 0 else (res.bg_hex, res.fg_hex)
    mine, other = (C.luminance(C.parse_hex(h)) for h in pair)
    if abs(mine - other) < 1e-6:
        return 1 if (dark if side == 0 else not dark) else -1
    return 1 if mine > other else -1


def fix_theme(theme: dict, levels: dict | None = None) -> tuple[dict, dict, list]:
    """Return (fixed theme, {key: (old color, new color)}, checks that still fail)."""
    fixer = Fixer(theme, levels or checks.LEVELS)
    failures = fixer.run()
    return fixer.data, fixer.changes(), failures


def _tidy(before: dict, after: dict) -> dict:
    """`after` with the keys `before` had in their order and new keys in the order the kit
    declares them: colors at the end of `colors`, highlight keys just before `syntax`."""
    colors = after.get("colors") or {}
    old = list((before.get("colors") or {}).keys())
    new = sorted((k for k in colors if k not in old), key=lambda k: kit.COLOR_KEYS.index(k))
    if colors:
        after["colors"] = {k: colors[k] for k in old + new}
    hl = after.get("highlight") or {}
    old = list((before.get("highlight") or {}).keys())
    new = sorted((k for k in hl if k not in old), key=lambda k: kit.HIGHLIGHT_KEYS.index(k))
    if new:
        at = old.index("syntax") if "syntax" in old else len(old)
        after["highlight"] = {k: hl[k] for k in old[:at] + new + old[at:]}
    return {k: after[k] for k in list(before) + [k for k in after if k not in before]}


def cmd_fix(args, files: list[Path]) -> int:
    status = 0
    for path in files:
        data = json.loads(path.read_text())
        lines, total = [], 0
        for i, theme in enumerate(data.get("themes") or []):
            fixed, changed, failures = fix_theme(theme)
            data["themes"][i] = _tidy(theme, fixed)
            total += len(changed)
            state = "ok" if not failures else f"{len(failures)} check(s) still fail"
            lines.append(f"  {theme.get('name')}: {state}, {len(changed)} color(s) moved")
            if args.verbose:
                for key, (old, new) in sorted(changed.items()):
                    lines.append(f"    {key[2:]}: {old} -> {new}")
            for res in failures:
                lines.append(f"    FAIL {res.check.level} {res.ratio:.2f} {res.fg_hex} on {res.bg_hex} {res.check.label}")
            status |= bool(failures)
        if total and not args.dry_run:
            path.write_text(generate.dump(data))
        print(f"{'would fix' if args.dry_run else 'fixed'} {path}" if total else f"ok   {path}")
        for line in lines:
            print(line)
    return status
