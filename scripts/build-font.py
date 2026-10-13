#!/usr/bin/env python3
"""Subset Cubic 11 (OFL) to the characters of the shipped web UI.

Writes rust/web/fonts/cubic11.woff2 from the pinned Cubic 11 source
(scripts/font_tools.py fetches and checks it), keeping every character
found in rust/web/**/*.html|js|css (tests excluded) plus printable ASCII.
Same subset options as the mockup's tools/embed_fonts.py. Glyphs typed
later (macro names) that are not in the subset fall back to the system font.

Run `python3 scripts/build-font.py`; it re-runs itself in the pinned venv
(scripts/font_tools.py). Re-run after adding UI text;
scripts/verify/font-coverage.py fails until you do.
"""
import pathlib, sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
WEB = ROOT / "rust" / "web"
OUT = WEB / "fonts" / "cubic11.woff2"
sys.path.insert(0, str(ROOT / "scripts"))
import font_tools  # noqa: E402


def main():
    from fontTools import subset
    from fontTools.ttLib import TTFont
    SRC = font_tools.ensure_font()
    chars = set(chr(c) for c in range(0x20, 0x7F))
    for p in WEB.rglob("*"):
        if p.suffix in (".html", ".js", ".css") and "tests" not in p.relative_to(WEB).parts:
            chars |= set(p.read_text(encoding="utf-8"))
    text = "".join(sorted(c for c in chars if not c.isspace() or c == " "))
    # Keep the source head.modified: a rebuild of the same text is byte-identical.
    font = TTFont(SRC, recalcTimestamp=False)
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
    font_tools.reexec_in_venv()
    sys.exit(main())
