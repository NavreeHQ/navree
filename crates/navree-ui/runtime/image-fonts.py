# Writes the fonts chart images are drawn with (`src/image`, the `image` feature):
#
#   assets/fonts/geist.ttf          the page's Geist (assets/fonts/geist.woff2) as TrueType,
#                                   the same glyphs and the same weight axis
#   assets/fonts/noto-rupee.ttf     the rupee sign from Noto Sans Regular, which Geist lacks
#
# An image renderer reads TrueType, not WOFF2, so the page's font is unpacked once and kept
# beside it. Run from the repository's root with fontTools and brotli installed:
#
#   python crates/navree-ui/runtime/image-fonts.py
import os

from fontTools import subset
from fontTools.ttLib import TTFont

ROOT = os.path.join(os.path.dirname(__file__), "..", "..", "..")
FONTS = os.path.join(ROOT, "crates", "navree-ui", "assets", "fonts")

geist = TTFont(os.path.join(FONTS, "geist.woff2"), recalcTimestamp=False)
geist.flavor = None
geist.save(os.path.join(FONTS, "geist.ttf"), reorderTables=True)

options = subset.Options()
options.hinting = False
options.layout_features = []
options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]
options.notdef_outline = True
noto = TTFont(os.path.join(ROOT, "crates", "navree-documents", "fonts", "NotoSans-Regular.ttf"), recalcTimestamp=False)
subsetter = subset.Subsetter(options)
subsetter.populate(unicodes=[0x20B9])
subsetter.subset(noto)
noto.save(os.path.join(FONTS, "noto-rupee.ttf"), reorderTables=True)
