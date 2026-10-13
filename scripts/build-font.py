#!/usr/bin/env python3
"""Subset Cubic 11 (OFL) to the characters of the shipped web UI.

Writes rust/web/fonts/cubic11.woff2 from
~/.cache/dualsense-mapper-tools/fonts/Cubic_11.ttf, keeping every character
found in rust/web/**/*.html|js|css (tests excluded) plus printable ASCII.
Same subset options as the mockup's tools/embed_fonts.py. Glyphs typed
later (macro names) that are not in the subset fall back to the system font.

Run with fontTools + brotli available ($DSM_FONT_PY or
~/.cache/dualsense-mapper-tools/venv/bin/python). Re-run after adding UI text;
scripts/verify/font-coverage.py fails until you do.
"""
import os, pathlib, sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
WEB = ROOT / "rust" / "web"
OUT = WEB / "fonts" / "cubic11.woff2"
SRC = pathlib.Path(os.path.expanduser("~/.cache/dualsense-mapper-tools/fonts/Cubic_11.ttf"))


def main():
    from fontTools import subset
    from fontTools.ttLib import TTFont
    if not SRC.exists():
        print(f"missing source font {SRC}")
        return 1
    chars = set(chr(c) for c in range(0x20, 0x7F))
    for p in WEB.rglob("*"):
        if p.suffix in (".html", ".js", ".css") and "tests" not in p.relative_to(WEB).parts:
            chars |= set(p.read_text(encoding="utf-8"))
    text = "".join(sorted(c for c in chars if not c.isspace() or c == " "))
    font = TTFont(SRC)
    opts = subset.Options()
    opts.flavor = "woff2"
    opts.layout_features = ["*"]
    sub = subset.Subsetter(opts)
    sub.populate(text=text)
    sub.subset(font)
    font.flavor = "woff2"
    OUT.parent.mkdir(parents=True, exist_ok=True)
    font.save(OUT)
    print(f"{OUT.relative_to(ROOT)}: {OUT.stat().st_size} bytes, {len(text)} characters")
    return 0


if __name__ == "__main__":
    sys.exit(main())
