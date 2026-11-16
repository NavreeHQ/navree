"""Runs the script that is put at the start of every document of the web view, as the Kotlin source builds it, in node
against stand-ins for the page's `WebSocket` and `Worker`, and tries what a page's own script could try after it: an
address that converts differently the second time, and every global and prototype method the page can replace.

    python3 -I packages/android/scripts/document_script_test.py

The script is taken out of `WebClient.kt` (`WebRules.documentScript`), so a change of it is run, not only read.
Needs `node`.
"""

import json
import os
import re
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.join(HERE, "..", "navree-native", "src", "main", "kotlin", "com", "navree", "client", "platform", "WebClient.kt")
HARNESS = os.path.join(HERE, "document_script_harness.js")
ORIGINS = '[["http","localhost",8080],["https","shop.example",443],["https","*.cdn.example",443]]'

# What a page can change after the script ran: each is one run of the harness, in a process of its own.
CHANGES = [
    "none",
    "url",
    "string",
    "string methods",
    "url getters",
    "base",
    "call and apply",
    "reflect",
    "arrays",
    "domexception",
    "regexp",
]


def script(source_path):
    with open(source_path, encoding="utf8") as file:
        text = file.read()
    start = text.index('return "(function(){"')
    end_text = 'catch(e){stop()}})();"'
    end = text.index(end_text) + len(end_text)
    expression = "\n".join(line for line in text[start:end].splitlines() if not line.strip().startswith("//"))
    parts = re.findall(r'"((?:[^"\\]|\\.)*)"', expression)
    return "".join(parts).replace("$rules", ORIGINS).replace('\\"', '"').replace("\\\\", "\\")


def run(js, change):
    with tempfile.TemporaryDirectory() as folder:
        path = os.path.join(folder, "script.js")
        with open(path, "w", encoding="utf8") as file:
            file.write(js)
        done = subprocess.run(["node", HARNESS, path, change], capture_output=True, text=True)
    if done.returncode != 0:
        raise AssertionError(change + ": " + done.stderr)
    return json.loads(done.stdout)


class DocumentScriptTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        js = script(SOURCE)
        cls.runs = {change: run(js, change) for change in CHANGES}
        cls.seen = cls.runs["none"]

    def test_a_socket_and_a_worker_go_only_to_the_apps_origins(self):
        results = self.seen["results"]
        self.assertEqual("made", results["own socket"])
        self.assertEqual("made", results["relative socket"])
        self.assertEqual("refused SecurityError", results["other socket"])
        self.assertEqual("refused SecurityError", results["other socket on the app's port"])
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
        self.assertEqual("made", self.seen["results"]["toPrimitive socket"])
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

    def test_what_a_page_replaces_after_the_script_ran_never_lets_a_socket_or_a_worker_go_elsewhere(self):
        for change, seen in self.runs.items():
            with self.subTest(change=change):
                results = seen["results"]
                reached = [a for a in seen["sockets"] + seen["workers"] if "other.example" in a or a.startswith(("blob:", "data:"))]
                self.assertEqual([], reached, f"what the real constructors were given after the page changed {change}")
                for name in ("other socket", "other socket on the app's port", "blob worker", "data worker", "constructor"):
                    self.assertTrue(results[name].startswith("refused"), f"{name} after the page changed {change}: {results[name]}")
                self.assertEqual("made", results["host under a pattern"], change)
                self.assertTrue(results["the pattern's own name"].startswith("refused"), change)
                self.assertTrue(results["a name that ends the same"].startswith("refused"), change)
                if change == "base":
                    # The document's real base is another host's: a relative address goes there, and is refused.
                    self.assertTrue(results["relative socket"].startswith("refused"), results["relative socket"])
                else:
                    self.assertEqual("made", results["relative socket"], change)

    def test_the_apps_own_socket_and_worker_still_open_whatever_the_page_replaced(self):
        # The check keeps working: it is not a refusal of everything because something it needed was replaced.
        for change, seen in self.runs.items():
            with self.subTest(change=change):
                self.assertEqual("made", seen["results"]["own socket"], change)
                self.assertIn("ws://localhost:8080/own", seen["sockets"], change)
                # The worker's address is relative: where the document's real base is another host's, it goes there.
                self.assertEqual("refused SecurityError" if change == "base" else "made", seen["results"]["own worker"], change)


if __name__ == "__main__":
    unittest.main()
