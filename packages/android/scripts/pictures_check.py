"""Checks that the pictures a run of the device tests pulled are pictures.

    python3 -I packages/android/scripts/pictures_check.py <folder of the pictures>

The pictures are named `<device>-<look>-<name>.png`. For each device and look
(a group of them) the check fails when the files are all one size (a frame that
is one colour compresses to the same bytes whatever the screen was) or when a
sample of them is a single colour. It reads PNG files with the standard library
only. It prints what it
found, and the name of the host image the job ran on, and exits 1 when a group
is not made of pictures, with an error annotation for the run.
"""

import collections
import os
import re
import struct
import sys
import zlib

GROUP = re.compile(r"^(?P<group>.+?-(?:light|dark|large|rtl))-.+\.png$")
SAMPLE = 6
LEAST = 4


def raw_bytes(path):
    """The decompressed image data of a PNG: its rows, each with its filter byte."""
    with open(path, "rb") as handle:
        data = handle.read()
    if data[:4] != b"\x89PNG":
        raise ValueError("not a PNG file")
    at = 8
    idat = []
    while at + 8 <= len(data):
        length, kind = struct.unpack(">I4s", data[at:at + 8])
        if kind == b"IDAT":
            idat.append(data[at + 8:at + 8 + length])
        elif kind == b"IEND":
            break
        at += 12 + length
    if not idat:
        raise ValueError("no image data")
    return zlib.decompress(b"".join(idat))


# A picture of one colour has a handful of byte values in it (its filter bytes, its pixel, zeros where a filter took the pixel away);
# a picture of a screen has many. Read at the speed of the decompressor, not pixel by pixel.
FLAT = 8


def one_colour(path):
    """Whether the picture is a single colour."""
    return len(set(raw_bytes(path))) <= FLAT


def check(folder):
    """The faults of a folder of pictures: a list of sentences, empty when they are pictures."""
    groups = collections.defaultdict(list)
    for name in sorted(os.listdir(folder)):
        found = GROUP.match(name)
        if found:
            groups[found.group("group")].append(os.path.join(folder, name))
    faults = []
    for group, files in sorted(groups.items()):
        sizes = {os.path.getsize(path) for path in files}
        if len(files) >= LEAST and len(sizes) == 1:
            faults.append(f"{group}: {len(files)} pictures, all {sizes.pop()} bytes")
            continue
        step = max(1, len(files) // SAMPLE)
        for path in files[::step][:SAMPLE]:
            try:
                flat = one_colour(path)
            except Exception as why:  # a file that is not read is not a picture this check can vouch for
                faults.append(f"{os.path.basename(path)}: not read ({why})")
                break
            if flat:
                faults.append(f"{os.path.basename(path)}: a single colour")
                break
    return groups, faults


def main(argv):
    if len(argv) != 2:
        print("usage: pictures_check.py <folder>", file=sys.stderr)
        return 2
    groups, faults = check(argv[1])
    host = f"{os.environ.get('ImageOS', '?')} {os.environ.get('ImageVersion', '?')} ({os.environ.get('RUNNER_NAME', '?')})"
    pictures = sum(len(files) for files in groups.values())
    print(f"pictures: {pictures} in {len(groups)} looks; the host image is {host}")
    for fault in faults:
        print(f"::error title=Pictures::{fault}; the host image is {host}")
    if not groups:
        print(f"::error title=Pictures::no pictures were found; the host image is {host}")
        return 1
    return 1 if faults else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
