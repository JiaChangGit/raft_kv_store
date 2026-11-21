"""The CI workflow and the clean rebuild must run the same checks.

.github/workflows/ci.yml runs on GitHub; scripts/clean_rebuild.py runs the
same steps locally in a fresh copy. If one of them gains or loses a step, the
other no longer tells the same story, so this test compares their commands.
"""
import ast
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Each command as written in the workflow; for clean_rebuild.py, the string
# parts of its argument list (the Python interpreter is a variable there).
COMMANDS = [
    "cargo fmt --all -- --check",
    "cargo clippy --locked --workspace --all-targets -- -D warnings",
    "cargo test --locked --workspace",
    "cargo build --locked --workspace --bins",
    "-m pytest -q tests scripts/test_verify_gate.py scripts/test_check_lessons.py scripts/test_mutation_check.py",
    "-m tools.learning_demo --output-dir",
]


def rebuild_commands():
    """The string parts of every run([...], label) call in clean_rebuild.py."""
    tree = ast.parse((ROOT / "scripts/clean_rebuild.py").read_text(encoding="utf-8"))
    found = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Call) and getattr(node.func, "id", None) == "run" and node.args \
                and isinstance(node.args[0], ast.List):
            parts = [item.value for item in node.args[0].elts if isinstance(item, ast.Constant) and isinstance(item.value, str)]
            found.append(" ".join(parts))
    return found


def test_the_ci_runs_the_same_steps_as_the_clean_rebuild():
    workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    rebuild = rebuild_commands()
    for command in COMMANDS:
        assert command in workflow, f"ci.yml does not run: {command}"
        assert any(command in line for line in rebuild), f"clean_rebuild.py does not run: {command}"
    # The toolchain comes from rust-toolchain.toml, not from a second copy of the version.
    assert "rust-toolchain.toml" in workflow
    assert "1.99" not in workflow, "the version is read from rust-toolchain.toml"
