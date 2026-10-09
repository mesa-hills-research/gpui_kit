"""Color math for the theme tools: parsing, OKLab/OKLCH, compositing, WCAG 2 and APCA contrast.

Colors are tuples of floats `(r, g, b, a)` in 0..1, in sRGB with straight (not premultiplied)
alpha, which is how gpui stores `Rgba`.
"""

from __future__ import annotations

import math
import re

RGBA = tuple  # (r, g, b, a)

WHITE = (1.0, 1.0, 1.0, 1.0)
BLACK = (0.0, 0.0, 0.0, 1.0)
TRANSPARENT = (0.0, 0.0, 0.0, 0.0)

_HEX = re.compile(r"^#([0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$")


def is_hex(value: str) -> bool:
    return bool(_HEX.match(value.strip()))


def parse_hex(value: str) -> RGBA:
    """Parse `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`, as gpui does."""
    m = _HEX.match(value.strip())
    if not m:
        raise ValueError(f"invalid hex color: {value!r}")
    h = m.group(1)
    if len(h) in (3, 4):
        h = "".join(c * 2 for c in h)
    r, g, b = (int(h[i : i + 2], 16) / 255 for i in (0, 2, 4))
    a = int(h[6:8], 16) / 255 if len(h) == 8 else 1.0
    return (r, g, b, a)


def to_hex(c: RGBA, alpha: bool | None = None) -> str:
    """Format as `#RRGGBB`, or `#RRGGBBAA` when the color is translucent (or `alpha=True`)."""
    r, g, b, a = c
    parts = [round(max(0.0, min(1.0, v)) * 255) for v in (r, g, b)]
    s = "#" + "".join(f"{p:02X}" for p in parts)
    if alpha or (alpha is None and a < 0.999):
        s += f"{round(max(0.0, min(1.0, a)) * 255):02X}"
    return s


def with_alpha(c: RGBA, a: float) -> RGBA:
    return (c[0], c[1], c[2], a)


# ---------------------------------------------------------------------------
# Compositing


def over(top: RGBA, bottom: RGBA) -> RGBA:
    """Source-over compositing with straight alpha."""
    ta, ba = top[3], bottom[3]
    a = ta + ba * (1 - ta)
    if a <= 0:
        return TRANSPARENT
    rgb = tuple((top[i] * ta + bottom[i] * ba * (1 - ta)) / a for i in range(3))
    return (*rgb, a)


def flatten(layers: list[RGBA], base: RGBA = WHITE) -> RGBA:
    """Composite `layers` (bottom first) over an opaque `base` and return an opaque color."""
    c = base
    for layer in layers:
        c = over(layer, c)
    return (c[0], c[1], c[2], 1.0)


# ---------------------------------------------------------------------------
# sRGB, OKLab and OKLCH


def srgb_to_linear(c: float) -> float:
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def linear_to_srgb(c: float) -> float:
    if c <= 0.0031308:
        return 12.92 * c
    return 1.055 * (c ** (1 / 2.4)) - 0.055 if c > 0 else 12.92 * c


def to_oklab(c: RGBA) -> tuple[float, float, float]:
    lr, lg, lb = (srgb_to_linear(v) for v in c[:3])
    l = 0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb
    m = 0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb
    s = 0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb
    l_, m_, s_ = (math.copysign(abs(v) ** (1 / 3), v) for v in (l, m, s))
    return (
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    )


def _oklab_to_linear(L: float, a: float, b: float) -> tuple[float, float, float]:
    l_ = L + 0.3963377774 * a + 0.2158037573 * b
    m_ = L - 0.1055613458 * a - 0.0638541728 * b
    s_ = L - 0.0894841775 * a - 1.2914855480 * b
    l, m, s = l_**3, m_**3, s_**3
    return (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    )


def from_oklab(L: float, a: float, b: float, alpha: float = 1.0) -> RGBA:
    rgb = tuple(max(0.0, min(1.0, linear_to_srgb(v))) for v in _oklab_to_linear(L, a, b))
    return (*rgb, alpha)


def to_oklch(c: RGBA) -> tuple[float, float, float]:
    L, a, b = to_oklab(c)
    C = math.hypot(a, b)
    h = math.degrees(math.atan2(b, a)) % 360 if C > 1e-7 else 0.0
    return (L, C, h)


def _in_gamut(L: float, C: float, h: float, eps: float = 1e-5) -> bool:
    hr = math.radians(h)
    lin = _oklab_to_linear(L, C * math.cos(hr), C * math.sin(hr))
    return all(-eps <= v <= 1 + eps for v in lin)


def from_oklch(L: float, C: float, h: float, alpha: float = 1.0) -> RGBA:
    """OKLCH to sRGB, keeping lightness and hue and reducing chroma to fit the gamut."""
    L = max(0.0, min(1.0, L))
    if not _in_gamut(L, C, h):
        lo, hi = 0.0, C
        for _ in range(30):
            mid = (lo + hi) / 2
            if _in_gamut(L, mid, h):
                lo = mid
            else:
                hi = mid
        C = lo
    hr = math.radians(h)
    return from_oklab(L, C * math.cos(hr), C * math.sin(hr), alpha)


def mix(a: RGBA, b: RGBA, t: float) -> RGBA:
    """Interpolate opaque colors in OKLab: `t = 0` gives `a`, `t = 1` gives `b`."""
    la, lb = to_oklab(a), to_oklab(b)
    lab = tuple(la[i] + (lb[i] - la[i]) * t for i in range(3))
    alpha = a[3] + (b[3] - a[3]) * t
    return from_oklab(*lab, alpha)


def set_l(c: RGBA, L: float) -> RGBA:
    _, C, h = to_oklch(c)
    return from_oklch(L, C, h, c[3])


def shift_l(c: RGBA, d: float) -> RGBA:
    L, C, h = to_oklch(c)
    return from_oklch(L + d, C, h, c[3])


def parse_oklch(value: str) -> RGBA:
    """Parse `oklch(L C H)` or `oklch(L C H / A)`, L as 0..1 or a percentage."""
    m = re.match(r"^oklch\(\s*([0-9.]+%?)\s+([0-9.]+)\s+([0-9.]+)\s*(?:/\s*([0-9.]+%?)\s*)?\)$", value.strip())
    if not m:
        raise ValueError(f"invalid oklch color: {value!r}")
    L = float(m.group(1).rstrip("%")) / (100 if m.group(1).endswith("%") else 1)
    a = 1.0
    if m.group(4):
        a = float(m.group(4).rstrip("%")) / (100 if m.group(4).endswith("%") else 1)
    return from_oklch(L, float(m.group(2)), float(m.group(3)), a)


def parse_css(value: str) -> RGBA:
    """Parse a palette color: hex or `oklch(...)`."""
    value = value.strip()
    if value.startswith("oklch"):
        return parse_oklch(value)
    return parse_hex(value)


# ---------------------------------------------------------------------------
# gpui's HSL model (used to replay the kit's `darken`, `lighten` and `blend`)


def rgb_to_hsl(c: RGBA) -> tuple[float, float, float, float]:
    r, g, b, a = c
    mx, mn = max(r, g, b), min(r, g, b)
    d = mx - mn
    l = (mx + mn) / 2
    if l == 0 or l == 1:
        s = 0.0
    elif l < 0.5:
        s = d / (2 * l)
    else:
        s = d / (2 - 2 * l)
    if d == 0:
        h = 0.0
    elif mx == r:
        h = ((g - b) / d % 6) / 6
    elif mx == g:
        h = ((b - r) / d + 2) / 6
    else:
        h = ((r - g) / d + 4) / 6
    return (h, s, l, a)


def hsl_to_rgb(h: float, s: float, l: float, a: float = 1.0) -> RGBA:
    c = (1 - abs(2 * l - 1)) * s
    x = c * (1 - abs((h * 6) % 2 - 1))
    m = l - c / 2
    cm, xm = c + m, x + m
    sector = math.floor(h * 6)
    r, g, b = {
        0: (cm, xm, m),
        6: (cm, xm, m),
        1: (xm, cm, m),
        2: (m, cm, xm),
        3: (m, xm, cm),
        4: (xm, m, cm),
    }.get(sector, (cm, m, xm))
    return (max(0.0, min(1.0, r)), max(0.0, min(1.0, g)), max(0.0, min(1.0, b)), a)


def hsl_darken(c: RGBA, f: float) -> RGBA:
    h, s, l, a = rgb_to_hsl(c)
    return hsl_to_rgb(h, s, l * (1 - max(0.0, min(1.0, f))), a)


def hsl_lighten(c: RGBA, f: float) -> RGBA:
    h, s, l, a = rgb_to_hsl(c)
    return hsl_to_rgb(h, s, l * (1 + max(0.0, min(1.0, f))), a)


def hsl_set_saturation(c: RGBA, s: float) -> RGBA:
    h, _, l, a = rgb_to_hsl(c)
    return hsl_to_rgb(h, s, l, a)


def blend(base: RGBA, other: RGBA) -> RGBA:
    """gpui's `Hsla::blend`: paint `other` over `base`, keeping `base`'s alpha."""
    if other[3] >= 1:
        return other
    if other[3] <= 0:
        return base
    t = other[3]
    return (*(base[i] * (1 - t) + other[i] * t for i in range(3)), base[3])


def opacity(c: RGBA, f: float) -> RGBA:
    return (c[0], c[1], c[2], c[3] * max(0.0, min(1.0, f)))


# ---------------------------------------------------------------------------
# Contrast


def luminance(c: RGBA) -> float:
    """WCAG 2 relative luminance of an opaque color."""
    r, g, b = (srgb_to_linear(v) for v in c[:3])
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def wcag(fg: RGBA, bg: RGBA) -> float:
    """WCAG 2 contrast ratio of two opaque colors (1..21)."""
    l1, l2 = luminance(fg), luminance(bg)
    hi, lo = max(l1, l2), min(l1, l2)
    return (hi + 0.05) / (lo + 0.05)


def apca(text: RGBA, bg: RGBA) -> float:
    """APCA-W3 0.0.98G-4g lightness contrast Lc. Positive for dark text on light."""

    def y(c: RGBA) -> float:
        v = 0.2126729 * c[0] ** 2.4 + 0.7151522 * c[1] ** 2.4 + 0.0721750 * c[2] ** 2.4
        return v + (0.022 - v) ** 1.414 if v < 0.022 else v

    yt, yb = y(text), y(bg)
    if abs(yb - yt) < 0.0005:
        return 0.0
    if yb > yt:
        sapc = (yb**0.56 - yt**0.57) * 1.14
        out = 0.0 if sapc < 0.1 else sapc - 0.027
    else:
        sapc = (yb**0.65 - yt**0.62) * 1.14
        out = 0.0 if sapc > -0.1 else sapc + 0.027
    return out * 100


def best_text_on(fill: RGBA, light: RGBA = WHITE, dark: RGBA = BLACK) -> RGBA:
    """Whichever of `light` and `dark` contrasts more with `fill`."""
    return light if wcag(light, fill) >= wcag(dark, fill) else dark
