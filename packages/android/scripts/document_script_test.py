"""Runs the script that is put at the start of every document of the web view, as the Kotlin source builds it, in node
against stand-ins for the page's `WebSocket` and `Worker`, and tries the tricks a page's script could try.

    python3 -I packages/android/scripts/document_script_test.py

The script is taken out of `WebClient.kt` (`WebRules.documentScript`), so a change of it is run, not only read.
Needs `node`.
"""

import json
import os
import re
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.join(HERE, "..", "navree-native", "src", "main", "kotlin", "com", "navree", "client", "platform", "WebClient.kt")
HARNESS = os.path.join(HERE, "document_script_harness.js")
ORIGINS = '[["http","localhost",8080],["https","shop.example",443]]'


def script(source_path):
    text = open(source_path, encoding="utf8").read()
    start = text.index('return "(function(){"')
    end_text = 'catch(e){stop()}})();"'
    end = text.index(end_text) + len(end_text)
    expression = "\n".join(line for line in text[start:end].splitlines() if not line.strip().startswith("//"))
    parts = re.findall(r'"((?:[^"\\]|\\.)*)"', expression)
    return "".join(parts).replace("$rules", ORIGINS).replace('\\"', '"').replace("\\\\", "\\")


def run(js):
    with tempfile.TemporaryDirectory() as folder:
        path = os.path.join(folder, "script.js")
        with open(path, "w", encoding="utf8") as file:
            file.write(js)
        done = subprocess.run(["node", HARNESS, path], capture_output=True, text=True)
    if done.returncode != 0:
        raise AssertionError(done.stderr)
    return json.loads(done.stdout)


class DocumentScriptTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.seen = run(script(SOURCE))

    def test_a_socket_and_a_worker_go_only_to_the_apps_origins(self):
        results = self.seen["results"]
        self.assertEqual("made", results["own socket"])
        self.assertEqual("refused SecurityError", results["other socket"])
        self.assertEqual("made", results["own worker"])
        self.assertEqual("refused SecurityError", results["blob worker"])
        self.assertEqual("refused SecurityError", results["data worker"])
        self.assertEqual("refused SecurityError", results["constructor"], "the instance's constructor is the wrapper")

    def test_an_object_that_answers_differently_the_second_time_it_is_turned_into_a_string_gets_nowhere(self):
        # The first answer is the app's address, the second another host (or a blob): whatever reaches the real
        # constructor must be the first, the one that was checked.
        self.assertNotIn("ws://other.example/x", self.seen["sockets"])
        self.assertNotIn("blob:http://localhost:8080/abc", self.seen["workers"])
        self.assertEqual("made", self.seen["results"]["toString socket"])
        self.assertEqual("made", self.seen["results"]["toString worker"])

    def test_what_the_real_constructor_gets_is_a_string_and_options_are_copied(self):
        self.assertTrue(all(isinstance(s, str) for s in self.seen["sockets"] + self.seen["workers"]))
        self.assertIn(["a", "b"], self.seen["protocols"], "protocols become an array of strings")
        copied = [o for o in self.seen["options"] if isinstance(o, dict)]
        self.assertTrue(copied)
        for o in copied:
            self.assertTrue(o["plain"], "the options are the script's own object, not the page's")
            self.assertEqual(["name", "type", "credentials"], o["keys"], "and only the fields a worker is made with")
        self.assertEqual("n0", self.seen["optionName"], "a field is read once")

    def test_nothing_is_emptied_for_a_document_of_the_app(self):
        self.assertFalse(self.seen["emptied"])


if __name__ == "__main__":
    unittest.main()
