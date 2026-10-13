#!/usr/bin/env python3
"""Glyph-coverage check for the bundled Cubic 11 subset (spec 4.4 / 5.6).

Every non-ASCII, non-whitespace character in the shipped web text
(rust/web/**/*.html|js|css, tests excluded) must be in
rust/web/fonts/cubic11.woff2. The characters the font lacks and the UI
draws as SVG (✕ ◀ ▶ ↖ ↗ ↙ ↘) must not appear in that text at all.

Needs fontTools + brotli: run with $DSM_FONT_PY or
~/.cache/dualsense-mapper-tools/venv/bin/python. Exit 0 = ok, 1 = missing.
"""
import pathlib, sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
WEB = ROOT / "rust" / "web"
FONT = WEB / "fonts" / "cubic11.woff2"
SVG_ONLY = "✕◀▶↖↗↙↘"


def web_text():
    out = {}
    for p in sorted(WEB.rglob("*")):
        if p.suffix not in (".html", ".js", ".css") or "tests" in p.relative_to(WEB).parts:
            continue
        out[p.relative_to(ROOT)] = p.read_text(encoding="utf-8")
    return out


def main():
    from fontTools.ttLib import TTFont
    if not FONT.exists():
        print(f"no {FONT.relative_to(ROOT)}: run scripts/build-font.py")
        return 1
    cmap = TTFont(FONT).getBestCmap()
    missing, banned = {}, {}
    for path, text in web_text().items():
        for ch in set(text):
            if ord(ch) < 0x80 or ch.isspace():
                continue
            if ch in SVG_ONLY:
                banned.setdefault(ch, path)
            elif ord(ch) not in cmap:
                missing.setdefault(ch, path)
    if banned:
        print("drawn as SVG, must not be typed: " + " ".join(f"{c} ({p})" for c, p in sorted(banned.items())))
    if missing:
        print("missing: " + " ".join(f"{c} U+{ord(c):04X} ({p})" for c, p in sorted(missing.items())))
    if banned or missing:
        return 1
    print(f"font-coverage: ok ({len(cmap)} glyphs)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
