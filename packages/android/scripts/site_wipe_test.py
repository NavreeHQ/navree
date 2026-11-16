"""Runs the script the client puts at the start of its own document when a session has ended, as the Kotlin source
builds it, in node against a stand-in for a document that holds service worker registrations and caches: it stops and
empties the document, unregisters every registration, deletes every cache, and says what it did through its listener.

    python3 -I packages/android/scripts/site_wipe_test.py

The script is taken out of `SiteWipe.kt` (`WipeRules.script`), so a change of it is run, not only read. Needs `node`.
"""

import json
import os
import re
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.join(HERE, "..", "navree-native", "src", "main", "kotlin", "com", "navree", "client", "platform", "SiteWipe.kt")
HARNESS = os.path.join(HERE, "site_wipe_harness.js")
WORD = "0f3a9c5e7b1d2468ace013579bdf2468"


def script(source_path):
    with open(source_path, encoding="utf8") as file:
        text = file.read()
    start = text.index('return "(function(){"')
    end_text = '"p.then(say,fail)}catch(e){fail(e)}})();"'
    end = text.index(end_text) + len(end_text)
    parts = re.findall(r'"((?:[^"\\]|\\.)*)"', text[start:end])
    return "".join(parts).replace("$LISTENER", "navreeWipe").replace("$word", WORD)


def run(js, case):
    with tempfile.TemporaryDirectory() as folder:
        path = os.path.join(folder, "script.js")
        with open(path, "w", encoding="utf8") as file:
            file.write(js)
        done = subprocess.run(["node", HARNESS, path, case], capture_output=True, text=True)
    if done.returncode != 0:
        raise AssertionError(case + ": " + done.stderr)
    return json.loads(done.stdout)


class SiteWipeScriptTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.js = script(SOURCE)

    def said(self, case):
        seen = run(self.js, case)
        return seen, [json.loads(message) for message in seen["messages"]]

    def test_the_script_is_the_one_the_source_builds(self):
        self.assertTrue(self.js.startswith("(function(){") and self.js.endswith("})();"), self.js)
        self.assertNotIn("$", self.js, "every part the source fills in is filled in")
        self.assertIn("navreeWipe", self.js)
        self.assertIn(WORD, self.js)

    def test_every_worker_is_unregistered_and_every_cache_deleted_and_the_document_says_so(self):
        seen, messages = self.said("a worker and two caches")
        self.assertEqual(1, seen["unregistered"])
        self.assertEqual(["pages", "orders"], seen["deleted"])
        self.assertEqual([], seen["left"])
        self.assertEqual([{"n": WORD, "workers": 1, "caches": 2, "left": 0}], messages)

    def test_the_document_is_stopped_and_emptied_before_anything_else(self):
        # Whatever answered the address, the client or the origin's own worker, nothing of it is drawn or run.
        for case in ("a worker and two caches", "nothing stored", "no listener", "a frame"):
            seen, _ = self.said(case)
            self.assertTrue(seen["stopped"], case)
            self.assertTrue(seen["observed"], case)
            self.assertTrue(seen["emptied"], case)

    def test_an_origin_that_holds_nothing_says_so(self):
        _, messages = self.said("nothing stored")
        self.assertEqual([{"n": WORD, "workers": 0, "caches": 0, "left": 0}], messages)
        _, messages = self.said("no worker and no cache interface")
        self.assertEqual([{"n": WORD, "workers": 0, "caches": 0, "left": 0}], messages)

    def test_a_document_that_cannot_ask_for_its_registrations_says_it_failed_and_deletes_nothing_behind_that(self):
        seen, messages = self.said("the registrations cannot be asked for")
        self.assertEqual(1, len(messages))
        self.assertIn("error", messages[0])
        self.assertIn("invalid state", messages[0]["error"])
        self.assertEqual(0, seen["unregistered"])

    def test_a_cache_that_stays_is_counted_as_left(self):
        _, messages = self.said("a cache that stays")
        self.assertEqual([{"n": WORD, "workers": 0, "caches": 1, "left": 1}], messages)

    def test_a_frame_and_a_document_without_the_listener_say_nothing_and_touch_nothing(self):
        for case in ("a frame", "no listener"):
            seen, messages = self.said(case)
            self.assertEqual([], messages, case)
            self.assertEqual(0, seen["unregistered"], case)
            self.assertEqual([], seen["deleted"], case)


if __name__ == "__main__":
    unittest.main()
