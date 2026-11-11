#!/usr/bin/env python3
"""Structural check for bug lessons under docs/lessons; never judges teaching quality.

It verifies that each lesson has the agreed sections in order, a runnable-looking
reproduction script and saved real output. Whether the reproduction still shows the
bug, and whether a beginner can follow the text, need a person to try it.
"""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import sys

SECTIONS = [
    "一句話說後果",
    "需要的背景",
    "親手重現",
    "為什麼會這樣",
    "為什麼嚴重",
    "怎麼修",
    "怎麼證明修好了",
    "還沒解決的事",
    "自我檢查與練習",
    "面試時怎麼講",
]
DIRECTORY = re.compile(r"^(\d\d)-[a-z0-9]+(?:-[a-z0-9]+)*$")
STATUS = re.compile(r"^狀態：(草稿|完成)\s*$", re.MULTILINE)
PLACEHOLDERS = ("待補", "TODO", "TBD")


def check_lesson(path: Path) -> list[str]:
    problems: list[str] = []
    readme = path / "README.md"
    if not readme.is_file():
        return [f"{path.name}: README.md missing"]
    text = readme.read_text(encoding="utf-8")
    if not text.startswith("# "):
        problems.append(f"{path.name}: README.md must start with a level-1 title")
    status = STATUS.search(text)
    if not status:
        problems.append(f"{path.name}: needs a line '狀態：草稿' or '狀態：完成'")
    headings = re.findall(r"^## (\d+)\. (.+?)\s*$", text, re.MULTILINE)
    expected = [(str(number), title) for number, title in enumerate(SECTIONS, 1)]
    actual = [(number, title) for number, title in headings]
    if actual != expected:
        problems.append(f"{path.name}: sections must be exactly, in order: "
                        + "; ".join(f"## {n}. {t}" for n, t in expected))
    else:
        bodies = re.split(r"^## \d+\. .+$", text, flags=re.MULTILINE)[1:]
        for (number, title), body in zip(expected, bodies):
            if len(body.strip()) < 40:
                problems.append(f"{path.name}: section {number} ({title}) is nearly empty")
    if status and status.group(1) == "完成":
        for word in PLACEHOLDERS:
            if word in text:
                problems.append(f"{path.name}: marked 完成 but still contains '{word}'")
    script = path / "repro.sh"
    if not script.is_file():
        problems.append(f"{path.name}: repro.sh missing")
    else:
        if not script.read_text(encoding="utf-8").startswith("#!"):
            problems.append(f"{path.name}: repro.sh needs a shebang line")
        if not os.access(script, os.X_OK):
            problems.append(f"{path.name}: repro.sh is not executable")
        if "repro.sh" not in text:
            problems.append(f"{path.name}: README.md never tells the reader to run repro.sh")
    output = path / "expected-output.txt"
    if not output.is_file() or not output.read_text(encoding="utf-8").strip():
        problems.append(f"{path.name}: expected-output.txt (real captured output) missing or empty")
    return problems


def check(root: Path) -> tuple[int, list[str]]:
    lessons = root / "docs" / "lessons"
    problems: list[str] = []
    for name in ("README.md", "TEMPLATE.md"):
        if not (lessons / name).is_file():
            problems.append(f"docs/lessons/{name} missing")
    if problems:
        return 0, problems
    index = (lessons / "README.md").read_text(encoding="utf-8")
    directories = sorted(p for p in lessons.iterdir() if p.is_dir())
    numbers = []
    for directory in directories:
        match = DIRECTORY.match(directory.name)
        if not match:
            problems.append(f"{directory.name}: lesson folders are named NN-short-name in lower case")
            continue
        numbers.append(int(match.group(1)))
        problems.extend(check_lesson(directory))
        if f"({directory.name}/" not in index:
            problems.append(f"{directory.name}: not linked from docs/lessons/README.md")
    if numbers != list(range(1, len(numbers) + 1)):
        problems.append(f"lesson numbers must run 01, 02, ... without gaps; found {numbers}")
    return len(directories), problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    count, problems = check(args.root)
    for problem in problems:
        print(f"FAIL {problem}")
    print(f"lessons checked: {count}; problems: {len(problems)}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
