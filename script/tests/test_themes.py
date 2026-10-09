"""The theme tools: contrast math, the replay of the kit's theme loading, and the generated themes."""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "themes"))

import checks  # noqa: E402
import colors as C  # noqa: E402
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
        self.assertEqual(source["colors"], kit.COLOR_KEYS)
        self.assertEqual(source["syntax"], kit.SYNTAX_KEYS)
        self.assertEqual(set(source["highlight"]) - kit.PENDING_KEYS, set(kit.HIGHLIGHT_KEYS) - kit.PENDING_KEYS)

    def test_missing_keys_fall_back_like_the_kit(self):
        theme = {"name": "T", "mode": "light", "colors": {"primary.background": "#0000FF", "background": "#FFFFFF"}}
        tokens = kit.resolve_theme(theme).tokens
        # selection falls back to primary and is capped at 30% opacity
        self.assertEqual(C.to_hex(tokens["selection.background"].color), "#0000FF4C")
        # the danger button is the danger color at 20% with danger-colored text
        danger = tokens["danger.background"].color
        self.assertEqual(tokens["button.danger.foreground"].color, danger)
        self.assertAlmostEqual(tokens["button.danger.background"].color[3], danger[3] * 0.2)

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

    def test_comment_doc_spelling_is_flagged(self):
        data = {"name": "T", "themes": [{"name": "T", "mode": "light", "colors": {},
                                         "highlight": {"syntax": {"comment.doc": {"color": "#000000"}}}}]}
        _, notes = validate.validate(data)
        self.assertTrue(any("comment_doc" in n for n in notes))


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
