# Themes

Theme files live in [`themes/`](../themes) and load through `ThemeRegistry`. The tools in
[`script/themes`](../script/themes) check a theme's contrast, build themes from small palette
specs and draw a preview of every theme. They need Python 3.11 or later and nothing else.

```sh
python3 script/themes check                     # every file in themes/
python3 script/themes check themes/macos.json   # one file
python3 script/themes generate                  # rebuild themes from script/themes/palettes/
python3 script/themes preview -o preview.html   # an HTML page showing every theme
```

## Checking a theme

`check` validates each file against [`.theme-schema.json`](../.theme-schema.json), then works
out the colors the kit will actually paint: it applies the same fallbacks for keys a theme
leaves out, derives the same hover and pressed colors and the same text for tinted status
buttons, caps `list.active.background` and `table.active.background` at 20% opacity and
`selection.background` at 30%, and composites every translucent color over the surface beneath
it. It then measures the contrast of each pair the interface draws: text on every surface,
list and table rows at rest, hovered and selected, menus, tabs, the title and status bars,
inputs, every button variant and state, semantic colors as text and as fills, focus rings,
borders, the scrollbar thumb, and in the editor the text, line numbers, the caret line's
number, gutter separator, caret, selection, search matches, diagnostics and every syntax color
on the editor background, on the active line and under a selection.

Each failing pair is listed with its ratio, the minimum it needed, the two colors and where
it appears. The exit status is 1 when anything fails.

| Option | Effect |
| --- | --- |
| `--summary` | One line per theme with pass counts per level |
| `--all` | List passing checks too |
| `--notes` | List keys the kit ignores, such as Zed's `panel.background` |
| `--json` | Machine-readable results |
| `--report` | Exit 0 even when checks fail |

The kit also reads the names Zed themes and older theme files use for a few keys, listed in
the schema: `link.foreground` for `link`, `drag_border` for `drag.border`, `comment.doc` for
`comment_doc` and so on. A theme that sets both names gets the kit's own.

A code editor draws its text in `editor.foreground`, and editor gutters draw line numbers in
`editor.line_number` and the caret line's number in `editor.active_line_number`. They default
to `foreground`, `muted.foreground` and the editor's text color.

## Contrast rules

Ratios are WCAG 2 contrast ratios. Each pair belongs to one level:

| Level | Minimum | Used for |
| --- | --- | --- |
| text | 4.5 | Body text, labels, button text, links, status text, syntax colors, text on selected or hovered rows |
| ui | 3.0 | Focus ring, caret, the accent as a fill or as text (checked controls, sliders, progress, outline buttons), the active-row border, diagnostics, chart series, base colors |
| subtle | 3.0 | Line numbers, punctuation, inline hints, code under a selection |
| control | 1.4 | Input borders, the scrollbar thumb, the switch track when off |
| highlight | 1.2 | Text selection, the current search match, selected list and table rows, the active sidebar item |
| divider | 1.12 | Borders, separators, the gutter separator, whitespace markers |
| hover | 1.05 | Hover fills, other search matches, button hover against rest |

The text and ui levels follow WCAG 2.2 success criteria 1.4.3 (4.5:1 for text) and 1.4.11
(3:1 for the parts of a control that show its state). Line numbers, punctuation and inline
hints are meant to recede, so they use the 3:1 large-text bar.

The accent as text sits at the ui level on purpose. In a dark theme, white text on an accent
fill and the same accent as text on the background cannot both reach 4.5:1, and the kit uses
one `primary` color for both. Links have their own `link` color and are held to 4.5:1.

The four lowest levels keep quiet elements visible. They come from macOS: a 10% black
separator on white measures 1.25, a hover row of `#F0F0F0` on white 1.14 and the system blue
selection 1.49, so the minimums sit a little under those.

The report also gives the APCA lightness contrast (Lc) for text, subtle and ui pairs. It is
there for comparison, and a theme passes or fails on the WCAG ratios. As a guide, body text
reads well from Lc 60 and quiet text from Lc 45.

## Generating themes

`generate` builds a theme file from a palette spec in
[`script/themes/palettes`](../script/themes/palettes). Every generated file in `themes/`
comes from one of these specs, so edit the spec and regenerate rather than editing the
JSON. `generate --check` exits 1 when a file on disk differs from what its spec produces,
and `-v` lists every color the tuning step moved.

A spec names a few colors and derives the rest:

```toml
family = "Ember"
file = "ember.json"

[options]
shadow = true                      # any top-level theme key: shadow, radius, font.size…
active_line = "accent"             # tint the active line with the accent (default: neutral)

[options.styles]
comment = { font_style = "italic" }
keyword = { font_weight = 700 }

[[themes]]
name = "Ember Dark"
mode = "dark"
background = "oklch(0.205 0.012 55)"
foreground = "oklch(0.91 0.025 80)"
accent = "oklch(0.72 0.16 52)"
sidebar = "oklch(0.235 0.014 55)"  # optional, derived from the background when left out

[themes.semantic]                  # red, orange, yellow, green, cyan, blue, magenta
red = "oklch(0.70 0.17 28)"

[themes.syntax]                    # keyword, function, type, string, number, comment, …
keyword = "oklch(0.74 0.15 45)"
```

Colors are hex or `oklch(L C H)`. Optional roles include the surfaces (`editor`, `sidebar`,
`title_bar`, `status_bar`, `tab_bar`, `popover`, `control`), `muted`, `border`,
`input_border`, `gutter_border`, `selection`, `link`, `ring` and the syntax roles `property`,
`attribute`, `tag`, `operator`, `punctuation`, `escape`, `regex`, `special`, `preproc` and
`title`. `[[accents]]` entries (with `name`, `light` and `dark`) add a light and dark theme
per accent color, named by each theme's `accent_name` pattern. `[levels]` raises the bar for
a whole family.

The derivation rules:

- Surfaces such as hover fills, muted fills, borders, striped rows and the sidebar mix the
  background toward the foreground in OKLab by a small fraction, so they keep the
  background's hue.
- Hover and pressed button fills step away from the text on them, so a state change never
  lowers the text's contrast.
- The selection is the accent at 30% opacity and the selected list row the accent at 16%
  (20% in dark themes), the most the kit allows.
- Semantic colors serve as text and as fills. In a light theme the fills take white text,
  and in a dark theme they take the background color as text.
- Every color key the kit reads is written out, so a generated theme looks the same whatever
  the kit's built-in defaults.

After building a theme, `generate` runs the contrast checks. For each failure it moves the
OKLCH lightness of the color behind it a small step away from the color it sits on, keeping
hue and chroma, and repeats until every check passes. The surfaces a spec sets stay put, the
foreground moves only when nothing else can, and only the colors that fail move at all.

## Previewing

`preview` writes one self-contained HTML page with a small mock app for each theme: title
bar, sidebar with active and hovered rows, tabs, an editor with line numbers, syntax colors,
a selection and search matches, buttons, inputs, a menu and a status bar, each drawn from the
colors the kit resolves. Under each card a line counts the theme's contrast issues and opens
into a list grouped by part of the interface, with both colors, the ratio and the minimum.
Each issue is marked as coming from a color the theme sets or from the kit's fallback for a
key the theme leaves out. Filters at the top switch between generated and other themes and
between light and dark.

## Generated themes

| File | Themes | Character |
| --- | --- | --- |
| `macos.json` | macOS Light, macOS Dark | Neutral greys, the system blue, Xcode-style code colors |
| `ember.json` | Ember Light, Ember Dark | Ivory or charcoal lit by a glowing orange |
| `glacier.json` | Glacier Light, Glacier Dark | Frost and blue slate with an ice-cyan accent |
| `beacon.json` | Beacon Light, Beacon Dark | High contrast, with text at 7:1 |
| `sorbet.json` | Sorbet Light, Sorbet Dark | Soft pastels on lavender cream or plum grey |
| `mesa.json` | Mesa Light, Mesa Dark | High desert sand, terracotta and sage |
| `prism.json` | Prism Light, Prism Dark | Vivid, fully saturated colors |
| `lantern.json` | Lantern Light, Lantern Dark | Evening amber with almost no blue light |
| `folio.json` | Folio Light, Folio Dark | Paper and ink for reading and writing |
| `fern.json` | Fern Light, Fern Dark | Calm woodland greens |
| `radar.json` | Radar Green, Radar Amber | A phosphor screen in a dark room |
| `kiln.json` | Kiln Light, Kiln Dark | Warm charcoal or ivory with a fired-clay accent |
| `holly.json` | Holly Light, Holly Dark | Christmas: cranberry, pine, gold and snow |
| `haunt.json` | Haunt | Halloween night: pumpkin and witch violet on violet-black |
| `onyx.json` | Onyx | True black for OLED screens, with soft text and a mint accent |
| `midnight.json` | Midnight | Deep navy night sky with a gold accent |
| `ash.json` | Ash | A dim dark grey for people who find dark themes too dark |
| `reef.json` | Reef Light, Reef Dark | Sunlit aqua shallows or the deep sea |
| `gazette.json` | Gazette | Newsprint: black ink, headline red, little else |

## Tests

```sh
python3 -m unittest discover -s script/tests
```

The tests cover the contrast math, compare the checker's key tables and the schema with the
kit's Rust sources, and confirm every spec still produces passing themes identical to the files in
`themes/`.
