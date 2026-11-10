"""Writes the sources of the real documents `make.sh` converts with LibreOffice:
a sheet, a book, a document of images and a deck. Usage: make.py DIR [ROWS]."""
import os
import random
import struct
import sys
import zlib

out = sys.argv[1]
rows = int(sys.argv[2]) if len(sys.argv) > 2 else 6000
random.seed(1)
words = (
    "the quick brown fox jumps over a lazy dog while readers open very large "
    "documents about budgets limits and memory in every kind of office"
).split()


def paragraph(count):
    return " ".join(random.choice(words) for _ in range(count))


def png(width, height):
    """Noise, which does not compress: an image of a photograph's kind."""
    raw = b"".join(
        b"\x00" + bytes(random.getrandbits(8) for _ in range(width * 3))
        for _ in range(height)
    )

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 1))
        + chunk(b"IEND", b"")
    )


# A sheet of twelve columns of numbers: LibreOffice writes it tagged, a
# structure element for each cell.
with open(os.path.join(out, "sheet.csv"), "w") as f:
    for _ in range(rows):
        f.write(",".join(str(random.randint(0, 99999)) for _ in range(12)) + "\n")

# A book: paragraphs of plain text, some three thousand pages.
with open(os.path.join(out, "book.txt"), "w") as f:
    for _ in range(20000):
        f.write(paragraph(120) + "\n\n")

# A document of images, each with a line of text under it.
with open(os.path.join(out, "images.html"), "w") as f:
    f.write("<html><body>\n")
    for i in range(80):
        name = f"image{i}.png"
        with open(os.path.join(out, name), "wb") as image:
            image.write(png(320, 240))
        f.write(
            f'<p><img src="{name}" width="320" height="240"></p>'
            f"<p>Figure {i}. {paragraph(20)}</p>\n"
        )
    f.write("</body></html>\n")

# A deck: a page for each slide with a title and bullets, and a picture on
# every tenth, in the size of a slide.
with open(os.path.join(out, "picture.png"), "wb") as f:
    f.write(png(320, 240))
with open(os.path.join(out, "deck.html"), "w") as f:
    f.write(
        "<html><head><style>@page { size: 28cm 15.75cm; margin: 1cm } "
        "div { page-break-after: always }</style></head><body>\n"
    )
    for i in range(300):
        bullets = "".join(f"<li>{paragraph(8)}</li>" for _ in range(5))
        image = '<img src="picture.png" width="320" height="240">' if i % 10 == 0 else ""
        f.write(f"<div><h1>Slide {i}: {paragraph(4)}</h1><ul>{bullets}</ul>{image}</div>\n")
    f.write("</body></html>\n")
