#!/usr/bin/env python3
"""Romanian glyph coverage gate for shipped font files.

    python check-glyphs.py <font> [<font> ...]           # each file must cover the set on its own
    python check-glyphs.py --union <subset> [<subset>]   # the SET of subset files must cover it together
    python check-glyphs.py --extra 201E,201D <font>      # also require extra code points (hex)

Required: U+0218 S-comma, U+0219 s-comma, U+021A T-comma, U+021B t-comma (comma-below, NOT
the cedilla forms U+015E/F, U+0162/3), U+0102/U+0103 A-breve, U+00C2/U+00E2 A-circumflex,
U+00CE/U+00EE I-circumflex.

Exit 0 = covered, 1 = missing glyphs (per file without --union, for the union with --union),
2 = usage / unreadable font. Needs `fonttools`; WOFF2 also needs `brotli`
(`pip install fonttools brotli` inside a project venv, never global).
Cedilla-only coverage is reported as a warning: fonts that map only U+015E/U+0162 render
Romanian text wrongly even though it "looks close".
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

REQUIRED = {
    0x0218: "Ș", 0x0219: "ș", 0x021A: "Ț", 0x021B: "ț",
    0x0102: "Ă", 0x0103: "ă", 0x00C2: "Â", 0x00E2: "â", 0x00CE: "Î", 0x00EE: "î",
}
CEDILLA = {0x015E: "Ş", 0x015F: "ş", 0x0162: "Ţ", 0x0163: "ţ"}


def load_cmap(path: Path) -> set[int]:
    try:
        from fontTools.ttLib import TTFont
    except ImportError:
        sys.exit("error: fontTools not installed (pip install fonttools brotli in a venv)")
    if path.suffix.lower() == ".woff2":
        try:
            import brotli  # noqa: F401
        except ImportError:
            sys.exit(f"error: {path.name} is WOFF2 and needs the 'brotli' package")
    with TTFont(str(path), lazy=True) as font:
        cmap = font.getBestCmap() or {}
    return set(cmap)


def label(cps: list[int], table: dict[int, str]) -> str:
    return " ".join(f"{table.get(c, chr(c))}(U+{c:04X})" for c in cps)


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description="Romanian glyph coverage gate")
    ap.add_argument("fonts", nargs="+", type=Path)
    ap.add_argument("--union", action="store_true", help="judge the files together (unicode-range subsets)")
    ap.add_argument("--extra", default="", help="comma-separated extra hex code points to require")
    args = ap.parse_args(argv)

    required = dict(REQUIRED)
    for tok in filter(None, (t.strip() for t in args.extra.split(","))):
        cp = int(tok.upper().removeprefix("U+"), 16)
        required[cp] = chr(cp)

    union: set[int] = set()
    any_missing = False
    for path in args.fonts:
        if not path.is_file():
            print(f"error: not a file: {path}", file=sys.stderr)
            return 2
        try:
            cps = load_cmap(path)
        except Exception as exc:  # corrupt or unsupported font file
            print(f"error: cannot read {path}: {exc}", file=sys.stderr)
            return 2
        union |= cps
        missing = [c for c in required if c not in cps]
        any_missing |= bool(missing)
        status = "OK" if not missing else "MISSING " + label(missing, required)
        print(f"{path.name}: {status} ({len(cps)} code points)")
        cedilla_only = [c for c in (0x015E, 0x0162) if c in cps] and 0x0218 not in cps
        if cedilla_only:
            print(f"  warning: {path.name} has cedilla Ş/Ţ but no comma-below Ș/Ț")

    if args.union:
        missing = [c for c in required if c not in union]
        status = "OK" if not missing else "MISSING " + label(missing, required)
        print(f"UNION of {len(args.fonts)} file(s): {status} ({len(union)} code points)")
        return 1 if missing else 0

    print("RESULT:", "FAIL (at least one file is missing glyphs; use --union for subset sets)" if any_missing else "OK")
    return 1 if any_missing else 0


if __name__ == "__main__":
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.exit(main(sys.argv[1:]))
