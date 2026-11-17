"""Builds the Allo web font the app ships: a 50KB Latin subset.

Allo (https://allo.joshpuckett.me, SIL Open Font License 1.1) is variable in
weight, roundness and optical size. Keeping every axis costs ~310KB, so this
script pins roundness at 80 and optical size at 24, keeps weight 400-800, and
keeps only Latin-1 plus the punctuation and symbols the UI uses.

Usage (needs Python 3.10+ and fonttools with brotli):
    pip install fonttools brotli
    python apps/client/scripts/subset-allo.py path/to/AlloVariable.ttf

AlloVariable.ttf is in the Allo-1.0.zip download (fonts/variable/). The result
is written to apps/client/public/fonts/allo-latin.woff2 and its metrics are
printed for the fallback font in src/styles.css.
"""

import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

UNICODES = (
    "U+0020-007E,U+00A0-00FF,U+2013-2014,U+2018-201D,U+2022,U+2026,U+20AC,"
    "U+2190-2193,U+2197,U+2212,U+2605,U+2665,U+2713,U+2715"
)
FEATURES = ["kern", "liga", "calt", "ccmp", "locl", "mark", "mkmk", "case", "tnum", "frac", "numr", "dnom"]
AXES = {"opsz": 24, "ROND": 80, "wght": (400, 800)}

OUT = Path(__file__).resolve().parent.parent / "public" / "fonts" / "allo-latin.woff2"


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    font = instancer.instantiateVariableFont(TTFont(sys.argv[1]), AXES)

    options = subset.Options()
    options.flavor = "woff2"
    options.layout_features = FEATURES
    options.name_IDs = ["*"]
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=subset.parse_unicodes(UNICODES))
    subsetter.subset(font)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    font.flavor = "woff2"
    font.save(OUT)

    upm = font["head"].unitsPerEm
    hhea = font["hhea"]
    avg = font["OS/2"].xAvgCharWidth
    print(f"Wrote {OUT} ({OUT.stat().st_size / 1024:.1f} KB)")
    print(f"unitsPerEm={upm} ascent={hhea.ascent} descent={hhea.descent} lineGap={hhea.lineGap} xAvgCharWidth={avg}")


if __name__ == "__main__":
    main()
