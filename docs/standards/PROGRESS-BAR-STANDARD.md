# Progress Bar Standard (v1)

> **Instruction to Claude:** If this file is in a project, the project needs a **live progress page**.
> Build it to this standard and keep it current for the rest of the project. Nobody has to ask again.
> The reference is the *WCR Migration Progress* artifact (wordscourtreporting.com, Duda → Nginx, Oct 2026).

---

## 1. What it is

One static HTML page, published as a claude.ai **Artifact**. It shows how far the project's plan has
progressed. It is **generated** from the checkboxes in the project's plan file, never written by hand,
so it can't drift from the plan.

What the page contains, top to bottom:

1. **Eyebrow:** small mono uppercase line with the project context (e.g. `domain.com · Old → New`).
2. **H1:** italic serif heading in the brand color (e.g. "Site migration progress").
3. **Subline:** "Tracks the task checkboxes in `docs/PLAN.md`. Updated as each task is committed."
4. **Stats row:** a big percentage, then *Tasks* (`N of M done`), *Current phase* (`Phase N — Title`), and *Next task* (`id text`, cut to 70 characters).
5. **Rail:** a 14px rounded track with a striped, animated fill at the overall %. Each phase has a
   marker dot labelled `P0`, `P1`, … at the point where that phase's tasks **start**.
6. **Legend:** Complete (green) · In progress (brand) · Not started (tan/grey).
7. **Phase ledgers:** one collapsible `<details>` card per phase showing the phase number, title, status pill,
   `done/total` count, a 4px mini bar, and the task list (checkbox square, mono task id, text, and an
   "Up next" tag on the next task).
8. **Footer:** `Last updated Mon D, YYYY · h:mm AM` in the user's time zone.

## 2. Source of truth: the plan file

Default path: `docs/PLAN.md` (can be changed in config). The generator understands this format:

```markdown
## Phase 0 — Capture the reference        ← "## " or "### ", "Phase <n>", then — / – / - / : and a title
- [x] **0.1** Scaffold `package.json` …   ← top-level checkbox = one task
  - [ ] sub-step                           ← indented items are ignored (details, not tasks)
- [ ] 0.2 Download every asset …           ← id may be bold, plain, or missing
- [ ] Write the manifest                   ← no id → auto-numbered <phase>.<index>
```

Rules:
- **Only top-level `- [ ]` / `- [x]` lines under a Phase heading count.** Everything else in the file
  (decisions, notes, sub-bullets) is ignored.
- Use `<phase>.<n>` ids (`0.1`, `3.10`). Phases may start at 0 or 1.
- Only the first line of a task is displayed, so put the meaning first.
- If the project has no plan file yet, **write one in this format first**, broken into phases of about
  4–10 tasks each.

## 3. Workflow (what Claude does)

**Setup (once per project)**
1. Make sure the plan file exists and uses the format above.
2. Create `scripts/progress.mjs` from §8 **verbatim**. Don't rewrite or "improve" it per project.
3. Create `progress.config.json` in the repo root (§6) with the project's title, eyebrow and brand colors.
4. Run `node scripts/progress.mjs`. It writes `docs/progress/index.html`.
5. Publish that file with the **Artifact** tool (icon: `chart`). Write the returned URL into
   `progress.config.json` → `"artifactUrl"`, and put a one-line link in the project's `CLAUDE.md`/README.

**Every time a task is finished**
1. Tick its box in the plan (`- [ ]` → `- [x]`) **in the same commit as the work**.
2. Re-run `node scripts/progress.mjs`.
3. Republish to the **same URL** (pass `artifactUrl` as `url`). Never create a second artifact.
   A new conversation must reuse the URL stored in the config.
4. Before republishing, check the console summary
   (`progress: 12/49 (24%) · phase 2 · next 2.3`) against what was just done.

**When the plan changes** (tasks added, split or reordered): edit the plan, regenerate and republish.
The percentage and marker positions update on their own. Never hard-code numbers in the HTML.

## 4. Computation rules (must match exactly)

| Value | Rule |
|---|---|
| Total / done | Count top-level tasks across phases that have ≥1 task (empty phases are skipped). |
| Big % | `floor(done/total × 100)`. Shows **100 only when everything is done** (no rounding up to 100). |
| Fill width | `done/total × 100`, 1 decimal, set as the `--pct` CSS variable. |
| Marker position | `--at = tasks in all earlier phases / total × 100`, 2 decimals. P0 is always 0% and left-aligned. Example from WCR: P1 at 10/49 = 20.41%. |
| Phase state | `done` = all tasks ticked. `active` = the **first** phase that is not done (never more than one). `todo` = every phase after it. |
| Next task | The first unticked task in the active phase. Gets class `now` and the "Up next" tag. |
| Open ledger | Only the active phase's `<details>` is `open`. |
| Labels | Complete / In progress / Not started (pills and screen-reader text). |
| All done | Current phase = "All phases complete", Next task = "—". |
| Timestamp | Time of generation, `en-US`, in the config time zone (default `America/Los_Angeles`). |

## 5. Visual spec

**Tokens.** Every color is a CSS variable on `:root`, redefined for dark mode under
`@media (prefers-color-scheme: dark) :root:not([data-theme="light"])` **and** `:root[data-theme="dark"]`.

| Token | Role | WCR light | WCR dark |
|---|---|---|---|
| `--bg` | page | `#f6f3ec` | `#171312` |
| `--surface` | cards, marker interiors | `#fffdf8` | `#211b19` |
| `--fg` | text | `#1d1a18` | `#f1ebe2` |
| `--muted` | secondary text | `#6d655e` | `#a99f94` |
| `--line` | borders, task dividers | `#ddd5c4` | `#3a312d` |
| `--brand` | H1, fill, active state | `#700908` | `#d4574f` |
| `--brand-soft` | empty track, code background | `#e2dcc6` | `#4a3a2f` |
| `--done` | complete state (always a green) | `#2f6b3a` | `#6fbf7c` |
| `--todo` | not-started outlines | `#b8ad98` | `#5c524b` |

**Type.** Three roles: `--display` (italic 700 serif, H1 only; Merriweather by default), `--body`
(Poppins 400/600/700), `--mono` (IBM Plex Mono 400/500; eyebrow, labels, ids, counts, footer).
Numbers use `font-variant-numeric: tabular-nums`. Fonts load only from Google Fonts.

**Sizes.** Content column max 920px; side gutter `max(16px, safe-area)`; padding 40px top / 64px bottom.
H1 `clamp(26px, 4vw, 36px)`. Big % 44px with a 20px muted `%`. Track 14px with radius 7. Marker dot 24px with a 3px
border, `top:-5px` (18px on phones). Ledger cards radius 10 with 1px `--line` border; the active card's border is
mixed 45% toward brand. Mini bar 4px.

**State colors.** Done = `--done` (filled dot, filled box, green pill, green mini bar, muted text).
Active = `--brand` (hollow dot with a brand ring and pulse, brand pill, brand mini bar, brand box outline,
"Up next" in brand). Todo = `--todo` outline, muted pill.

**Motion.**
- Fill grows from `scaleX(.02)` over 1.4s `cubic-bezier(.2,.8,.2,1)`.
- The −45° stripes (10px brand / 10px brand-82%-mixed-with-surface) scroll every 1.2s.
- The active dot pulses a 10px ring every 1.8s.
- **All animation stops under `prefers-reduced-motion`.**

**Accessibility.**
- The rail is `role="img"` with `aria-label="N percent of tasks complete"`.
- Each marker has visually hidden text: "Phase N, Complete".
- Decorative dots, boxes and mini bars are `aria-hidden`.
- `summary:focus-visible` gets a 2px brand outline.
- Done-state text uses `--muted`, never lower contrast than that.

**Responsive (≤560px).**
- Marker labels shrink to 10px and dots to 18px.
- The summary grid becomes 2 columns, with the phase number on its own row.
- The task grid drops the "Up next" column, which moves under the text.
- There must be **no horizontal scroll at 390px**.

## 6. Adapting to a new project

Change **only** the config. Don't touch the layout, the CSS structure, the sizes, the motion or the semantics.

```json
{
  "pageTitle": "Acme Rebuild Progress",
  "eyebrow": "acme.com · WordPress → Astro",
  "heading": "Rebuild progress",
  "plan": "docs/PLAN.md",
  "out": "docs/progress/index.html",
  "timezone": "America/Los_Angeles",
  "artifactUrl": "",
  "theme": {
    "light": { "brand": "#0b4f8a", "brand-soft": "#d9e3ee" },
    "dark":  { "brand": "#6aa8e0", "brand-soft": "#24384c" }
  },
  "fonts": { }
}
```

- `pageTitle` is the browser tab and artifact title: 2–4 words, `<Project> Progress`.
- **Brand color:** take it from the project (client logo or site CSS, or the product's accent color). If the project
  has none, keep the WCR maroon. Always set a lighter dark-mode brand (about 4.5:1 contrast against `#211b19`).
  `brand-soft` is the brand mixed about 80–85% toward `--bg`.
- Leave `done` green and `todo` neutral in every project, so the meaning of each state stays the same everywhere.
  You may move `bg`/`surface`/`line` to a cooler neutral if the brand clashes with sand.
- `fonts` can swap the three families (`href`, `display`, `body`, `mono`). Keep the roles: an italic serif
  display face, a geometric sans for body text, a mono face for labels.
- Unspecified keys fall back to the WCR defaults inside the script.

## 7. Don'ts

- Don't build the page by hand or let an LLM "update the numbers": always regenerate from the plan.
- Don't add JavaScript, charts, frameworks or external scripts. It is pure HTML/CSS.
- Don't use `localStorage` or runtime capabilities. Republishing is the update mechanism.
- Don't publish a new artifact for each update, and don't rename the page between updates.
- Don't count sub-bullets, show more than one active phase, or round the percentage up to 100.

## 8. Reference implementation: `scripts/progress.mjs`

Node ≥18, no dependencies. Copy verbatim. Run `node scripts/progress.mjs [config path]`.

```js
#!/usr/bin/env node
// Progress page generator — PROGRESS-BAR-STANDARD v1. No dependencies.
// Usage: node scripts/progress.mjs [path/to/progress.config.json]
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname, resolve } from "node:path";

const cfgPath = resolve(process.argv[2] || "progress.config.json");
const cfg = existsSync(cfgPath) ? JSON.parse(readFileSync(cfgPath, "utf8")) : {};
const C = {
  pageTitle: "Project Progress",
  eyebrow: "",
  heading: "Project progress",
  plan: "docs/PLAN.md",
  out: "docs/progress/index.html",
  timezone: "America/Los_Angeles",
  ...cfg,
  theme: {
    light: { bg: "#f6f3ec", surface: "#fffdf8", fg: "#1d1a18", muted: "#6d655e", line: "#ddd5c4",
             brand: "#700908", "brand-soft": "#e2dcc6", done: "#2f6b3a", todo: "#b8ad98", ...(cfg.theme?.light || {}) },
    dark:  { bg: "#171312", surface: "#211b19", fg: "#f1ebe2", muted: "#a99f94", line: "#3a312d",
             brand: "#d4574f", "brand-soft": "#4a3a2f", done: "#6fbf7c", todo: "#5c524b", ...(cfg.theme?.dark || {}) },
  },
  fonts: {
    href: "https://fonts.googleapis.com/css2?family=Merriweather:ital,wght@1,700&family=Poppins:wght@400;600;700&family=IBM+Plex+Mono:wght@400;500&display=swap",
    display: '"Merriweather", Georgia, serif', body: '"Poppins", system-ui, sans-serif', mono: '"IBM Plex Mono", ui-monospace, monospace',
    ...(cfg.fonts || {}),
  },
};
const base = dirname(cfgPath);
const planFile = resolve(base, C.plan);
const outFile = resolve(base, C.out);

// ---------- parse the plan ----------
const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#x27;");
const inline = (s) => esc(s)
  .replace(/`([^`]+)`/g, "<code>$1</code>")
  .replace(/\*\*([^*]+)\*\*/g, "$1").replace(/__([^_]+)__/g, "$1")
  .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1");
const plain = (s) => s.replace(/`([^`]+)`/g, "$1").replace(/\*\*([^*]+)\*\*/g, "$1").replace(/\[([^\]]+)\]\([^)]+\)/g, "$1");

const PHASE_RE = /^#{2,3}\s+Phase\s+([\w.]+)\s*(?:[:—–-]+\s*)?(.*)$/i;
const TASK_RE = /^[-*]\s+\[([ xX])\]\s+(.*)$/;          // top-level only (no indent)
const ID_RE = /^(?:\*\*)?([A-Za-z]?\d+(?:\.\d+)+)\.?(?:\*\*)?[\s:.)—–-]*\s*/;

const phases = [];
for (const raw of readFileSync(planFile, "utf8").split(/\r?\n/)) {
  const ph = raw.match(PHASE_RE);
  if (ph) { phases.push({ n: ph[1], title: ph[2].trim().replace(/\*\*/g, ""), tasks: [] }); continue; }
  const t = raw.match(TASK_RE);
  if (t && phases.length) {
    const p = phases.at(-1);
    let text = t[2].trim(), id;
    const m = text.match(ID_RE);
    if (m) { id = m[1]; text = text.slice(m[0].length); }
    else id = `${p.n}.${p.tasks.length + 1}`;
    p.tasks.push({ id, text, done: t[1].toLowerCase() === "x" });
  }
}
const live = phases.filter((p) => p.tasks.length);
if (!live.length) { console.error(`No "## Phase N — Title" sections with "- [ ]" tasks found in ${planFile}`); process.exit(1); }

// ---------- compute ----------
const total = live.reduce((a, p) => a + p.tasks.length, 0);
const doneN = live.reduce((a, p) => a + p.tasks.filter((t) => t.done).length, 0);
const pctExact = (doneN / total) * 100;
const pctShown = doneN === total ? 100 : Math.floor(pctExact);
let before = 0, activeFound = false, next = null;
for (const p of live) {
  p.at = (before / total) * 100; before += p.tasks.length;
  p.doneN = p.tasks.filter((t) => t.done).length;
  if (p.doneN === p.tasks.length) p.state = "done";
  else if (!activeFound) { p.state = "active"; activeFound = true; next = p.tasks.find((t) => !t.done); next.now = true; }
  else p.state = "todo";
}
const active = live.find((p) => p.state === "active");
const LABEL = { done: "Complete", active: "In progress", todo: "Not started" };
const r2 = (n) => Math.round(n * 100) / 100;
const trunc = (s, n) => (s.length > n ? s.slice(0, n - 1).trimEnd() + "…" : s);
const stamp = new Intl.DateTimeFormat("en-US", { timeZone: C.timezone, month: "short", day: "numeric", year: "numeric" }).format(new Date())
  + " · " + new Intl.DateTimeFormat("en-US", { timeZone: C.timezone, hour: "numeric", minute: "2-digit" }).format(new Date());

// ---------- render ----------
const vars = (o) => Object.entries(o).map(([k, v]) => `--${k}: ${v};`).join(" ");
const markers = live.map((p) =>
  `<li class="m ${p.state}" style="--at:${r2(p.at)}%"><span class="dot" aria-hidden="true"></span><span class="mk">P${esc(p.n)}</span><span class="sr">Phase ${esc(p.n)}, ${LABEL[p.state]}</span></li>`).join("");
const ledgers = live.map((p) => {
  const tasks = p.tasks.map((t) =>
    `<li class="t ${t.done ? "done" : t.now ? "now" : ""}"><span class="box" aria-hidden="true"></span><span class="tid">${esc(t.id)}</span><span class="tx">${inline(t.text)}</span>${t.now ? '<span class="nowtag">Up next</span>' : ""}</li>`).join("");
  return `<details class="ph ${p.state}" ${p.state === "active" ? "open" : ""}><summary><span class="pn">Phase ${esc(p.n)}</span><span class="pt">${inline(p.title)}</span><span class="pill ${p.state}">${LABEL[p.state]}</span><span class="cnt">${p.doneN}/${p.tasks.length}</span><span class="mini" aria-hidden="true"><span style="width:${r2((p.doneN / p.tasks.length) * 100)}%"></span></span></summary><ol class="tasks">${tasks}</ol></details>`;
}).join("");

const html = `<!doctype html>
<html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>${esc(C.pageTitle)}</title>
<meta name="claude:viewport-fit-sides" content="cover">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="stylesheet" href="${esc(C.fonts.href)}">
<style>
/* Generated by scripts/progress.mjs (PROGRESS-BAR-STANDARD v1). Do not hand-edit; edit the plan and regenerate. */
:root { ${vars(C.theme.light)} --display: ${C.fonts.display}; --body: ${C.fonts.body}; --mono: ${C.fonts.mono}; color-scheme: light; }
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { ${vars(C.theme.dark)} color-scheme: dark } }
:root[data-theme="dark"] { ${vars(C.theme.dark)} color-scheme: dark }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg); font: 15px/1.55 var(--body); }
.wrap { max-width: 920px; margin: 0 auto; padding-block: 40px 64px;
  padding-left: max(16px, env(safe-area-inset-left, 0px)); padding-right: max(16px, env(safe-area-inset-right, 0px)); }
.eyebrow { font: 500 12px/1 var(--mono); letter-spacing: .08em; text-transform: uppercase; color: var(--muted); }
h1 { font: italic 700 clamp(26px, 4vw, 36px)/1.15 var(--display); color: var(--brand); margin: 8px 0 6px; text-wrap: balance; }
.sub { color: var(--muted); margin: 0; }
.stats { display: flex; flex-wrap: wrap; gap: 10px 28px; margin: 28px 0 18px; align-items: baseline; }
.big { font: 700 44px/1 var(--body); font-variant-numeric: tabular-nums; }
.big small { font-size: 20px; color: var(--muted); font-weight: 600; }
.stat { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.stat b { font: 600 15px/1.3 var(--body); }
.stat span { font: 12px/1.2 var(--mono); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
.rail { position: relative; margin: 8px 0 54px; }
.track { height: 14px; border-radius: 7px; background: var(--brand-soft); overflow: hidden; position: relative; }
.fill { height: 100%; width: var(--pct); background: repeating-linear-gradient(-45deg, var(--brand) 0 10px, color-mix(in srgb, var(--brand) 82%, var(--surface)) 10px 20px);
  background-size: 28px 28px; border-radius: 7px; transform-origin: left; animation: grow 1.4s cubic-bezier(.2,.8,.2,1) both, stripe 1.2s linear infinite; }
@keyframes grow { from { transform: scaleX(.02); } to { transform: scaleX(1); } }
@keyframes stripe { to { background-position: 28px 0; } }
.markers { list-style: none; margin: 0; padding: 0; position: absolute; inset: 0; }
.m { position: absolute; left: var(--at); top: -5px; transform: translateX(-50%); display: flex; flex-direction: column; align-items: center; gap: 8px; }
.m:first-child { transform: none; align-items: flex-start; }
.dot { width: 24px; height: 24px; border-radius: 50%; background: var(--surface); border: 3px solid var(--todo); }
.m.done .dot { background: var(--done); border-color: var(--done); }
.m.active .dot { border-color: var(--brand); background: var(--surface); box-shadow: 0 0 0 0 color-mix(in srgb, var(--brand) 55%, transparent); animation: pulse 1.8s ease-out infinite; }
@keyframes pulse { 70% { box-shadow: 0 0 0 10px transparent; } 100% { box-shadow: 0 0 0 0 transparent; } }
.mk { font: 600 12px/1 var(--mono); color: var(--muted); }
.m.active .mk { color: var(--brand); }
.m.done .mk { color: var(--done); }
.sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
.legend { display: flex; gap: 18px; flex-wrap: wrap; font-size: 13px; color: var(--muted); margin-bottom: 26px; }
.legend i { display: inline-block; width: 10px; height: 10px; border-radius: 50%; margin-right: 6px; vertical-align: 0; }
.ph { background: var(--surface); border: 1px solid var(--line); border-radius: 10px; margin-bottom: 10px; }
.ph.active { border-color: color-mix(in srgb, var(--brand) 45%, var(--line)); }
summary { display: grid; grid-template-columns: auto 1fr auto auto; gap: 4px 14px; align-items: center; padding: 14px 18px; cursor: pointer; list-style: none; }
summary::-webkit-details-marker { display: none; }
summary:focus-visible { outline: 2px solid var(--brand); outline-offset: 2px; border-radius: 10px; }
.pn { font: 500 12px/1 var(--mono); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
.pt { font-weight: 600; min-width: 0; }
.pill { font: 600 11px/1 var(--body); padding: 5px 9px; border-radius: 99px; text-transform: uppercase; letter-spacing: .05em; }
.pill.done { background: color-mix(in srgb, var(--done) 16%, transparent); color: var(--done); }
.pill.active { background: color-mix(in srgb, var(--brand) 14%, transparent); color: var(--brand); }
.pill.todo { background: color-mix(in srgb, var(--todo) 22%, transparent); color: var(--muted); }
.cnt { font: 500 13px/1 var(--mono); font-variant-numeric: tabular-nums; color: var(--muted); }
.mini { grid-column: 1 / -1; height: 4px; background: var(--brand-soft); border-radius: 2px; overflow: hidden; }
.mini span { display: block; height: 100%; background: var(--done); }
.ph.active .mini span { background: var(--brand); }
.tasks { list-style: none; margin: 0; padding: 0 18px 14px; display: grid; gap: 2px; }
.t { display: grid; grid-template-columns: 16px 36px 1fr auto; gap: 10px; align-items: baseline; padding: 6px 0; border-top: 1px solid var(--line); font-size: 14px; }
.t .tx { min-width: 0; overflow-wrap: anywhere; }
.box { width: 12px; height: 12px; border: 2px solid var(--todo); border-radius: 3px; transform: translateY(1px); }
.t.done .box { background: var(--done); border-color: var(--done); }
.t.done .tx { color: var(--muted); }
.t.now .box { border-color: var(--brand); }
.tid { font: 500 12px var(--mono); color: var(--muted); font-variant-numeric: tabular-nums; }
.nowtag { font: 600 11px/1 var(--body); color: var(--brand); white-space: nowrap; }
code { font: 12.5px var(--mono); background: color-mix(in srgb, var(--brand-soft) 55%, transparent); padding: 1px 4px; border-radius: 4px; }
footer { margin-top: 28px; font: 12px var(--mono); color: var(--muted); }
@media (max-width: 560px) { .mk { font-size: 10px; } .dot { width: 18px; height: 18px; } summary { grid-template-columns: 1fr auto; } .pn { grid-column: 1 / -1; } .t { grid-template-columns: 16px 30px 1fr; } .nowtag { grid-column: 3; } }
@media (prefers-reduced-motion: reduce) { .fill, .m.active .dot { animation: none; } }
</style></head><body>
<div class="wrap">
  ${C.eyebrow ? `<div class="eyebrow">${esc(C.eyebrow)}</div>` : ""}
  <h1>${esc(C.heading)}</h1>
  <p class="sub">Tracks the task checkboxes in <code>${esc(C.plan)}</code>. Updated as each task is committed.</p>
  <div class="stats">
    <div class="big">${pctShown}<small>%</small></div>
    <div class="stat"><span>Tasks</span><b>${doneN} of ${total} done</b></div>
    <div class="stat"><span>Current phase</span><b>${active ? `Phase ${esc(active.n)} — ${inline(active.title)}` : "All phases complete"}</b></div>
    <div class="stat"><span>Next task</span><b>${next ? `${esc(next.id)} ${esc(trunc(plain(next.text), 70))}` : "—"}</b></div>
  </div>
  <div class="rail" role="img" aria-label="${pctShown} percent of tasks complete">
    <div class="track"><div class="fill" style="--pct:${pctExact.toFixed(1)}%"></div></div>
    <ol class="markers">${markers}</ol>
  </div>
  <div class="legend"><span><i style="background:var(--done)"></i>Complete</span><span><i style="background:var(--brand)"></i>In progress</span><span><i style="background:var(--todo)"></i>Not started</span></div>
  ${ledgers}
  <footer>Last updated ${stamp}</footer>
</div>
</body></html>
`;
mkdirSync(dirname(outFile), { recursive: true });
writeFileSync(outFile, html);
console.log(`progress: ${doneN}/${total} (${pctShown}%) · phase ${active ? active.n : "all done"} · next ${next ? next.id : "—"} → ${outFile}`);
```

## 9. Done checklist (run before every publish)

- [ ] The plan box is ticked in the same commit as the work.
- [ ] Generator ran without errors, and its summary line matches reality.
- [ ] Published to the stored `artifactUrl` (same page, same title).
- [ ] Light and dark both read well, and nothing scrolls sideways at 390px.
- [ ] (Setup only) URL saved to config and linked from `CLAUDE.md`/README.
