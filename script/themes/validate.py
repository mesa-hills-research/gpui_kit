"""Validate a theme file against `.theme-schema.json` and against the keys the kit reads."""

from __future__ import annotations

import json
import re
from pathlib import Path

import kit

SCHEMA_PATH = kit.ROOT / ".theme-schema.json"


def _check(value, schema: dict, root: dict, path: str, errors: list[str]) -> None:
    """A small JSON Schema (draft 7) validator covering what the theme schema uses."""
    if "$ref" in schema:
        node = root
        for part in schema["$ref"].lstrip("#/").split("/"):
            node = node[part]
        _check(value, node, root, path, errors)
        return
    if "anyOf" in schema:
        for option in schema["anyOf"]:
            sub: list[str] = []
            _check(value, option, root, path, sub)
            if not sub:
                break
        else:
            errors.append(f"{path}: {json.dumps(value)[:60]} matches none of the allowed forms")
        return
    types = schema.get("type")
    if types is not None:
        types = types if isinstance(types, list) else [types]
        kinds = {
            "string": lambda v: isinstance(v, str),
            "number": lambda v: isinstance(v, (int, float)) and not isinstance(v, bool),
            "integer": lambda v: isinstance(v, int) and not isinstance(v, bool),
            "boolean": lambda v: isinstance(v, bool),
            "null": lambda v: v is None,
            "object": lambda v: isinstance(v, dict),
            "array": lambda v: isinstance(v, list),
        }
        if not any(kinds[t](value) for t in types):
            errors.append(f"{path}: expected {' or '.join(types)}, found {type(value).__name__}")
            return
    if "enum" in schema and value not in schema["enum"]:
        errors.append(f"{path}: {value!r} is not one of {schema['enum']}")
    if "pattern" in schema and isinstance(value, str) and not re.search(schema["pattern"], value):
        errors.append(f"{path}: {value!r} does not match {schema['pattern']}")
    if isinstance(value, dict):
        for key in schema.get("required", []):
            if key not in value:
                errors.append(f"{path}: missing required key {key!r}")
        props = schema.get("properties", {})
        for key, sub in value.items():
            if key in props:
                _check(sub, props[key], root, f"{path}.{key}" if path else key, errors)
    if isinstance(value, list) and "items" in schema:
        for i, item in enumerate(value):
            _check(item, schema["items"], root, f"{path}[{i}]", errors)


def validate(data, schema: dict | None = None) -> tuple[list[str], list[str]]:
    """Return (errors, notes) for a parsed theme file."""
    if schema is None:
        schema = json.loads(SCHEMA_PATH.read_text())
    errors: list[str] = []
    notes: list[str] = []
    _check(data, schema, schema, "", errors)
    if not isinstance(data, dict):
        return errors, notes
    names = set()
    for i, theme in enumerate(data.get("themes") or []):
        if not isinstance(theme, dict):
            continue
        where = f"themes[{i}] ({theme.get('name', '?')})"
        name = theme.get("name")
        if name in names:
            errors.append(f"{where}: duplicate theme name")
        names.add(name)
        for key in theme:
            if key not in kit.THEME_KEYS:
                notes.append(f"{where}: {key!r} is not read by the kit")
        colors = theme.get("colors") or {}
        for key, value in colors.items():
            if key in kit.COLOR_ALIASES and kit.COLOR_ALIASES[key] in colors:
                notes.append(f"{where}: colors.{key} is ignored, the theme sets colors.{kit.COLOR_ALIASES[key]}")
                continue
            if key not in kit.COLOR_KEYS and key not in kit.COLOR_ALIASES:
                notes.append(f"{where}: colors.{key} is not read by the kit")
                continue
            if not isinstance(value, str):
                continue
            try:
                kit.parse_token(value)
            except ValueError as err:
                errors.append(f"{where}: colors.{key}: {err}")
        hl = theme.get("highlight") or {}
        ignored = [k for k in hl if k not in kit.HIGHLIGHT_KEYS]
        if ignored:
            notes.append(f"{where}: highlight keys not read by the kit: {', '.join(ignored)}")
        for key in kit.HIGHLIGHT_KEYS:
            value = hl.get(key)
            if key != "syntax" and isinstance(value, str):
                try:
                    kit.C.parse_hex(value)
                except ValueError as err:
                    errors.append(f"{where}: highlight.{key}: {err}")
        syntax = hl.get("syntax") or {}
        for key, style in syntax.items():
            if key in kit.SYNTAX_ALIASES and kit.SYNTAX_ALIASES[key] in syntax:
                notes.append(f"{where}: syntax.{key} is ignored, the theme sets syntax.{kit.SYNTAX_ALIASES[key]}")
            elif key not in kit.SYNTAX_KEYS and key not in kit.SYNTAX_ALIASES:
                notes.append(f"{where}: syntax.{key} is not read by the kit")
            elif isinstance(style, dict) and isinstance(style.get("color"), str):
                try:
                    kit.C.parse_hex(style["color"])
                except ValueError as err:
                    errors.append(f"{where}: syntax.{key}: {err}")
    return errors, notes


def load(path: Path):
    """Parse a theme file. Returns (data, error)."""
    try:
        return json.loads(Path(path).read_text()), None
    except (OSError, json.JSONDecodeError) as err:
        return None, str(err)
