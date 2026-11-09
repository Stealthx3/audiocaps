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
