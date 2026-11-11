# Codex Execution Instructions

Each phase is a direct, standalone implementation task specification, not a vague planning prompt. Run `bash scripts/run_phase.sh 00`, then the next phase **only after** a successful local gate.

The runner feeds phase specification to Codex with:

```
codex exec --sandbox workspace-write --cd "$(pwd)" -
```

Prompts instruct Codex to read `AGENTS.md` and architecture documents. The runner does not use `--yolo`; it captures the CLI log and runs the gate in the user's WSL shell. The gate needs Rust/Cargo/pytest installed. If sandbox prevents cargo dependency resolution, run `cargo fetch` **outside** Codex sandbox and rerun, rather than disabling sandbox.

`gates.json` is an independent acceptance contract. `scripts/verify_gate.py` checks required files, Rust test discovery, `cargo fmt`, `cargo clippy`, `cargo test`, exact Python pytest nodes and required evidence report properties. It writes `.gates/phase-XX.json` only on success and fingerprints the source tree for the next phase's prerequisite check. A gate receipt is necessary but does not replace human code review.

Never run all nine phases in a single unreviewed prompt. Record a Git checkpoint and review the diff after each phase.
