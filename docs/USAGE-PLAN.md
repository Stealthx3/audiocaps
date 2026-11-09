# Usage Plan — AudioCaps
_Standard: TOKEN-USAGE-OPTIMIZATION-STANDARD v1 · Created 2026-11-08_

## Routing (A)
| Task type | Tier | Why |
|---|---|---|
| Data model, API shape, schema, crate boundaries | opus | expensive to reverse; every later phase builds on it |
| Parsers (ELD, EDID, HDA, UAC, procfs) | sonnet | spec-driven implementation with tests |
| Backends (ALSA, PipeWire, Pulse, BlueZ, JACK) | sonnet | known APIs; escalate concurrency/passivity bugs to opus |
| Rule engine design, path-report algebra | opus | cross-layer reasoning |
| Rule pack entries, quirk data, fixtures | sonnet | mechanical once the engine exists |
| Docs, changelog, log entries | sonnet | routine writing |
| Build/test runs in the lab, fuzz triage | haiku | report failures only |
| Codebase inventory, dependency/license audit | haiku | conclusion only |
| Gate review before each phase hand-off | opus | high-stakes, unanchored review |

## Agents (B)
| Agent | Model | Tools | Used for |
|---|---|---|---|
| scout | haiku | Read, Grep, Glob | locate code, inventory fixtures, audit deps |
| runner | haiku | Bash | `scripts/lab.sh run` builds/tests; returns failures only |
| reviewer | opus | Read, Grep, Glob, Bash | gate review of a phase diff against its gate |

Defined in `.claude/agents/`. Small lookups stay in the main thread.

## Tools (C)
| Tool | Path | Scope | Replaces |
|---|---|---|---|
| progress | scripts/progress.mjs | shared (verbatim) | hand-written progress page |
| repo-check | scripts/repo-check.mjs | shared (verbatim) | manual repo audits, README progress block, log entries |
| lab | scripts/lab.sh | shared (verbatim) | raw virsh/ssh for builds and tests |
| phone-alerts | scripts/phone-alerts.mjs | shared (verbatim) | manual notification setup |
| gate | scripts/gate.sh | project | fmt + clippy + test + deny in one lab call, summary only |
| scan-history | scripts/scan-history.sh | project | checking git history for IPs, MACs, plan docs, captures |
Candidates to watch: fixture redaction check, capture diffing, fuzz summary.

## Pacing (D)
| Session | Covers | Size | Checkpoint |
|---|---|---|---|
| P1-S1 | Standards, repo, Pages, lab VM | M | standards committed on main |
| P1-S2 | Workspace, deny, model, schema | L (opus) | schema v1 committed |
| P1-S3 | SystemRoot, capture tool, replay | M | capture + snapshot tests green |
| P1-S4 | Five captures, docs, gate | M | gate summary to Micaiah |
Later phases: one or two sessions per roadmap phase; never two L sessions back to back.

## Context (E)
- Never read by default: `target/`, `Cargo.lock`, `schema/*.json` (generated), `tests/fixtures/**` (large), `docs/progress/`, `docs/plan/*.pdf` (use the text extract in the scratchpad)
- On-demand docs: `docs/plan/*`, `docs/standards/*`, `docs/architecture.md`
- CLAUDE.md line budget: ~100 (project rules are dense; standards are referenced, not inlined)

## Log
- 2026-11-08 — Plan created. Insta replaced by expect-test (license), so no snapshot-review tool; `UPDATE_EXPECT=1` re-blesses.

## Handoff
Phase 1 session 1: standards adopted on `main`, repo public at github.com/Stealthx3/audiocaps, Pages workflow in place. Next: branch `feat/phase-1-scaffold-model-schema`, bring up `lab-audiocaps-main`, then the workspace.
