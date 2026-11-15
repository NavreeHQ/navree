"""Tests of `ui.py`: what it says about a dump of the device's screen, and that it says nothing of a dump that is not there.

    python3 -I packages/android/scripts/ui_test.py
"""

import os
import subprocess
import sys
import tempfile
import unittest

UI = os.path.join(os.path.dirname(os.path.abspath(__file__)), "ui.py")

SCREEN = """<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>
<hierarchy rotation="0">
  <node class="android.widget.FrameLayout" text="" content-desc="" bounds="[0,0][1080,2400]">
    <node class="android.widget.TextView" text="Dashboard" content-desc="" bounds="[0,100][1080,200]" />
    <node class="android.view.View" text="" content-desc="Account, Navjot Kaur" bounds="[0,300][1080,400]" />
    <node class="android.widget.EditText" text="Search" content-desc="" bounds="[100,500][900,600]" />
  </node>
</hierarchy>
"""


class UiTest(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)

    def dump(self, text):
        path = os.path.join(self.folder.name, "window.xml")
        with open(path, "w", encoding="utf8") as file:
            file.write(text)
        return path

    def ask(self, path, *words):
        done = subprocess.run([sys.executable, "-I", UI, path, *words], capture_output=True, text=True)
        return done.returncode, done.stdout.strip()

    def test_a_node_is_found_by_what_it_says(self):
        path = self.dump(SCREEN)
        self.assertEqual(0, self.ask(path, "has", "Dashboard")[0])
        self.assertEqual(0, self.ask(path, "has", "Account")[0], "a part of a description between commas")
        self.assertEqual(1, self.ask(path, "has", "Dash")[0], "has is the whole text")
        self.assertEqual(0, self.ask(path, "contains", "Dash")[0])
        self.assertEqual((0, "540 150"), self.ask(path, "tap", "Dashboard"))
        self.assertEqual((0, "500 550"), self.ask(path, "field"))
        self.assertEqual((0, "1"), self.ask(path, "count", "Dash"))

    def test_what_is_absent_is_known_only_from_a_dump_that_parsed(self):
        path = self.dump(SCREEN)
        self.assertEqual(0, self.ask(path, "absent", "Back")[0])
        self.assertEqual(1, self.ask(path, "absent", "Dashboard")[0])
        self.assertEqual(0, self.ask(path, "absent-part", "Fulfilment")[0])
        self.assertEqual(1, self.ask(path, "absent-part", "Dash")[0])

    def test_there_is_no_answer_from_a_dump_that_is_not_there(self):
        missing = os.path.join(self.folder.name, "none.xml")
        broken = self.dump("<hierarchy rotation=")
        empty = self.dump('<?xml version="1.0"?><hierarchy rotation="0"></hierarchy>')
        for path in (missing, broken, empty):
            for command in ("absent", "absent-part", "has", "contains", "tap", "parsed"):
                self.assertEqual(2, self.ask(path, command, "Back")[0], f"{command} of {os.path.basename(path)}")
            self.assertEqual(2, self.ask(path, "texts")[0])
            self.assertEqual(2, self.ask(path, "count", "x")[0])

    def test_a_dump_that_parsed_is_said_to_have(self):
        self.assertEqual(0, self.ask(self.dump(SCREEN), "parsed")[0])

    def test_nothing_to_look_for_is_not_anything(self):
        path = self.dump(SCREEN)
        for command in ("has", "contains", "absent", "absent-part", "tap"):
            self.assertEqual(1, self.ask(path, command, "")[0], command)


if __name__ == "__main__":
    unittest.main()
