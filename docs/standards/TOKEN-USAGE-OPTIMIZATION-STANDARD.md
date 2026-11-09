# Token & Usage Optimization Standard (v1)

> **Instruction to Claude:** If this file is in a project, the project needs a **usage optimization plan**
> before substantive work begins. Run the planning phase in §2, write the plan to `docs/USAGE-PLAN.md`,
> and follow it for the rest of the project. Nobody has to ask again. If `docs/USAGE-PLAN.md` already
> exists, read it at the start of each session and work to it instead of re-planning.

---

## 1. What it is

A short, mandatory planning phase that decides **how** the project will spend tokens and usage before it
spends them. It produces one file, `docs/USAGE-PLAN.md`, covering five areas:

| # | Area | Question it answers |
|---|---|---|
| A | Model designation | Which model tier does each kind of task get? |
| B | Spin-off agents | What gets delegated to subagents, and what stays in the main thread? |
| C | Static tooling | Which repeated tasks become scripts instead of fresh reasoning each time? |
| D | Usage pacing | How is work spread across sessions, limits and cache windows? |
| E | Context narrowing | What does each step actually need to see, and how is the rest kept out? |

The goal is not minimum spend at any cost. It is **the right model, on the right context, doing work
that can't be done more cheaply** — with quality held constant.

## 2. Planning phase (what Claude does, once per project)

1. **Inventory the work.** Read the project brief / `CLAUDE.md` / plan file (`docs/PLAN.md` if the
   Progress Bar Standard is also present). List the task *types* the project contains, not every task
   (e.g. "scaffold", "bulk file edits", "architecture decisions", "test runs", "content generation").
2. **Assign tiers (§3).** Put every task type into the routing table.
3. **Decide delegation (§4).** Mark which task types run as subagents and define those agents.
4. **Find repeatables (§5).** Flag any task type expected to happen ≥3 times, in this project or across
   projects, and decide whether it becomes a script.
5. **Set pacing (§6).** Group work into sessions with an estimated weight and a checkpoint.
6. **Set context rules (§7).** Write the read/ignore rules and the lean `CLAUDE.md`.
7. **Write `docs/USAGE-PLAN.md`** using the template in §9. Keep it under ~150 lines.
8. **Tell the user** in 3–5 lines: the tier split, agents created, tools planned, and any decision that
   needs their input (e.g. whether to use the top tier at all). Then begin the project.

Keep the planning phase itself cheap: it should take one short pass, not a research project. If the
project is small (≈ one session of work), write a 10-line plan and move on.

## 3. Model designation (A)

Route by **what the task demands**, not by habit. Default every task to the cheapest tier that will do it
correctly on the first try; a failed cheap attempt plus a retry costs more than one correct mid-tier run.

| Tier | Alias | Use for | Don't use for |
|---|---|---|---|
| Light | `haiku` | File search/inventory, renames, formatting, boilerplate from a clear template, log/grep triage, summarizing tool output, simple data transforms, running and reporting tests | Anything with ambiguous requirements or cross-file reasoning |
| Standard | `sonnet` | Most implementation: features, bug fixes with a known cause, refactors within a module, writing tests, docs, routine reviews | Novel architecture, subtle concurrency/security bugs |
| Heavy | `opus` | Architecture and data-model decisions, multi-module refactors, hard debugging, security-sensitive code, writing the plan itself, final review of high-stakes work | Mechanical or repetitive work |
| Frontier | (top tier, if available) | Only when Heavy has demonstrably failed or the task is explicitly research-grade. Requires a note in the plan explaining why. | Anything routine |

Rules:
- **Write the routing table per project** in `USAGE-PLAN.md` (task type → tier → reason). One line each.
- **Pin tiers in agent definitions**, not in memory: `.claude/agents/<name>.md` frontmatter `model: haiku`
  / `sonnet` / `opus` / `inherit`. Use aliases, not dated model IDs, so the plan survives model updates.
- **Escalate, don't start high.** If a Standard-tier attempt fails twice on the same problem, escalate
  that task to Heavy and record it in the plan's log. If a task type keeps escalating, re-tier it.
- **De-escalate after decisions.** Once the Heavy tier has made the design call, implementation of that
  design drops back to Standard.
- The main thread's model is the user's choice. Don't switch it silently; recommend a switch when a
  phase's work clearly sits in another tier.

## 4. Spin-off agents (B)

Subagents keep large tool output out of the main context and can run in parallel. They also cost a
fresh context, a brief, and a lossy summary on return. Delegate only when that trade wins.

**Delegate when:**
- The task reads many files but only the **conclusion** matters (codebase sweeps, "where is X used",
  dependency audits).
- Tasks are **independent and parallel** (e.g. three unrelated modules to scaffold) — launch them in
  one message.
- The task is **mechanical and well-specified** and can run on a cheaper tier than the main thread.
- An **unanchored review** is wanted: give the reviewer the code, not your conclusion.

**Keep in the main thread when:**
- It's a handful of tool calls or a lookup whose target is already known.
- Each finding shapes the next step (iterative debugging, design work).
- The result has to be understood in detail, not summarized.

Rules:
- **Define reusable agents as files** in `.claude/agents/` with a narrow tool list and a pinned model.
  Starter set for most projects:
  - `scout` — read-only search/inventory, `model: haiku`, tools: Read, Grep, Glob.
  - `implementer` — scoped edits to named files, `model: sonnet`.
  - `reviewer` — read-only review of a diff against stated requirements, `model: opus` for high-stakes
    work, `sonnet` otherwise.
  - `runner` — runs tests/builds and returns only failures, `model: haiku`.
- **Brief like a peer.** Every delegation states: goal, files/paths in scope, what's already ruled out,
  output format, and a length cap on the return ("≤ 20 lines; file:line refs, no code dumps").
- **Narrow scope beats big scope.** Three tight agents beat one agent told to "look at everything".
- **Don't duplicate.** Once delegated, wait for the result; don't also do the work in the main thread.
- **No agent fan-out for its own sake.** If the plan shows more than ~5 parallel agents in one step,
  justify it in the plan.

## 5. Static tooling (C)

Any reasoning the model does the same way more than twice is a script waiting to be written. A script
costs tokens once; re-deriving the same steps costs tokens every time and drifts.

**Make it a tool when** a task is:
- repeated ≥3 times in this project, **or** likely to recur in other projects;
- deterministic (same input → same output);
- currently done by reading/pasting large output and reasoning over it (parsing logs, diffing configs,
  generating boilerplate, validating file formats, counting/aggregating data).

Rules:
- **Project-local tools** go in `scripts/` with a one-line usage comment at the top and a one-line entry
  in `docs/USAGE-PLAN.md` → *Tools*. Prefer the language already used by the project; otherwise Node
  (`.mjs`) or Python with no unusual dependencies.
- **Cross-project tools** are promoted to the shared assets folder (the folder this file came from) as
  `tools/<name>/` with a `README.md`, and referenced by path from each project. Promote a tool when a
  second project needs it — not speculatively.
- **Tools return small output.** Design them to print a summary and only the lines that need attention
  (e.g. `12 passed, 2 failed:` then the two failures), never full dumps. Add `--json` if another tool
  will consume it.
- **Freeze, don't fork.** Once a tool is shared, copy it verbatim into projects (as the Progress Bar
  Standard does with `progress.mjs`). Improvements go back to the shared copy with a version bump.
- **Skills for procedures, scripts for computation.** If the repeated thing is a *procedure with
  judgment* (a review checklist, a release routine), write it as a skill or a standard `.md` like this
  one. If it's computation, write a script.
- During the project, whenever Claude notices itself repeating a pattern a third time, it stops, writes
  the tool, and logs it — that counts as part of the task, not a detour.

## 6. Usage pacing (D)

Spend evenly and predictably rather than in bursts that hit limits mid-task.

Rules:
- **Size sessions to phases.** Each session in the plan covers a coherent chunk with a clear end state
  and a checkpoint (commit + plan tick). Estimate each as **S / M / L** by expected tool calls and
  files touched. Avoid back-to-back L sessions where limits are tight.
- **Front-load the cheap, back-load the expensive.** Inventory, scaffolding and tooling (Light/Standard)
  come first; Heavy-tier design work is concentrated in a few deliberate sessions, not sprinkled
  throughout.
- **Checkpoint before limits.** At a natural stopping point — or when a session is getting long — commit,
  tick the plan, and write a 3–5 line *handoff note* at the bottom of `USAGE-PLAN.md` (what's done,
  what's next, open questions). A new session starts from that note, not from re-reading the repo.
- **Batch over drip.** Group related edits into one pass; send independent tool calls together; run the
  test suite once after a batch of changes, not after each line.
- **Respect cache windows.** Keep a working session continuous rather than leaving long idle gaps
  mid-task, and keep stable content (`CLAUDE.md`, standards, system context) unchanged during a session
  so it stays cached. For API-driven workloads, use prompt caching for stable prefixes and the Batch API
  for non-urgent bulk jobs.
- **Long-running or unattended work** (scheduled tasks, bulk generation) runs on the lowest viable tier
  and reports a one-line summary, not a narrative.
- **Measure.** At each checkpoint, note rough usage (e.g. `/cost` or the plan's usage view) against the
  estimate. If a session type consistently runs over, re-tier, delegate, or tool it.

## 7. Context narrowing (E)

Every token in context is paid for on every turn that follows. Keep the working set to what the current
step needs.

Rules:
- **Lean `CLAUDE.md`.** ≤ ~60 lines: what the project is, how to build/test, conventions, and pointers
  (paths) to longer docs. Long reference material lives in separate files read *on demand*.
- **Search before reading.** Grep/Glob to locate, then read the relevant **line range** — not whole
  files. Never read generated, vendored or minified files unless the task is about them.
- **Ignore list.** Record in the plan the paths that are never read by default (e.g. `node_modules/`,
  `dist/`, `build/`, lockfiles, large fixtures, media, logs) and enforce with deny rules in
  `.claude/settings.json` where the tool supports it.
- **Delegate dumps.** Anything that would bring back hundreds of lines (full test logs, large diffs,
  directory trees) goes through a subagent or a tool that summarizes (§4, §5).
- **Reset between phases.** Start a new session (or clear/compact) when moving to an unrelated phase;
  carry state through the handoff note and the plan file, not through conversation history.
- **Don't re-read what you just wrote.** Trust successful edits; verify by running, not by re-reading.
- **Scope instructions to the task.** When briefing an agent, include only the files and facts it needs;
  don't paste the whole plan.
- **Prune standards.** Only standards that apply to the current project should sit in it. Remove ones
  that don't.

## 8. Ongoing obligations (every session after planning)

1. Read `docs/USAGE-PLAN.md` (especially the latest handoff note) before starting work.
2. Route tasks per the tier table; use the defined agents and tools.
3. When a task type is re-tiered, a new agent is defined, or a new tool is written, update the plan in
   the same commit.
4. At each checkpoint, append a handoff note and a one-line usage observation.
5. At project end, list any project-local tools worth promoting to the shared assets folder and tell
   the user.

## 9. `docs/USAGE-PLAN.md` template

```markdown
# Usage Plan — <project name>
_Standard: TOKEN-USAGE-OPTIMIZATION-STANDARD v1 · Created <YYYY-MM-DD>_

## Routing (A)
| Task type | Tier | Why |
|---|---|---|
| Codebase inventory | haiku | read-only, conclusion only |
| Feature implementation | sonnet | default build work |
| Data model / architecture | opus | decisions are expensive to reverse |

## Agents (B)
| Agent | Model | Tools | Used for |
|---|---|---|---|
| scout | haiku | Read, Grep, Glob | locate code, audit deps |
| runner | haiku | Bash | run tests, return failures only |

## Tools (C)
| Tool | Path | Scope | Replaces |
|---|---|---|---|
| <name> | scripts/<name>.mjs | project / shared | <manual step it replaces> |
Candidates to watch: <task types that may become tools>

## Pacing (D)
| Session | Covers | Size | Checkpoint |
|---|---|---|---|
| 1 | Inventory + tooling | S | tools committed |
| 2 | Core design | M (opus) | design doc committed |

## Context (E)
- Never read by default: <paths>
- On-demand docs: <paths>
- CLAUDE.md line budget: 60

## Log
- <YYYY-MM-DD> — <re-tier / new tool / escalation and why>

## Handoff
<latest 3–5 line note: done, next, open questions>
```

## 10. Anti-patterns (never)

- Running the whole project on the heaviest tier "to be safe".
- Spawning agents for lookups that take one or two tool calls.
- Re-deriving the same parsing/formatting logic by hand in every session.
- Reading entire large files, lockfiles or build output to find one value.
- Pasting long tool output into the conversation instead of summarizing it.
- Letting `CLAUDE.md` grow into a manual.
- Optimizing usage at the expense of correctness — if a cheaper path produces worse work, it isn't
  cheaper.
