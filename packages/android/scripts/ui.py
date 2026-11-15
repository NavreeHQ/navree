"""Reads a `uiautomator dump` of the device: what is on the screen, and where.

    ui.py <dump.xml> has <text>            exit 0 when a node says <text> (exactly, or as a whole of its description)
    ui.py <dump.xml> contains <text>       exit 0 when a node's text or description holds <text>
    ui.py <dump.xml> tap <text>            prints "x y", the centre of the first node that says <text>
    ui.py <dump.xml> count <pattern>       prints how many nodes say something that matches the regular expression
    ui.py <dump.xml> texts                 prints every text and description, one a line, in the order of the dump
    ui.py <dump.xml> field                 prints "x y" of the first editable node

Exits 1 when it finds nothing.
"""

import re
import sys
import xml.etree.ElementTree as ET


def nodes(path):
    try:
        root = ET.parse(path).getroot()
    except ET.ParseError:
        return
    for node in root.iter("node"):
        yield node


def words(node):
    return [w for w in (node.get("text", ""), node.get("content-desc", "")) if w]


def centre(node):
    left, top, right, bottom = map(int, re.findall(r"-?\d+", node.get("bounds", "[0,0][0,0]")))
    return (left + right) // 2, (top + bottom) // 2


def main(argv):
    path, command = argv[1], argv[2]
    argument = argv[3] if len(argv) > 3 else ""
    found = False
    if command == "has":
        found = any(argument == w or argument in w.split(", ") for node in nodes(path) for w in words(node))
    elif command == "contains":
        found = any(argument in w for node in nodes(path) for w in words(node))
    elif command == "tap":
        for node in nodes(path):
            if any(argument == w or argument in w.split(", ") for w in words(node)):
                print(*centre(node))
                found = True
                break
        if not found:
            for node in nodes(path):
                if any(argument in w for w in words(node)):
                    print(*centre(node))
                    found = True
                    break
    elif command == "count":
        pattern = re.compile(argument)
        count = sum(1 for node in nodes(path) if any(pattern.search(w) for w in words(node)))
        print(count)
        found = True
    elif command == "texts":
        for node in nodes(path):
            for w in words(node):
                print(w)
        found = True
    elif command == "field":
        for node in nodes(path):
            if node.get("class", "").endswith("EditText"):
                print(*centre(node))
                found = True
                break
    return 0 if found else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
