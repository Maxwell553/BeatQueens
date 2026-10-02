#!/usr/bin/env python3
"""Turn the samimsu/queens-game community boards into the bench corpus."""

import os
import re
import sys

ROOT = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "vendor",
    "queens-game",
    "src",
    "utils",
    "community-levels",
)
OUT = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "corpus",
    "community.txt",
)


def load(path):
    text = open(path, encoding="utf-8").read()
    if "colorRegions" not in text:
        return None
    size = re.search(r"size:\s*(\d+)", text)
    if not size:
        return None
    n = int(size.group(1))
    block = text.split("colorRegions:", 1)[1].split("regionColors", 1)[0]
    cells = re.findall(r'"([^"]+)"', block)
    if len(cells) != n * n:
        return None
    sols = re.search(r"solutionsCount:\s*(\d+)", text)
    expected = int(sols.group(1)) if sols else -1
    rows = ["".join(cells[i * n : (i + 1) * n]) for i in range(n)]
    return n, expected, rows


def main():
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    files = []
    for name in os.listdir(ROOT):
        m = re.fullmatch(r"level(\d+)\.ts", name)
        if m:
            files.append((int(m.group(1)), name))
    files.sort()
    written = 0
    with open(OUT, "w", encoding="utf-8") as out:
        out.write("# id expected\n")
        for num, name in files:
            parsed = load(os.path.join(ROOT, name))
            if not parsed:
                print("skip", name, file=sys.stderr)
                continue
            _n, expected, rows = parsed
            out.write(f"community-{num} {expected}\n")
            for row in rows:
                out.write(row + "\n")
            out.write("\n")
            written += 1
    print(f"wrote {written} boards to {OUT}")


if __name__ == "__main__":
    main()
