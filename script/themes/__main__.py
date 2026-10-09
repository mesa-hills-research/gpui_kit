"""Theme tools for gpui-component.

    python3 script/themes check [FILES...]      validate theme files and check their contrast
    python3 script/themes generate [SPECS...]   build theme files from palette specs
    python3 script/themes fix [FILES...]        move the colors that fail a check until they pass
    python3 script/themes preview [-o FILE]     write an HTML page previewing every theme

Run `python3 script/themes <command> --help` for the options.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import checks  # noqa: E402
import kit  # noqa: E402
import validate  # noqa: E402

THEMES_DIR = kit.ROOT / "themes"


def theme_files(paths: list[str]) -> list[Path]:
    if not paths:
        return sorted(THEMES_DIR.glob("*.json"))
    out = []
    for p in map(Path, paths):
        out.extend(sorted(p.glob("*.json")) if p.is_dir() else [p])
    return out


def check_file(path: Path) -> dict:
    """Validate one file and evaluate every theme in it."""
    data, err = validate.load(path)
    report = {"file": str(path), "errors": [], "notes": [], "themes": []}
    if err:
        report["errors"].append(err)
        return report
    report["errors"], report["notes"] = validate.validate(data)
    for theme in data.get("themes") or []:
        if not isinstance(theme, dict):
            continue
        resolved = kit.resolve_theme(theme)
        results = checks.evaluate(resolved)
        report["themes"].append({"name": resolved.name, "mode": resolved.mode, "results": results})
    return report


def _fmt_result(res: checks.Result) -> str:
    lc = f"  Lc {abs(res.apca):5.1f}" if res.apca is not None else ""
    mark = "ok  " if res.ok else "FAIL"
    return (
        f"    {mark} {res.check.level:<9} {res.ratio:5.2f} (min {res.minimum:g}){lc}  "
        f"{res.fg_hex} on {res.bg_hex}  {res.check.label}"
    )


def cmd_check(args) -> int:
    files = theme_files(args.files)
    reports = [check_file(f) for f in files]
    failed = 0
    if args.json:
        out = []
        for rep in reports:
            out.append({
                "file": rep["file"],
                "errors": rep["errors"],
                "notes": rep["notes"],
                "themes": [{
                    "name": t["name"],
                    "mode": t["mode"],
                    "summary": checks.summarize(t["results"]),
                    "failures": [{
                        "id": r.check.id, "level": r.check.level, "label": r.check.label,
                        "ratio": round(r.ratio, 3), "min": r.minimum,
                        "apca": None if r.apca is None else round(r.apca, 1),
                        "fg": r.fg_hex, "bg": r.bg_hex,
                    } for r in t["results"] if not r.ok],
                } for t in rep["themes"]],
            })
        json.dump(out, sys.stdout, indent=2)
        print()
    for rep in reports:
        bad = bool(rep["errors"]) or any(not r.ok for t in rep["themes"] for r in t["results"])
        failed += bad
        if args.json:
            continue
        print(f"{'FAIL' if bad else 'ok  '} {rep['file']}")
        for e in rep["errors"]:
            print(f"    error: {e}")
        if args.notes:
            for n in rep["notes"]:
                print(f"    note: {n}")
        elif rep["notes"]:
            print(f"    {len(rep['notes'])} note(s) about keys the kit ignores, --notes lists them")
        for t in rep["themes"]:
            fails = [r for r in t["results"] if not r.ok]
            summary = checks.summarize(t["results"])
            counts = "  ".join(
                f"{lvl} {s['checks'] - s['failed']}/{s['checks']}" for lvl, s in summary.items()
            )
            print(f"  {'FAIL' if fails else 'ok  '} {t['name']} ({t['mode']})  {counts}")
            if args.summary:
                continue
            shown = t["results"] if args.all else fails
            for res in sorted(shown, key=lambda r: (checks.LEVEL_ORDER.index(r.check.level), r.ratio)):
                print(_fmt_result(res))
    if not args.json:
        print(f"\n{len(reports) - failed} of {len(reports)} files pass")
    return 1 if failed and not args.report else 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="python3 script/themes", description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("check", help="validate theme files and check their contrast")
    p.add_argument("files", nargs="*", help="theme files or directories (default: themes/)")
    p.add_argument("--all", action="store_true", help="list passing checks too")
    p.add_argument("--summary", action="store_true", help="one line per theme")
    p.add_argument("--notes", action="store_true", help="list keys the kit ignores")
    p.add_argument("--json", action="store_true", help="machine-readable output")
    p.add_argument("--report", action="store_true", help="exit 0 even when a check fails")
    p.set_defaults(func=cmd_check)

    p = sub.add_parser("generate", help="build theme files from palette specs")
    p.add_argument("specs", nargs="*", help="palette specs (default: script/themes/palettes/*.toml)")
    p.add_argument("-o", "--out", default=str(THEMES_DIR), help="output directory (default: themes/)")
    p.add_argument("--check", action="store_true", help="fail if a generated file differs from the one on disk")
    p.add_argument("-v", "--verbose", action="store_true", help="list every lightness adjustment")
    p.set_defaults(func=lambda a: __import__("generate").cmd_generate(a))

    p = sub.add_parser("fix", help="move the colors that fail a check until they pass")
    p.add_argument("files", nargs="*", help="theme files or directories (default: themes/)")
    p.add_argument("-n", "--dry-run", action="store_true", help="report what would change, write nothing")
    p.add_argument("-v", "--verbose", action="store_true", help="list every color that moves")
    p.set_defaults(func=lambda a: __import__("fix").cmd_fix(a, theme_files(a.files)))

    p = sub.add_parser("preview", help="write an HTML preview of every theme")
    p.add_argument("files", nargs="*", help="theme files or directories (default: themes/)")
    p.add_argument("-o", "--out", default="themes-preview.html", help="output file")
    p.add_argument("--new", nargs="*", default=None, help="file names to label as new")
    p.set_defaults(func=lambda a: __import__("preview").cmd_preview(a, theme_files(a.files)))

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
