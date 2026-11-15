"""Tests of `pictures_check.py`: a folder of black frames fails, a folder of real pictures does not.

    python3 -I packages/android/scripts/pictures_check_test.py
"""

import os
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "pictures_check.py")


def chunk(kind, body):
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)


def png(width, height, pixel):
    """A PNG of `height` rows, whose pixel at (x, y) is `pixel(x, y)` as (r, g, b)."""
    raw = b"".join(b"\x00" + b"".join(bytes(pixel(x, y)) for x in range(width)) for y in range(height))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def write(folder, name, data):
    with open(os.path.join(folder, name), "wb") as handle:
        handle.write(data)


def run(folder, **env):
    return subprocess.run([sys.executable, "-I", SCRIPT, folder], capture_output=True, text=True, env={**os.environ, **env})


class PicturesCheckTest(unittest.TestCase):
    def test_pictures_that_differ_pass(self):
        with tempfile.TemporaryDirectory() as folder:
            for n in range(6):
                write(folder, f"tablet-light-screen-{n}.png", png(24 + n, 16, lambda x, y, n=n: (x * 9 % 256, y * 15 % 256, n * 40)))
            done = run(folder)
            self.assertEqual(done.returncode, 0, done.stdout + done.stderr)

    def test_black_frames_of_one_size_fail_and_say_the_host_image(self):
        with tempfile.TemporaryDirectory() as folder:
            for n in range(6):
                write(folder, f"tablet-light-screen-{n}.png", png(24, 16, lambda x, y: (0, 0, 0)))
            done = run(folder, ImageOS="ubuntu24", ImageVersion="20261101.1")
            self.assertEqual(done.returncode, 1, done.stdout)
            self.assertIn("::error title=Pictures::", done.stdout)
            self.assertIn("ubuntu24 20261101.1", done.stdout)

    def test_one_black_frame_among_pictures_of_different_sizes_fails(self):
        with tempfile.TemporaryDirectory() as folder:
            for n in range(3):
                write(folder, f"phone-dark-screen-{n}.png", png(30 + n, 20, lambda x, y, n=n: (x * 7 % 256, y * 11 % 256, n * 60)))
            write(folder, "phone-dark-screen-9.png", png(40, 20, lambda x, y: (0, 0, 0)))
            done = run(folder)
            self.assertEqual(done.returncode, 1, done.stdout)
            self.assertIn("a single colour", done.stdout)

    def test_a_folder_with_no_pictures_fails(self):
        with tempfile.TemporaryDirectory() as folder:
            done = run(folder)
            self.assertEqual(done.returncode, 1)
            self.assertIn("no pictures were found", done.stdout)


if __name__ == "__main__":
    unittest.main()
