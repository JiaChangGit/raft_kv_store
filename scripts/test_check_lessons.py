"""Negative and positive cases for the lesson structure check."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("check_lessons", Path(__file__).with_name("check_lessons.py"))
lessons = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lessons)

BODY = "這一節有足夠的內容，說明讀者會看到什麼、為什麼，以及下一步要做什麼，不是空白佔位。\n"


def write_lesson(root: Path, name: str = "01-sample-bug", status: str = "完成", sections=None) -> Path:
    base = root / "docs" / "lessons"
    base.mkdir(parents=True, exist_ok=True)
    (base / "TEMPLATE.md").write_text("# 樣板\n", encoding="utf-8")
    (base / "README.md").write_text(f"# 索引\n\n- [範例]({name}/README.md)\n", encoding="utf-8")
    folder = base / name
    folder.mkdir()
    titles = lessons.SECTIONS if sections is None else sections
    text = f"# 範例\n\n狀態：{status}\n\n" + "".join(
        f"## {number}. {title}\n\n{BODY}執行 `./repro.sh`。\n\n" for number, title in enumerate(titles, 1))
    (folder / "README.md").write_text(text, encoding="utf-8")
    script = folder / "repro.sh"
    script.write_text("#!/usr/bin/env bash\necho demo\n", encoding="utf-8")
    script.chmod(0o755)
    (folder / "expected-output.txt").write_text("demo\n", encoding="utf-8")
    return folder


class LessonCheckTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def problems(self):
        return lessons.check(self.root)[1]

    def test_missing_index_or_template_fails(self):
        self.assertTrue(self.problems())

    def test_complete_lesson_passes(self):
        write_lesson(self.root)
        self.assertEqual(lessons.check(self.root), (1, []))

    def test_missing_reordered_or_empty_section_fails(self):
        write_lesson(self.root, sections=lessons.SECTIONS[:-1])
        self.assertTrue(any("sections must be exactly" in p for p in self.problems()))

    def test_swapped_sections_fail(self):
        swapped = [lessons.SECTIONS[1], lessons.SECTIONS[0], *lessons.SECTIONS[2:]]
        write_lesson(self.root, sections=swapped)
        self.assertTrue(any("sections must be exactly" in p for p in self.problems()))

    def test_nearly_empty_section_fails(self):
        folder = write_lesson(self.root)
        readme = folder / "README.md"
        readme.write_text(readme.read_text(encoding="utf-8").replace(BODY + "執行 `./repro.sh`。", "略", 1)
                          + "\n執行 `./repro.sh`。\n", encoding="utf-8")
        self.assertTrue(any("nearly empty" in p for p in self.problems()))

    def test_complete_status_rejects_placeholders_but_draft_allows_them(self):
        folder = write_lesson(self.root)
        readme = folder / "README.md"
        readme.write_text(readme.read_text(encoding="utf-8") + "\n這段待補。\n", encoding="utf-8")
        self.assertTrue(any("待補" in p for p in self.problems()))
        readme.write_text(readme.read_text(encoding="utf-8").replace("狀態：完成", "狀態：草稿"), encoding="utf-8")
        self.assertEqual(self.problems(), [])

    def test_reproduction_script_and_output_are_required(self):
        folder = write_lesson(self.root)
        (folder / "repro.sh").chmod(0o644)
        (folder / "expected-output.txt").write_text("", encoding="utf-8")
        found = self.problems()
        self.assertTrue(any("not executable" in p for p in found))
        self.assertTrue(any("expected-output.txt" in p for p in found))

    def test_unlinked_misnamed_or_gapped_lessons_fail(self):
        write_lesson(self.root)
        base = self.root / "docs" / "lessons"
        (base / "README.md").write_text("# 索引\n", encoding="utf-8")
        (base / "Bad_Name").mkdir()
        found = self.problems()
        self.assertTrue(any("not linked" in p for p in found))
        self.assertTrue(any("named NN-short-name" in p for p in found))
        (base / "Bad_Name").rmdir()
        (base / "01-sample-bug").rename(base / "02-sample-bug")
        self.assertTrue(any("without gaps" in p for p in self.problems()))


if __name__ == "__main__":
    unittest.main()
