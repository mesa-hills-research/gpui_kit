"""The theme tools: contrast math, the replay of the kit's theme loading, and the generated themes."""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "themes"))

import checks  # noqa: E402
import colors as C  # noqa: E402
import fix  # noqa: E402
import generate  # noqa: E402
import kit  # noqa: E402
import validate  # noqa: E402


class ContrastMath(unittest.TestCase):
    def test_wcag_ratio(self):
        self.assertAlmostEqual(C.wcag(C.BLACK, C.WHITE), 21.0, places=6)
        self.assertAlmostEqual(C.wcag(C.parse_hex("#777777"), C.WHITE), 4.478, places=3)

    def test_apca_reference_values(self):
        # Published APCA-W3 0.0.98G-4g reference pairs.
        self.assertAlmostEqual(C.apca(C.parse_hex("#888888"), C.WHITE), 63.06, places=1)
        self.assertAlmostEqual(C.apca(C.WHITE, C.parse_hex("#888888")), -68.54, places=1)
        self.assertAlmostEqual(C.apca(C.BLACK, C.WHITE), 106.04, places=1)
        self.assertAlmostEqual(C.apca(C.WHITE, C.BLACK), -107.88, places=1)

    def test_oklch_round_trip(self):
        for hex_ in ("#0A84FF", "#1D1D1F", "#FFCC00", "#30B0C7"):
            c = C.parse_hex(hex_)
            self.assertEqual(C.to_hex(C.from_oklch(*C.to_oklch(c))), hex_)

    def test_compositing_matches_the_macos_selection(self):
        # The system blue at 30% over white is the macOS text selection color.
        self.assertEqual(C.to_hex(C.flatten([C.with_alpha(C.parse_hex("#007AFF"), 0.3)])), "#B2D7FF")


class KitReplay(unittest.TestCase):
    def test_key_tables_match_the_rust_sources(self):
        source = kit.keys_from_source()
        self.assertEqual(source["theme"], kit.THEME_KEYS)
        self.assertEqual(source["colors"], kit.COLOR_KEYS)
        self.assertEqual(source["color_aliases"], kit.COLOR_ALIASES)
        self.assertEqual(source["syntax"], kit.SYNTAX_KEYS)
        self.assertEqual(source["syntax_aliases"], kit.SYNTAX_ALIASES)
        self.assertEqual(set(source["highlight"]), set(kit.HIGHLIGHT_KEYS))

    def test_schema_lists_every_key_the_kit_reads(self):
        source = kit.keys_from_source()
        defs = json.loads(validate.SCHEMA_PATH.read_text())["$defs"]
        props = lambda name: set(defs[name]["properties"])  # noqa: E731
        self.assertEqual(props("ThemeConfig"), set(source["theme"]))
        self.assertEqual(props("ThemeConfigColors"), set(source["colors"]) | set(source["color_aliases"]))
        self.assertEqual(props("HighlightThemeStyle"), set(source["highlight"]))
        self.assertEqual(props("SyntaxColors"), set(source["syntax"]) | set(source["syntax_aliases"]))
        for name in ("ThemeConfigColors", "HighlightThemeStyle"):
            for key, prop in defs[name]["properties"].items():
                self.assertTrue(prop.get("description"), f"{name}.{key} has no description")

    def test_aliases_apply_unless_the_key_is_set(self):
        theme = {"name": "T", "mode": "light", "colors": {
            "link.foreground": "#112233", "drag_border": "#445566", "drag.border": "#778899",
        }, "highlight": {"syntax": {"comment.doc": {"color": "#AA0000"}}}}
        r = kit.resolve_theme(theme)
        self.assertEqual(C.to_hex(r.tokens["link"].color), "#112233")
        self.assertEqual(C.to_hex(r.tokens["link.hover"].color), "#112233")
        self.assertEqual(C.to_hex(r.tokens["drag.border"].color), "#778899")
        self.assertEqual(C.to_hex(kit.syntax_color(r.highlight["syntax"], "comment.doc")), "#AA0000")

    def test_derived_tinted_button_text_reads_in_every_state(self):
        themes = [
            {"name": "L", "mode": "light", "colors": {}},
            {"name": "D", "mode": "dark", "colors": {}},
            {"name": "Pale", "mode": "light", "colors": {
                "background": "#FFFFFF", "danger.background": "#FF8080", "warning.background": "#FFE066",
                "success.background": "#7CE08A", "info.background": "#7FD8F0"}},
            {"name": "Deep", "mode": "dark", "colors": {
                "background": "#202020", "danger.background": "#8B1A1A", "warning.background": "#7A5C00",
                "success.background": "#1E5E2A", "info.background": "#14506B"}},
        ]
        for theme in themes:
            results = {res.check.id: res for res in checks.evaluate(kit.resolve_theme(theme))}
            for status in ("danger", "success", "warning", "info"):
                for state in ("", "hover.", "active."):
                    with self.subTest(theme=theme["name"], button=status + " " + state):
                        self.assertGreaterEqual(results[f"button.{status}.{state}text"].ratio, 4.5)

    def test_editor_keys_are_checked(self):
        theme = {"name": "T", "mode": "dark", "colors": {"foreground": "#FFFFFF", "muted.foreground": "#FFFFFF"},
                 "highlight": {"editor.background": "#000000", "editor.foreground": "#333333",
                               "editor.line_number": "#222222", "syntax": {}}}
        results = {res.check.id: res for res in checks.evaluate(kit.resolve_theme(theme))}
        self.assertFalse(results["editor.text"].ok)
        self.assertFalse(results["editor.line_number"].ok)
        # The active line's number falls back to the editor's text color.
        self.assertEqual(results["editor.active_line_number"].fg_hex, "#333333")

    def test_missing_keys_fall_back_like_the_kit(self):
        theme = {"name": "T", "mode": "light", "colors": {"primary.background": "#0000FF", "background": "#FFFFFF"}}
        tokens = kit.resolve_theme(theme).tokens
        # selection falls back to primary and is capped at 30% opacity
        self.assertEqual(C.to_hex(tokens["selection.background"].color), "#0000FF4C")
        # the danger button is the danger color at 20%, its text the danger color made darker
        danger = tokens["danger.background"].color
        text = tokens["button.danger.foreground"].color
        self.assertAlmostEqual(tokens["button.danger.background"].color[3], danger[3] * 0.2)
        self.assertLess(C.to_oklch(text)[0], C.to_oklch(danger)[0])
        self.assertAlmostEqual(C.rgb_to_hsl(text)[0], C.rgb_to_hsl(danger)[0], places=2)

    def test_list_active_is_capped(self):
        theme = {"name": "T", "mode": "dark", "colors": {"list.active.background": "#FF0000"}}
        tokens = kit.resolve_theme(theme).tokens
        self.assertAlmostEqual(tokens["list.active.background"].color[3], 0.2)

    def test_gradient_values_check_both_stops(self):
        theme = {"name": "T", "mode": "light", "colors": {
            "background": "#FFFFFF", "foreground": "#000000",
            "button.background": "linear-gradient(180deg, #FFFFFF, #000000)",
        }}
        results = {r.check.id: r for r in checks.evaluate(kit.resolve_theme(theme))}
        self.assertLess(results["button.text"].ratio, 1.01)

    def test_an_alias_beside_its_key_is_flagged(self):
        data = {"name": "T", "themes": [{"name": "T", "mode": "light", "colors": {"link.foreground": "#000000"},
                                         "highlight": {"syntax": {"comment.doc": {"color": "#000000"}}}}]}
        self.assertEqual(validate.validate(data)[1], [])
        theme = data["themes"][0]
        theme["colors"]["link"] = "#111111"
        theme["highlight"]["syntax"]["comment_doc"] = {"color": "#111111"}
        _, notes = validate.validate(data)
        self.assertTrue(any("link.foreground is ignored" in n for n in notes))
        self.assertTrue(any("comment.doc is ignored" in n for n in notes))


class FixingThemes(unittest.TestCase):
    THEME = {
        "name": "Faded", "mode": "light",
        "colors": {
            "background": "#FFFFFF", "foreground": "#222222", "border": "#E5E5E5",
            "muted.foreground": "#B0B0B0", "link": "#7AA7E8", "list.active.background": "#3B82F608",
            "primary.background": "#2563EB", "primary.foreground": "#FFFFFF",
            "button.hover.background": "linear-gradient(180deg, #FFFFFF, #FFFFFF)",
        },
        "highlight": {"editor.background": "#FFFFFF", "syntax": {"comment": {"color": "#C8C8C8"}}},
    }

    def test_a_fixed_theme_passes_and_keeps_what_passed(self):
        fixed, changed, failures = fix.fix_theme(self.THEME)
        self.assertEqual(failures, [])
        self.assertEqual([r for r in checks.evaluate(kit.resolve_theme(fixed)) if not r.ok], [])
        for key in ("background", "foreground", "border", "primary.background", "primary.foreground"):
            self.assertEqual(fixed["colors"][key], self.THEME["colors"][key])
        self.assertEqual(fixed["highlight"]["editor.background"], "#FFFFFF")
        # Text that was too light got darker in the same hue.
        old, new = (C.parse_hex(v) for v in changed["c:link"])
        self.assertLess(C.to_oklch(new)[0], C.to_oklch(old)[0])
        self.assertAlmostEqual(C.to_oklch(new)[2], C.to_oklch(old)[2], delta=3)
        self.assertNotEqual(fixed["highlight"]["syntax"]["comment"]["color"], "#C8C8C8")
        # A gradient moves as a whole.
        self.assertTrue(fixed["colors"]["button.hover.background"].startswith("linear-gradient(180deg, #"))

    def test_a_tint_too_faint_for_any_lightness_gets_more_opacity(self):
        fixed, changed, _ = fix.fix_theme(self.THEME)
        color = C.parse_hex(fixed["colors"]["list.active.background"])
        self.assertGreater(color[3], 0x08 / 255)
        self.assertEqual(C.to_hex(C.with_alpha(color, 1)), "#3B82F6")

    def test_a_passing_theme_is_left_alone(self):
        spec = generate.load_spec(generate.PALETTES / "macos.toml")
        theme = generate.generate_family(spec)[0]["themes"][0]
        fixed, changed, failures = fix.fix_theme(theme)
        self.assertEqual((fixed, changed, failures), (theme, {}, []))


class GeneratedThemes(unittest.TestCase):
    def test_every_spec_builds_passing_themes_that_match_the_files(self):
        for path in sorted(generate.PALETTES.glob("*.toml")):
            spec = generate.load_spec(path)
            with self.subTest(spec=path.name):
                data, _, failures = generate.generate_family(spec)
                self.assertEqual(failures, [])
                errors, notes = validate.validate(data)
                self.assertEqual(errors, [])
                self.assertEqual(notes, [])
                on_disk = kit.ROOT / "themes" / spec["file"]
                self.assertEqual(on_disk.read_text(), generate.dump(data),
                                 f"{on_disk.name} is out of date, run `python3 script/themes generate`")

    def test_written_file_loads(self):
        spec = generate.load_spec(generate.PALETTES / "macos.toml")
        data, _, _ = generate.generate_family(spec)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "t.json"
            path.write_text(generate.dump(data))
            self.assertEqual(json.loads(path.read_text())["name"], "macOS")


if __name__ == "__main__":
    unittest.main()
