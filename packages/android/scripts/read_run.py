"""Reads the raw output of a run of the device tests (`am instrument -r -w`) and says what happened to each test.

    read_run.py <output> --look <look> [--count N] [--skips <file>]

A test ends with a status code: 0 passed, -1 or -2 failed, -3 was ignored, -4 stopped on an assumption
(a skip). The count a run prints (`OK (N tests)`) counts a skip as run, so it cannot see one; this reads
the statuses themselves. It fails when:

  - the run did not finish (no `OK (` and no `FAILURES!!!`), or a test failed;
  - `--count` is given and the number of tests that ended (passed, failed or skipped) is another;
  - a test skipped that the skips file does not list for this look, or a test the file says skips (a line
    without `?`) did not skip, so a skip that went away is noticed too.

The skips file has one line for each test or group of looks: `<looks> <class>#<test> <reason>`, where
looks is `all` or a comma list (`dark,large`), and a `?` in front of the test says it may skip or run.
A line that starts with `#` is a comment.
"""

import argparse
import re
import sys

OK, ERROR, FAILURE, IGNORED, SKIPPED, START = 0, -1, -2, -3, -4, 1


def results(text):
    """The (class, test, code) of every test that ended, in the order they ended."""
    found = []
    current = {}
    key = None
    for line in text.splitlines():
        line = line.rstrip("\r")
        status = re.match(r"INSTRUMENTATION_STATUS: ([^=]+)=(.*)$", line)
        code = re.match(r"INSTRUMENTATION_STATUS_CODE: (-?\d+)$", line)
        if status:
            key = status.group(1)
            current[key] = status.group(2)
        elif code:
            value = int(code.group(1))
            if value != START and "class" in current and "test" in current:
                found.append((current["class"], current["test"], value))
            current = {}
            key = None
        elif line.startswith("INSTRUMENTATION_"):
            key = None
    return found


def allowed(path, look):
    """(must skip, may skip): the tests the file lists for this look."""
    must, may = set(), set()
    with open(path, encoding="utf8") as file:
        for line in file:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            looks, name = line.split()[:2]
            if looks != "all" and look not in looks.split(","):
                continue
            if name.startswith("?"):
                may.add(name[1:])
            else:
                must.add(name)
    return must, may


def check(text, look, count=None, skips=None):
    """The problems of a run, as plain sentences; empty when it is as expected."""
    problems = []
    ended = results(text)
    finished = re.search(r"^OK \(\d+ tests?\)", text, re.M) or "FAILURES!!!" in text
    if not finished:
        problems.append("the run did not finish: it printed neither OK nor FAILURES")
    failed = [f"{c.split('.')[-1]}#{t}" for c, t, code in ended if code in (ERROR, FAILURE)]
    if failed:
        problems.append("tests failed: " + ", ".join(failed))
    ran = [(c, t, code) for c, t, code in ended if code != IGNORED]
    if count is not None and len(ran) != count:
        problems.append(f"{len(ran)} tests ended; this device is expected to end {count}")
    skipped = {f"{c.split('.')[-1]}#{t}" for c, t, code in ran if code == SKIPPED}
    if skips is not None:
        must, may = allowed(skips, look)
        unexpected = sorted(skipped - must - may)
        if unexpected:
            problems.append(f"skipped and not listed for the {look} look: " + ", ".join(unexpected))
        gone = sorted(must - skipped)
        if gone:
            problems.append(f"listed as skipped for the {look} look and did not skip: " + ", ".join(gone))
    return problems, len(ran), sorted(skipped)


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("output")
    parser.add_argument("--look", required=True)
    parser.add_argument("--count", type=int)
    parser.add_argument("--skips")
    args = parser.parse_args(argv[1:])
    with open(args.output, encoding="utf8", errors="replace") as file:
        text = file.read()
    problems, ran, skipped = check(text, args.look, args.count, args.skips)
    print(f"{ran} tests ended in the {args.look} look; {len(skipped)} skipped: {', '.join(skipped) or 'none'}")
    for problem in problems:
        print(f"::error title=Device tests ({args.look})::{problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
