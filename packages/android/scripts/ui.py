"""Reads a `uiautomator dump` of the device: what is on the screen, and where.

    ui.py <dump.xml> parsed                exit 0 when the file is a dump with at least one node
    ui.py <dump.xml> has <text>            exit 0 when a node says <text> (exactly, or as a whole of its description)
    ui.py <dump.xml> contains <text>       exit 0 when a node's text or description holds <text>
    ui.py <dump.xml> absent <text>         exit 0 when the dump parsed and no node says <text> (as `has` reads it)
    ui.py <dump.xml> absent-part <text>    exit 0 when the dump parsed and no node's text or description holds <text>
    ui.py <dump.xml> tap <text>            prints "x y", the centre of the first node that says <text>
    ui.py <dump.xml> count <pattern>       prints how many nodes say something that matches the regular expression
    ui.py <dump.xml> texts                 prints every text and description, one a line, in the order of the dump
    ui.py <dump.xml> field                 prints "x y" of the first editable node

Exits 1 when it finds nothing, and 2 when there is no dump to read: the file is missing, does not
parse or holds no node. A question about what is not on the screen is never answered from a dump
that is not there: `absent` exits 2 then, and so does every other command.
"""

import re
import sys
import xml.etree.ElementTree as ET


class NoDump(Exception):
    """The file is not a dump of a screen."""


def nodes(path):
    try:
        root = ET.parse(path).getroot()
    except (ET.ParseError, OSError) as why:
        raise NoDump(str(why))
    found = list(root.iter("node"))
    if not found:
        raise NoDump("no node in " + path)
    return found


def words(node):
    return [w for w in (node.get("text", ""), node.get("content-desc", "")) if w]


def centre(node):
    left, top, right, bottom = map(int, re.findall(r"-?\d+", node.get("bounds", "[0,0][0,0]")))
    return (left + right) // 2, (top + bottom) // 2


def says(node, argument):
    return any(argument == w or argument in w.split(", ") for w in words(node))


def holds(node, argument):
    return any(argument in w for w in words(node))


def run(path, command, argument):
    found = False
    everything = nodes(path)
    if command in ("has", "contains", "absent", "absent-part", "tap") and not argument:
        # Nothing to look for is not "anything": an empty text would match the first node of every dump.
        print("no text to look for", file=sys.stderr)
        return 1
    if command == "parsed":
        found = True
    elif command == "has":
        found = any(says(node, argument) for node in everything)
    elif command == "contains":
        found = any(holds(node, argument) for node in everything)
    elif command == "absent":
        found = not any(says(node, argument) for node in everything)
    elif command == "absent-part":
        found = not any(holds(node, argument) for node in everything)
    elif command == "tap":
        for node in everything:
            if says(node, argument):
                print(*centre(node))
                found = True
                break
        if not found:
            for node in everything:
                if holds(node, argument):
                    print(*centre(node))
                    found = True
                    break
    elif command == "count":
        pattern = re.compile(argument)
        print(sum(1 for node in everything if any(pattern.search(w) for w in words(node))))
        found = True
    elif command == "texts":
        for node in everything:
            for w in words(node):
                print(w)
        found = True
    elif command == "field":
        for node in everything:
            if node.get("class", "").endswith("EditText"):
                print(*centre(node))
                found = True
                break
    return 0 if found else 1


def main(argv):
    path, command = argv[1], argv[2]
    argument = argv[3] if len(argv) > 3 else ""
    try:
        return run(path, command, argument)
    except NoDump as why:
        print("no dump of the screen: " + str(why), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
