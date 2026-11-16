"""Tests of `read_run.py`, with output written as the test runner prints it.

    python3 -I packages/android/scripts/read_run_test.py
"""

import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "read_run.py")


def block(cls, test, code):
    start = f"INSTRUMENTATION_STATUS: class={cls}\nINSTRUMENTATION_STATUS: current=1\nINSTRUMENTATION_STATUS: test={test}\nINSTRUMENTATION_STATUS_CODE: 1\n"
    end = f"INSTRUMENTATION_STATUS: class={cls}\nINSTRUMENTATION_STATUS: current=1\nINSTRUMENTATION_STATUS: test={test}\n"
    if code in (-2, -4):
        end += "INSTRUMENTATION_STATUS: stack=java.lang.AssertionError: it\n\tat com.navree.client.X.y(X.kt:1)\n"
    return start + end + f"INSTRUMENTATION_STATUS_CODE: {code}\n"


def run(tests, finish="OK ({n} tests)"):
    body = "".join(block("com.navree.client." + c, t, code) for c, t, code in tests)
    tail = "INSTRUMENTATION_RESULT: stream=\n\nTime: 12.3\n\n" + (finish.format(n=len(tests)) if finish else "") + "\n\nINSTRUMENTATION_CODE: -1\n"
    return body + tail


class ReadRunTest(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.skips = os.path.join(self.folder.name, "skips.txt")
        with open(self.skips, "w", encoding="utf8") as file:
            file.write(
                "# comment\n"
                "all FolderScreensTest#a no folder\n"
                "dark,large LiveDemosTest#walk walked in the light only\n"
                "all ?ScreensTest#form may skip\n"
            )

    def read(self, text, *words):
        path = os.path.join(self.folder.name, "out.txt")
        with open(path, "w", encoding="utf8") as file:
            file.write(text)
        done = subprocess.run([sys.executable, "-I", SCRIPT, path, *words], capture_output=True, text=True)
        return done.returncode, done.stdout

    def test_a_run_that_is_as_listed_passes(self):
        tests = [("FolderScreensTest", "a", -4), ("LiveDemosTest", "walk", 0), ("ScreensTest", "form", 0), ("HealthScreensTest", "h", 0)]
        code, said = self.read(run(tests), "--look", "light", "--count", "4", "--skips", self.skips)
        self.assertEqual(0, code, said)
        self.assertIn("4 tests ended in the light look; 1 skipped: FolderScreensTest#a", said)

    def test_a_skip_that_is_not_listed_for_the_look_fails_though_the_count_is_right(self):
        tests = [("FolderScreensTest", "a", -4), ("LiveDemosTest", "walk", -4), ("HealthScreensTest", "h", 0)]
        code, said = self.read(run(tests), "--look", "light", "--count", "3", "--skips", self.skips)
        self.assertEqual(1, code, said)
        self.assertIn("skipped and not listed for the light look: LiveDemosTest#walk", said)

    def test_the_same_skip_is_listed_for_another_look(self):
        tests = [("FolderScreensTest", "a", -4), ("LiveDemosTest", "walk", -4)]
        code, said = self.read(run(tests), "--look", "dark", "--count", "2", "--skips", self.skips)
        self.assertEqual(0, code, said)

    def test_a_listed_skip_that_ran_fails_so_the_list_is_kept_true(self):
        tests = [("FolderScreensTest", "a", 0), ("LiveDemosTest", "walk", 0)]
        code, said = self.read(run(tests), "--look", "dark", "--skips", self.skips)
        self.assertEqual(1, code, said)
        self.assertIn("did not skip: FolderScreensTest#a, LiveDemosTest#walk", said)

    def test_a_skip_marked_with_a_question_mark_may_run_or_skip(self):
        for status in (0, -4):
            tests = [("FolderScreensTest", "a", -4), ("ScreensTest", "form", status)]
            code, said = self.read(run(tests), "--look", "light", "--skips", self.skips)
            self.assertEqual(0, code, said)

    def test_a_failed_test_fails_the_run(self):
        tests = [("FolderScreensTest", "a", -4), ("HealthScreensTest", "h", -2)]
        code, said = self.read(run(tests, finish="FAILURES!!!\nTests run: 2,  Failures: 1"), "--look", "light", "--skips", self.skips)
        self.assertEqual(1, code, said)
        self.assertIn("tests failed: HealthScreensTest#h", said)

    def test_a_count_that_is_not_the_expected_one_fails(self):
        tests = [("HealthScreensTest", "h", 0)]
        code, said = self.read(run(tests), "--look", "light", "--count", "2")
        self.assertEqual(1, code, said)
        self.assertIn("1 tests ended; this device is expected to end 2", said)

    def test_a_run_that_did_not_finish_fails(self):
        tests = [("HealthScreensTest", "h", 0)]
        code, said = self.read(run(tests, finish=""), "--look", "light")
        self.assertEqual(1, code, said)
        self.assertIn("did not finish", said)

    def test_an_ignored_test_is_not_counted(self):
        tests = [("HealthScreensTest", "h", 0), ("HealthScreensTest", "i", -3)]
        code, said = self.read(run(tests), "--look", "light", "--count", "1")
        self.assertEqual(0, code, said)


if __name__ == "__main__":
    unittest.main()
