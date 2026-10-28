"""
Builds the site's web fonts in src/app/fonts/ from the @fontsource packages.

    cd apps/website
    uv run --no-project --with "fonttools[woff]" python scripts/subset-fonts.py

The output is committed, so a normal `pnpm build` needs no Python. Run this
again only after changing a font package or the character set below.

What it does to each face:

- Keeps the Latin characters the @fontsource "latin" files cover, plus the
  symbols the site uses (arrows, check mark, box drawing in code), and drops
  every other glyph.
- Keeps only the OpenType features the browser applies by default, plus
  `tnum` (tabular numbers) and `ordn`; stylistic sets nobody uses are gone.
- Mona Sans: pins the width axis at 112 (the semi-expanded cut every heading
  uses, see `--font-display` in globals.css) and keeps weights 500 to 650.
- Geist: keeps weights 400 to 700 (the site uses 400 to 700).
- Monaspace Neon: static faces, kept as they are apart from the subsetting.

Hinting is kept, so text renders exactly as it did with the full files.
"""

from __future__ import annotations

import io
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = Path(__file__).resolve().parent.parent
MODULES = ROOT / "node_modules"
OUT = ROOT / "src" / "app" / "fonts"

# The @fontsource "latin" range, plus symbols the site's text uses.
UNICODES = (
    "20-7E,A0-FF,131,152-153,2BB-2BC,2C6,2DA,2DC,304,308,329,2000-206F,20AC,2122,"
    "2190-2193,2212,2215,2265,2318,23BF,2500-257F,25CF,2713,FEFF,FFFD"
)

# (source file, output name, axis limits)
FACES: list[tuple[str, str, dict[str, float | tuple[float, float]]]] = [
    (
        "@fontsource-variable/mona-sans/files/mona-sans-latin-standard-normal.woff2",
        "mona-sans-display.woff2",
        {"wdth": 112, "wght": (500, 650)},
    ),
    ("@fontsource-variable/geist/files/geist-latin-wght-normal.woff2", "geist.woff2", {"wght": (400, 700)}),
    ("@fontsource-variable/geist/files/geist-latin-wght-italic.woff2", "geist-italic.woff2", {"wght": (400, 700)}),
    ("@fontsource/monaspace-neon/files/monaspace-neon-latin-400-normal.woff2", "monaspace-neon-400.woff2", {}),
    ("@fontsource/monaspace-neon/files/monaspace-neon-latin-400-italic.woff2", "monaspace-neon-400-italic.woff2", {}),
    ("@fontsource/monaspace-neon/files/monaspace-neon-latin-600-normal.woff2", "monaspace-neon-600.woff2", {}),
]


def codepoints(spec: str) -> list[int]:
    points: list[int] = []
    for part in spec.split(","):
        start, _, end = part.partition("-")
        points.extend(range(int(start, 16), int(end or start, 16) + 1))
    return points


def build(source: Path, target: Path, limits: dict[str, float | tuple[float, float]]) -> None:
    font = TTFont(source, lazy=False)
    if limits:
        font = instancer.instantiateVariableFont(font, limits)
        # Reload the instance so the subsetter sees plain, fully loaded tables.
        buffer = io.BytesIO()
        font.flavor = None
        font.save(buffer)
        buffer.seek(0)
        font = TTFont(buffer, lazy=False)

    options = subset.Options()
    options.flavor = "woff2"
    options.layout_features = [*options.layout_features, "tnum", "ordn"]
    options.hinting = True
    options.name_IDs = ["*"]
    options.notdef_outline = True
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=codepoints(UNICODES))
    subsetter.subset(font)
    font.flavor = "woff2"
    font.save(target)
    print(f"{target.name}: {source.stat().st_size // 1024} KiB -> {target.stat().st_size // 1024} KiB")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for source, name, limits in FACES:
        build(MODULES / source, OUT / name, limits)


if __name__ == "__main__":
    main()
