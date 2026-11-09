# Repo Structure Standard (v1)

> **Instruction to Claude:** If this file is in a code project, run the **pre-work process** in §2
> before any other work: bring the git repo, branches, names and folder layout up to the conventions in
> §3–§4, put all auxiliary documentation under `docs/`, and make the README meet §5. Keep the decision
> & action log (§6) for the rest of the project. If the Progress Bar Standard is also present, embed the
> simplified progress block at the top of the README and make the full progress page publicly readable
> by link (§8). Nobody has to ask again.

---

## 1. What it is

A fixed starting shape for every code project: a clean git repo with conventional names, a predictable
folder layout, documentation in one place, and a README that tells a newcomer what the project is for,
what it looks like, what it does, where it came from, and how far along it is.

| Piece | Path | Role |
|---|---|---|
| Checker | `scripts/repo-check.mjs` | Copied **verbatim** from §10. Audits, scaffolds, refreshes the progress block, writes the log. |
| Config | `.repo-standard.json` | Optional per-project exceptions and fork details (§9). |
| Docs folder | `docs/` | All auxiliary documentation, images and generated pages. |
| Decision log | `docs/DECISIONS.md` | Dated history of the user's decisions and actions (§6). |
| README | `README.md` | The front page (§5). |

`repo-check.mjs` only reads the repo, runs git and writes docs; it executes no project code, so it runs
on the host under the VM Lab Standard §1.

## 2. Pre-work process

Run this when the standard first appears in a project, **before any feature work**. Re-run `check` at
the start of each plan phase and before every release or tag.

1. Copy §10 to `scripts/repo-check.mjs` verbatim. Don't rewrite or "improve" it per project.
2. Run `node scripts/repo-check.mjs check`. Read every `FAIL` and `warn` line.
3. Run `node scripts/repo-check.mjs scaffold`. It only **creates** what's missing and never overwrites:
   `git init -b main` if needed, `docs/`, `docs/images/`, `docs/DECISIONS.md` (first entry: adoption),
   `docs/CHANGELOG.md`, `.gitattributes`, a starter `.gitignore`, a README skeleton or the missing README
   sections, and an "Upstream" section when a fork is detected. It prints suggested `git mv` commands for
   documents that belong under `docs/`.
4. Sort the remaining fixes into two groups and handle them:

   | Do directly | Ask the user first (one message, listed together) |
   |---|---|
   | Create missing files and folders; fill README sections from what's known; add `.gitattributes`; untrack a committed `.env` (`git rm --cached`) and add it to `.gitignore` | Renaming the default branch (especially with a remote, open PRs or CI); renaming branches others use; moving documents with `git mv` (links change); renaming folders or files; renaming the repo or its folder; rewriting history to purge a secret; adding a LICENSE (the user picks it) |

   A tracked secret is always reported as urgent: untrack it now, tell the user it must be **rotated**
   (it's in the history), and offer the history rewrite.
5. Fill the README (§5) with real content. Where the product doesn't exist yet, make **mockups** (§5)
   instead of leaving placeholders.
6. If the Progress Bar Standard is present: follow §8.
7. Log each user decision and action from this process in `docs/DECISIONS.md` (§6).
8. Re-run `check` until it reports `COMPLIANT`. Remaining `warn` lines are acceptable only if the user
   chose to keep them; record that choice in the log (e.g. "keep `master` as default branch").
9. Commit as `docs: adopt repo structure standard` (plus `chore:` / `refactor:` commits for moves and
   renames, kept separate so history stays readable).

## 3. Git and naming conventions

| Topic | Convention | `check` |
|---|---|---|
| Default branch | `main` (override in `.repo-standard.json` only on the user's say-so) | FAIL if different; warn if `master` |
| Branch names | `<type>/<kebab-case>`; types: `feat`, `fix`, `hotfix`, `release`, `docs`, `chore`, `refactor`, `test`, `perf`, `ci`, `build`, `exp`, `deps`, `revert`; long-lived: `main`, `develop`, `staging`, `production`; tool branches `claude/…`, `dependabot/…`, `renovate/…` | warn per non-matching branch |
| Commit messages | [Conventional Commits](https://www.conventionalcommits.org/): `type(scope)!: summary`, imperative, ≤72 chars | warn under 80% of the last 50 |
| Tags and versions | [SemVer](https://semver.org/) `vMAJOR.MINOR.PATCH` | warn per non-SemVer tag |
| Line endings | `.gitattributes` with `* text=auto eol=lf` | warn if missing |
| Ignored files | `.gitignore` covering secrets, dependencies, build output, OS files, `.claude/*.local.json` | FAIL if missing |
| Secrets | Never tracked: `.env*` (except `.env.example`), keys, certificates, credential files | FAIL |
| Large files | Nothing over 20 MB in git; use Git LFS or release assets | warn |
| Repo and folder names | lowercase kebab-case (`my-project`, `src/utils`); no spaces anywhere | FAIL on spaces; warn on uppercase folders |

Ecosystem conventions win where they differ: .NET and Xcode projects keep PascalCase folders (detected
automatically), Go keeps `cmd/` and `internal/`, Python keeps a `src/<package>/` layout, Java keeps
`src/main/java/…`. Follow the ecosystem's standard layout for code (`src/`, `tests/`, `scripts/`, …);
this standard governs documentation, naming and git hygiene around it.

## 4. Folder layout and documentation

```text
<repo>/
├── README.md            front page (§5)
├── LICENSE              plus NOTICE if required
├── CLAUDE.md            short; lists the standards in docs/standards/
├── .gitignore  .gitattributes  .repo-standard.json  progress.config.json  (tool config)
├── src/ tests/ scripts/ …   (ecosystem layout)
└── docs/
    ├── DECISIONS.md     decision & action log (§6)
    ├── CHANGELOG.md     Keep a Changelog
    ├── CONTRIBUTING.md  SECURITY.md  CODE_OF_CONDUCT.md   (GitHub reads these from docs/)
    ├── PLAN.md  USAGE-PLAN.md  LAB.md  postmortem/   (from other standards)
    ├── images/          screenshots and mockups
    ├── progress/        generated progress page + README badge (§8)
    └── standards/       the ai-assets standards used by this project
```

Rules:
- **The root holds only** `README.md`, the license files, `CLAUDE.md`/`AGENTS.md`, and configuration
  that tools require at the root (manifests, dotfiles, `progress.config.json`). Every other document
  (`.md`, `.rst`, `.adoc`, `.pdf`, `.docx`, notes) lives under `docs/`. `check` FAILs on stray root
  documents.
- **Standards** (`*-STANDARD.md`) belong in `docs/standards/`, and `CLAUDE.md` must name each one so
  every session knows they apply. `check` warns on standards left at the root and FAILs if one in
  `docs/standards/` isn't named in `CLAUDE.md`.
- Use subfolders under `docs/` (e.g. `docs/architecture/`, `docs/api/`) once a topic has more than one
  file. Link everything from the README's Documentation section.

## 5. README spec

In this order:

1. **`# Project Name`**: the only H1.
2. **Progress block**: only with the Progress Bar Standard (§8). Generated; never hand-edited.
3. **Summary**: one short paragraph: what it is, who it's for, the problem it solves.
4. **`## Goals`**: what the project sets out to achieve and what "done" looks like (bullets or a short paragraph).
5. **`## Screenshots`**: at least one image under `docs/images/`, embedded with descriptive alt text.
   - Before the product exists, use **mockups** of the intended result: a UI mockup for apps and sites;
     a terminal session for CLIs; a usage snippet plus an architecture diagram for libraries and services.
     Make them with the session's design or diagram tools, or render HTML/SVG to PNG; caption each one
     *Mockup*.
   - Replace mockups with real screenshots as features land, and always before a release. Keep image
     files under ~500 KB (PNG/WebP).
6. **`## Features`**: a bullet list, one user-visible capability per bullet; mark unfinished ones
   *(planned)*. Keep it in step with the plan and the changelog.
7. **`## Upstream`** (forks only): accreditation in one or two sentences: the upstream project, linked,
   its authors or organization, and its license. Then **`### Divergences from upstream`**: a bullet list
   of each specific difference (added, removed, changed, and why where it matters), kept current.
8. **`## Documentation`**: links into `docs/`, always including the decision & action log
   (`docs/DECISIONS.md`) and the changelog.
9. Optional, after these: Getting started / Installation, Usage, Development, License (one line linking
   `LICENSE`).

`check` FAILs on a missing title, Goals, Screenshots (or a missing image file), Features (no bullets),
Documentation (no log link), Upstream or Divergences (for forks), or a missing or misplaced progress
block. It warns on placeholder text (`TODO`, `TBD`, lorem ipsum) left in those sections.

## 6. Decision & action log (`docs/DECISIONS.md`)

A dated, append-only history of what the **user** decided and did. It explains *why* the project looks
the way it does, and it's a source of `user` evidence for the POST-MORTEM-STANDARD §4.

Log an entry whenever the user:

| Kind | Example |
|---|---|
| `decision` | Chose an option, set a direction, picked a library, accepted a trade-off |
| `approval` / `rejection` | Approved or turned down a proposed change, design or plan |
| `change` | Changed scope, requirements, priorities or a standard's exception |
| `action` | Did something outside the session: created a repo, set a secret, enabled sharing, merged a PR |
| `asset` | Supplied a file, credential, brand asset, screenshot or content |
| `note` | Stated a constraint or preference that shapes the work |

Rules:
- Add entries with `node scripts/repo-check.mjs log <kind> "<what>" --why "<reason>"` in the same
  commit as the work they affect. Format:
  `- **YYYY-MM-DD** · decision · Use PostgreSQL instead of SQLite — _why:_ multi-user access`.
- One line per entry, in the user's terms, past tense or imperative; include the reason when one was given.
- Log Claude's own choices only when the user explicitly delegated them (`--by claude`).
- Never rewrite or delete past entries. A reversal is a new entry ("Reverted to SQLite: …").
- No secrets, credentials or personal data.

## 7. Forks and upstream accreditation

- **Detection:** `.repo-standard.json` → `fork`, else a git remote named `upstream`, else GitHub's fork
  parent (via `gh`, when available). If a fork isn't detected, ask the user and set `fork` in the config.
  `"fork": false` marks a project that is definitely not a fork.
- **Remote:** keep the original as the `upstream` remote; `origin` is the user's copy.
- **Accreditation** (README §5 step 7) must link the upstream repo and name its license. Keep the
  upstream `LICENSE` and any `NOTICE`; for copyleft licenses (GPL, AGPL, MPL), the fork stays under the
  same license, and the README must say so.
- **Divergences:** review them at each release with
  `git log --oneline upstream/<branch>..HEAD` and `git diff --stat upstream/<branch>...HEAD`, and update
  the bullet list. Each bullet names a concrete difference ("Adds a theme engine", "Removes telemetry"),
  not a commit message.

## 8. Progress block (with the Progress Bar Standard)

When `PROGRESS-BAR-STANDARD.md` is in the project (root or `docs/standards/`), or `progress.config.json`
exists:

- **README block:** `node scripts/repo-check.mjs progress` writes `docs/progress/progress.svg` (a static
  bar with phase markers, readable in light and dark mode) and a block directly under the README title:
  the image linked to the full page, plus one text line, e.g. `**24% complete** · 12 of 49 tasks ·
  Phase 2 — Build · [Full progress page →](<artifactUrl>)`. It uses exactly the plan-parsing and
  percentage rules of the Progress Bar Standard §4, so the two always agree.
- **Keep it current:** every time the Progress Bar Standard §3 says to regenerate and republish, also
  run `node scripts/repo-check.mjs progress` and commit the README and SVG in the same commit.
- **Public by link:** on adopting this standard, the full progress page (the Progress Bar artifact) must
  be readable by anyone with the link, so the README link works for visitors.
  1. Publish it per the Progress Bar Standard §3 if it isn't yet, and store `artifactUrl`.
  2. Claude can't change an artifact's sharing. Ask the user to turn on link sharing for that page
     (view-only, anyone with the link) and to confirm.
  3. On confirmation, set `"artifactPublic": true` in `progress.config.json`, log it
     (`log action "Made the progress page publicly readable by link"`), and run `progress` again.
  4. Republish only to the same URL from then on, so the public link never changes. Sharing persists
     across republishes.
  - `check` FAILs until `artifactPublic` is `true`, and warns while there's no `artifactUrl`.
- The progress page is public, so it must not contain anything private: task text in `docs/PLAN.md`
  must be fit for any visitor to read (no secrets, client names under NDA, or personal data). Check
  this before turning sharing on.

## 9. `.repo-standard.json` (optional)

```json
{
  "defaultBranch": "main",
  "branchPatterns": ["^themeable(-upstream/.+)?$"],
  "fork": { "repo": "rommapp/romm", "url": "https://github.com/rommapp/romm", "license": "AGPL-3.0", "branch": "master" },
  "rootFilesAllowed": ["CITATION.cff"],
  "ignore": ["vendor/", "third_party/"],
  "allowMixedCaseDirs": false,
  "timezone": "America/Los_Angeles"
}
```

- `branchPatterns`: extra regexes for branch names that are legitimately outside the convention (e.g.
  an existing fork's workflow branches). Each exception gets a log entry with the reason.
- `fork`: `null` (auto-detect, default), an object, or `false`.
- `rootFilesAllowed`: extra root documents a tool requires (e.g. `CITATION.cff`, `CHANGELOG.md` for a
  release tool that insists on it).

## 10. `scripts/repo-check.mjs` (copy verbatim)

Node ≥18, no dependencies. Run from the repo root.

```js
#!/usr/bin/env node
// repo-check.mjs — Repo Structure Standard v1. Node >=18, no dependencies. Run from the repo root.
//   node scripts/repo-check.mjs check       pre-work audit: git, branches, names, layout, docs, README (exit 1 on FAIL)
//   node scripts/repo-check.mjs scaffold    create what's missing, never overwriting: git repo, docs/, DECISIONS.md, CHANGELOG, README sections
//   node scripts/repo-check.mjs progress    refresh the README progress block and docs/progress/progress.svg from docs/PLAN.md
//   node scripts/repo-check.mjs log <kind> "<text>" [--why "<reason>"] [--by user|claude]
//        append to docs/DECISIONS.md; kind: decision | action | approval | rejection | change | asset | note
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const VERSION = 'v1';
const ROOT = process.cwd();
const CFG = { defaultBranch: 'main', branchPatterns: [], fork: null, rootFilesAllowed: [], ignore: [], timezone: 'America/Los_Angeles',
  ...readJSON(path.join(ROOT, '.repo-standard.json')) };
const PROG = readJSON(path.join(ROOT, 'progress.config.json'));
const DECISIONS = 'docs/DECISIONS.md';
const LOG_KINDS = ['decision', 'action', 'approval', 'rejection', 'change', 'asset', 'note'];
const START = '<!-- progress:start · generated by scripts/repo-check.mjs progress; do not edit -->';
const END = '<!-- progress:end -->';

function readJSON(f) { try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return null; } }
const exists = (p) => fs.existsSync(path.join(ROOT, p));
const read = (p) => fs.readFileSync(path.join(ROOT, p), 'utf8');
const write = (p, s) => { fs.mkdirSync(path.dirname(path.join(ROOT, p)), { recursive: true }); fs.writeFileSync(path.join(ROOT, p), s); };
const git = (...a) => { const r = spawnSync('git', a, { cwd: ROOT, encoding: 'utf8', maxBuffer: 64 << 20 }); return r.status === 0 ? r.stdout.trim() : null; };
const today = () => new Intl.DateTimeFormat('en-CA', { timeZone: CFG.timezone, year: 'numeric', month: '2-digit', day: '2-digit' }).format(new Date());
const list = (xs, n = 5) => xs.slice(0, n).join(', ') + (xs.length > n ? ` (+${xs.length - n} more)` : '');
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const PLACEHOLDER = /\b(TODO|TBD|FIXME|lorem ipsum)\b|<!--\s*fill/i;

// ---------- standards present in this project (root or docs/standards/) ----------
function standards() {
  const found = {};
  for (const dir of ['.', 'docs/standards']) if (exists(dir)) for (const f of fs.readdirSync(path.join(ROOT, dir))) if (/^[A-Z0-9-]+-STANDARD\.md$/.test(f)) found[f] = path.posix.join(dir, f).replace(/^\.\//, '');
  return found;
}
const hasProgressStandard = () => !!standards()['PROGRESS-BAR-STANDARD.md'] || !!PROG;

// ---------- plan parsing: identical rules to PROGRESS-BAR-STANDARD §4 ----------
const PHASE_RE = /^#{2,3}\s+Phase\s+([\w.]+)\s*(?:[:—–-]+\s*)?(.*)$/i;
const TASK_RE = /^[-*]\s+\[([ xX])\]\s+(.*)$/;
const ID_RE = /^(?:\*\*)?([A-Za-z]?\d+(?:\.\d+)+)\.?(?:\*\*)?[\s:.)—–-]*\s*/;
function planStats() {
  const planPath = (PROG && PROG.plan) || 'docs/PLAN.md';
  if (!exists(planPath)) return null;
  const phases = [];
  for (const raw of read(planPath).split(/\r?\n/)) {
    const ph = raw.match(PHASE_RE);
    if (ph) { phases.push({ n: ph[1], title: ph[2].trim().replace(/\*\*/g, ''), tasks: [] }); continue; }
    const t = raw.match(TASK_RE);
    if (t && phases.length) { const p = phases.at(-1); let text = t[2].trim(); const m = text.match(ID_RE); if (m) text = text.slice(m[0].length); p.tasks.push({ text, done: t[1].toLowerCase() === 'x' }); }
  }
  const live = phases.filter((p) => p.tasks.length);
  if (!live.length) return null;
  const total = live.reduce((a, p) => a + p.tasks.length, 0), done = live.reduce((a, p) => a + p.tasks.filter((t) => t.done).length, 0);
  const pct = done === total ? 100 : Math.floor((done / total) * 100);
  let before = 0; for (const p of live) { p.at = (before / total) * 100; before += p.tasks.length; }
  const active = live.find((p) => p.tasks.some((t) => !t.done));
  return { planPath, live, total, done, pct, exact: (done / total) * 100, active };
}

// ---------- README parsing ----------
function readmeSections(text) {
  const lines = text.split(/\r?\n/); const secs = []; let cur = { level: 0, title: '(preamble)', body: [] }; let fence = false;
  for (const l of lines) {
    if (/^\s*(```|~~~)/.test(l)) fence = !fence;
    const h = !fence && l.match(/^(#{1,3})\s+(.*?)\s*#*\s*$/);
    if (h) { secs.push(cur); cur = { level: h[1].length, title: h[2], body: [] }; } else cur.body.push(l);
  }
  secs.push(cur); return secs;
}
const findSec = (secs, re, level = 2) => secs.find((s) => s.level === level && re.test(s.title));
const bullets = (s) => (s ? s.body.filter((l) => /^\s*[-*+]\s+\S/.test(l)) : []);
const textLines = (s) => (s ? s.body.filter((l) => l.trim() && !/^\s*(<!--|-->)/.test(l)) : []);
const SEC = {
  goals: /^(goals?|project goals|purpose|summary)\b/i,
  shots: /^(screenshots?|mockups?|screenshots?\s*(&|and)\s*mockups?|preview|gallery)\b/i,
  features: /^(features?|feature list)\b/i,
  docs: /^(documentation|docs)\b/i,
  upstream: /^(upstream|credits|acknowledg(e)?ments|attribution|based on)\b/i,
  diverge: /(diverg|differences from upstream|changes from upstream)/i,
};

// ---------- fork detection ----------
function forkInfo({ network = false } = {}) {
  if (CFG.fork === false) return null;
  if (CFG.fork && typeof CFG.fork === 'object') return { ...CFG.fork, source: '.repo-standard.json' };
  const up = git('remote', 'get-url', 'upstream');
  if (up) { const m = up.match(/github\.com[:/]([^/]+\/[^/.]+?)(?:\.git)?$/); return { repo: m ? m[1] : up, url: m ? `https://github.com/${m[1]}` : up, source: 'git remote "upstream"' }; }
  if (network && spawnSync('gh', ['--version']).status === 0 && git('remote', 'get-url', 'origin')) {
    const r = spawnSync('gh', ['repo', 'view', '--json', 'isFork,parent'], { cwd: ROOT, encoding: 'utf8', timeout: 15000 });
    const j = r.status === 0 ? readJSON0(r.stdout) : null;
    if (j && j.isFork && j.parent) { const repo = `${j.parent.owner.login}/${j.parent.name}`; return { repo, url: `https://github.com/${repo}`, source: 'GitHub fork parent' }; }
  }
  return null;
}
function readJSON0(s) { try { return JSON.parse(s); } catch { return null; } }

// ---------- check ----------
function check() {
  const out = []; const add = (level, area, msg) => out.push({ level, area, msg });
  const isRepo = git('rev-parse', '--is-inside-work-tree') === 'true';
  // git
  if (!isRepo) add('fail', 'git', 'not a git repository (scaffold runs `git init -b main`)');
  let files = [];
  if (isRepo) {
    files = (git('ls-files', '--cached', '--others', '--exclude-standard') || '').split('\n').filter(Boolean);
    const branches = (git('for-each-ref', '--format=%(refname:short)', 'refs/heads') || '').split('\n').filter(Boolean);
    const head = git('symbolic-ref', '--short', 'HEAD');
    const originHead = (git('symbolic-ref', '--short', 'refs/remotes/origin/HEAD') || '').replace(/^origin\//, '');
    const def = CFG.defaultBranch;
    const actualDefault = originHead || (branches.includes(def) ? def : branches.includes('master') ? 'master' : branches.length ? branches[0] : head);
    if (actualDefault === def) add('ok', 'git', `default branch is "${def}"`);
    else add(actualDefault === 'master' ? 'warn' : 'fail', 'git', `default branch is "${actualDefault}", expected "${def}" (renaming is disruptive: ask the user first)`);
    const pats = [/^(main|master|develop|dev|staging|production|gh-pages)$/,
      /^(feat|feature|fix|bugfix|hotfix|release|docs|chore|refactor|test|tests|perf|ci|build|style|exp|experiment|deps|revert)\/[a-z0-9][a-z0-9._-]*(\/[a-z0-9._-]+)*$/,
      /^(claude|dependabot|renovate)\/.+$/, ...CFG.branchPatterns.map((p) => new RegExp(p))];
    const bad = branches.filter((b) => !pats.some((re) => re.test(b)));
    add(bad.length ? 'warn' : 'ok', 'git', bad.length ? `branch names not in <type>/<kebab-case> form: ${list(bad)}` : `${branches.length} branch name(s) follow <type>/<kebab-case>`);
    const subjects = (git('log', '-n', '50', '--pretty=%s') || '').split('\n').filter(Boolean);
    if (subjects.length) {
      const cc = /^(feat|fix|docs|style|refactor|perf|test|tests|build|ci|chore|revert)(\([\w./ -]+\))?!?: \S/;
      const okN = subjects.filter((s) => cc.test(s) || /^(Merge |Revert ")/.test(s)).length;
      const ratio = okN / subjects.length;
      add(ratio >= 0.8 ? 'ok' : 'warn', 'git', `${Math.round(ratio * 100)}% of the last ${subjects.length} commits use Conventional Commits${ratio < 0.8 ? ` (e.g. "${subjects.find((s) => !cc.test(s))}")` : ''}`);
    } else add('info', 'git', 'no commits yet');
    const tags = (git('tag', '--list') || '').split('\n').filter(Boolean).filter((t) => !/^v?\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/.test(t));
    if (tags.length) add('warn', 'git', `tags not in SemVer form (vX.Y.Z): ${list(tags)}`);
    const tracked = (git('ls-files', '--cached') || '').split('\n').filter(Boolean);
    const secrets = tracked.filter((f) => /(^|\/)\.env(\.[\w-]+)?$/.test(f) && !/\.(example|sample|template|dist)$/.test(f) || /(^|\/)(id_rsa|id_ed25519|credentials\.json|.*\.(pem|key|p12|pfx|keystore))$/.test(f));
    add(secrets.length ? 'fail' : 'ok', 'git', secrets.length ? `secret-looking files are tracked: ${list(secrets)} (remove from git, rotate the secret)` : 'no secret-looking files tracked');
    const big = tracked.filter((f) => { try { return fs.statSync(path.join(ROOT, f)).size > 20 * 1024 * 1024; } catch { return false; } });
    if (big.length) add('warn', 'git', `files over 20 MB tracked (use Git LFS or release assets): ${list(big)}`);
    const dirty = (git('status', '--porcelain') || '').split('\n').filter(Boolean).length;
    add('info', 'git', `${git('remote', 'get-url', 'origin') ? 'remote origin set' : 'no remote origin'} · ${dirty} uncommitted change(s)`);
  } else files = walk('.');
  add(exists('.gitignore') ? 'ok' : 'fail', 'git', exists('.gitignore') ? '.gitignore present' : '.gitignore missing');
  if (!exists('.gitattributes')) add('warn', 'git', '.gitattributes missing (line-ending policy)');
  // names
  const ign = (f) => CFG.ignore.some((p) => f.startsWith(p)) || /(^|\/)(node_modules|vendor|third_party|\.git)\//.test(f);
  const fl = files.filter((f) => !ign(f));
  const spaced = fl.filter((f) => /\s/.test(f));
  add(spaced.length ? 'fail' : 'ok', 'names', spaced.length ? `paths with spaces: ${list(spaced)}` : 'no spaces in file or folder names');
  const mixedOk = CFG.allowMixedCaseDirs || fl.some((f) => /\.(csproj|fsproj|vbproj|sln|xcodeproj|vcxproj)(\/|$)/.test(f) || /(^|\/)Package\.swift$/.test(f));
  if (!mixedOk) {
    const dirs = [...new Set(fl.flatMap((f) => f.split('/').slice(0, -1)))].filter((d) => /[A-Z]/.test(d) && !/^(__[A-Za-z]+__|\.github|LICENSES)$/.test(d));
    if (dirs.length) add('warn', 'names', `folder names not lowercase: ${list(dirs)}`);
  }
  const base = path.basename(ROOT);
  if (!/^[a-z0-9]+([._-][a-z0-9]+)*$/.test(base)) add('warn', 'names', `repo folder "${base}" isn't lowercase kebab-case (rename only with the user's OK)`);
  // layout
  const rootAllowed = /^(README\.md|LICENSE(\.\w+)?|LICENCE(\.\w+)?|COPYING(\.\w+)?|NOTICE(\.\w+)?|CLAUDE\.md|AGENTS\.md|[A-Z0-9-]+-STANDARD\.md)$/;
  const rootDocs = fs.readdirSync(ROOT).filter((f) => fs.statSync(path.join(ROOT, f)).isFile() && !rootAllowed.test(f) && !CFG.rootFilesAllowed.includes(f)
    && (/\.(md|markdown|rst|adoc|pdf|docx?|odt|rtf)$/i.test(f) || /^(notes|todo|changelog|history|contributing|install|guide|manual|spec|design|authors).*\.txt$/i.test(f)));
  add(rootDocs.length ? 'fail' : 'ok', 'layout', rootDocs.length ? `auxiliary docs at the root belong in docs/: ${list(rootDocs, 8)}` : 'root holds only README, license and tool-required files');
  add(exists('docs') ? 'ok' : 'fail', 'layout', exists('docs') ? 'docs/ present' : 'docs/ missing');
  const st = standards(); const rootStd = Object.entries(st).filter(([, p]) => !p.startsWith('docs/')).map(([f]) => f);
  if (rootStd.length) add('warn', 'layout', `standards at the root: ${list(rootStd)} → move to docs/standards/ and list them in CLAUDE.md`);
  const docStd = Object.entries(st).filter(([, p]) => p.startsWith('docs/')).map(([f]) => f);
  if (docStd.length) { const cm = exists('CLAUDE.md') ? read('CLAUDE.md') : ''; const miss = docStd.filter((f) => !cm.includes(f)); if (miss.length) add('fail', 'layout', `CLAUDE.md doesn't list ${list(miss)} (Claude won't know they apply)`); }
  if (!fs.readdirSync(ROOT).some((f) => /^(LICENSE|LICENCE|COPYING)/.test(f))) add('warn', 'layout', 'no LICENSE file at the root');
  add(exists('docs/CHANGELOG.md') ? 'ok' : 'warn', 'docs', exists('docs/CHANGELOG.md') ? 'docs/CHANGELOG.md present' : 'docs/CHANGELOG.md missing (Keep a Changelog format)');
  if (!exists(DECISIONS)) add('fail', 'docs', `${DECISIONS} missing (decision & action log)`);
  else { const n = read(DECISIONS).split('\n').filter((l) => /^- \*\*\d{4}-\d{2}-\d{2}\*\*/.test(l)).length; add(n ? 'ok' : 'warn', 'docs', `${DECISIONS}: ${n} entr${n === 1 ? 'y' : 'ies'}`); }
  // README
  if (!exists('README.md')) add('fail', 'readme', 'README.md missing');
  else {
    const md = read('README.md'); const secs = readmeSections(md);
    const h1 = secs.find((s) => s.level === 1);
    add(h1 && secs.indexOf(h1) === 1 ? 'ok' : 'fail', 'readme', h1 && secs.indexOf(h1) === 1 ? `title: "${h1.title}"` : 'README must start with a single "# Title"');
    const intro = h1 ? h1.body.join('\n').replace(/<!-- progress:start[\s\S]*?<!-- progress:end -->/, '').split('\n').filter((l) => l.trim() && !/^\s*(\[!\[|!\[|<a |<img |<!--)/.test(l)) : [];
    add(intro.length ? 'ok' : 'warn', 'readme', intro.length ? 'one-paragraph summary under the title' : 'add a one-paragraph summary under the title');
    const req = (key, label, test) => { const s = findSec(secs, SEC[key]); if (!s) { add('fail', 'readme', `missing "## ${label}" section`); return null; } const r = test(s); add(r === true ? 'ok' : 'fail', 'readme', r === true ? `${label}: ok` : `${label}: ${r}`); if (textLines(s).some((l) => PLACEHOLDER.test(l))) add('warn', 'readme', `${label}: still has placeholder text`); return s; };
    req('goals', 'Goals', (s) => textLines(s).length ? true : 'empty');
    req('shots', 'Screenshots', (s) => {
      const imgs = [...s.body.join('\n').matchAll(/!\[[^\]]*\]\(([^)\s]+)[^)]*\)|<img[^>]+src="([^"]+)"/g)].map((m) => m[1] || m[2]);
      if (!imgs.length) return 'no screenshot or mockup image';
      const local = imgs.filter((u) => !/^https?:/.test(u));
      const missing = local.filter((u) => !exists(decodeURI(u.split('#')[0])));
      if (missing.length) return `image file(s) not found: ${list(missing)}`;
      const outside = local.filter((u) => !u.startsWith('docs/'));
      if (outside.length) add('warn', 'readme', `screenshots should live under docs/images/: ${list(outside)}`);
      return true;
    });
    req('features', 'Features', (s) => bullets(s).length ? true : 'needs a bullet-point list');
    req('docs', 'Documentation', (s) => s.body.join('\n').includes(DECISIONS) ? true : `must link ${DECISIONS}`);
    const fork = forkInfo({ network: true });
    if (fork) {
      const up = findSec(secs, SEC.upstream);
      if (!up) add('fail', 'readme', `fork of ${fork.repo} (${fork.source}) but no "## Upstream" accreditation section`);
      else {
        const body = up.body.join('\n');
        add(body.includes(fork.repo) || (fork.url && body.includes(fork.url)) ? 'ok' : 'fail', 'readme', body.includes(fork.repo) || (fork.url && body.includes(fork.url)) ? `Upstream: credits ${fork.repo}` : `Upstream: must link ${fork.url || fork.repo}`);
        if (!/licen[cs]e/i.test(body)) add('warn', 'readme', 'Upstream: state the upstream license');
        const dv = secs.find((s) => s.level === 3 && SEC.diverge.test(s.title) && secs.indexOf(s) > secs.indexOf(up));
        add(dv && bullets(dv).length ? 'ok' : 'fail', 'readme', dv && bullets(dv).length ? `Divergences: ${bullets(dv).length} listed` : 'Upstream: needs "### Divergences from upstream" with a bullet list');
      }
    } else add('info', 'readme', 'not detected as a fork (set "fork" in .repo-standard.json if it is one)');
    if (hasProgressStandard()) {
      const s = md.indexOf('<!-- progress:start'), e = md.indexOf(END);
      const firstH2 = md.search(/^##\s/m);
      if (s < 0 || e < 0) add('fail', 'readme', 'Progress Bar Standard present but README has no progress block (run `node scripts/repo-check.mjs progress`)');
      else {
        add(firstH2 < 0 || s < firstH2 ? 'ok' : 'fail', 'readme', firstH2 < 0 || s < firstH2 ? 'progress block at the top' : 'progress block must sit directly under the title');
        const stats = planStats();
        if (stats && !md.slice(s, e).includes(blockLine(stats))) add('warn', 'readme', 'progress block is stale (run `node scripts/repo-check.mjs progress`)');
        if (!exists('docs/progress/progress.svg')) add('fail', 'readme', 'docs/progress/progress.svg missing');
      }
      const url = PROG && PROG.artifactUrl;
      if (!url) add('warn', 'readme', 'progress page not published yet: progress.config.json has no artifactUrl (Progress Bar Standard §3)');
      else if (PROG.artifactPublic !== true) add('fail', 'readme', 'progress page isn\'t confirmed public: turn on link sharing for it, then set "artifactPublic": true in progress.config.json');
      else add('ok', 'readme', 'progress page published and public');
    }
  }
  return out;
}
function walk(dir, out = []) {
  for (const e of fs.readdirSync(path.join(ROOT, dir), { withFileTypes: true })) {
    if (['.git', 'node_modules'].includes(e.name)) continue;
    const p = path.posix.join(dir, e.name).replace(/^\.\//, '');
    if (e.isDirectory()) walk(p, out); else out.push(p);
  }
  return out;
}

// ---------- progress block ----------
const blockLine = (st) => `**${st.pct}% complete** · ${st.done} of ${st.total} tasks · ${st.active ? `Phase ${st.active.n} — ${st.active.title}` : 'All phases complete'}`;
function svg(st) {
  const L = { brand: '#700908', 'brand-soft': '#e2dcc6', done: '#2f6b3a', fg: '#1d1a18', muted: '#6d655e', ...((PROG && PROG.theme && PROG.theme.light) || {}) };
  const D = { brand: '#d4574f', 'brand-soft': '#4a3a2f', done: '#6fbf7c', fg: '#f1ebe2', muted: '#a99f94', ...((PROG && PROG.theme && PROG.theme.dark) || {}) };
  const W = 720, x0 = 0, bw = 720, y = 46, h = 12; const fillW = Math.round((st.exact / 100) * bw * 10) / 10;
  const ticks = st.live.map((p) => `<line class="tick" x1="${x0 + Math.round((p.at / 100) * bw)}" y1="${y - 4}" x2="${x0 + Math.round((p.at / 100) * bw)}" y2="${y + h + 4}"/><text class="ph" x="${Math.min(x0 + Math.round((p.at / 100) * bw) + (p.at === 0 ? 0 : -1), W - 24)}" y="${y + h + 18}" text-anchor="${p.at === 0 ? 'start' : 'middle'}">P${esc(p.n)}</text>`).join('');
  const label = st.active ? `Phase ${st.active.n} — ${st.active.title}` : 'All phases complete';
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="84" viewBox="0 0 ${W} 84" role="img" aria-label="${esc(`Progress: ${st.pct}% (${st.done} of ${st.total} tasks), ${label}`)}">
<style>
.pct{font:700 26px Georgia,serif;fill:${L.brand}}.sub{font:13px -apple-system,Segoe UI,Helvetica,Arial,sans-serif;fill:${L.muted}}
.track{fill:${L['brand-soft']}}.fill{fill:${st.pct === 100 ? L.done : L.brand}}.tick{stroke:${L.muted};stroke-width:1}.ph{font:10px ui-monospace,Menlo,Consolas,monospace;fill:${L.muted}}
@media (prefers-color-scheme:dark){.pct{fill:${D.brand}}.sub,.ph{fill:${D.muted}}.track{fill:${D['brand-soft']}}.fill{fill:${st.pct === 100 ? D.done : D.brand}}.tick{stroke:${D.muted}}}
</style>
<text class="pct" x="0" y="28">${st.pct}%</text><text class="sub" x="${st.pct === 100 ? 74 : st.pct >= 10 ? 62 : 46}" y="27">${esc(`${st.done} of ${st.total} tasks · ${label.length > 70 ? label.slice(0, 69) + '…' : label}`)}</text>
<rect class="track" x="${x0}" y="${y}" width="${bw}" height="${h}" rx="6"/><rect class="fill" x="${x0}" y="${y}" width="${Math.max(fillW, st.done ? 6 : 0)}" height="${h}" rx="6"/>${ticks}
</svg>
`;
}
function cmdProgress() {
  const st = planStats();
  if (!st) { console.error('progress: no docs/PLAN.md with "## Phase N — Title" sections and "- [ ]" tasks (Progress Bar Standard §2)'); process.exit(1); }
  if (!exists('README.md')) { console.error('progress: README.md missing (run scaffold first)'); process.exit(1); }
  write('docs/progress/progress.svg', svg(st));
  const url = (PROG && PROG.artifactUrl) || 'docs/progress/index.html';
  const block = `${START}\n<a href="${url}"><img src="docs/progress/progress.svg" alt="${esc(`Progress: ${st.pct}% (${st.done} of ${st.total} tasks)`)}" width="720"></a>\n\n${blockLine(st)} · [Full progress page →](${url})\n${END}`;
  let md = read('README.md');
  const s = md.indexOf('<!-- progress:start'), e = md.indexOf(END);
  if (s >= 0 && e > s) md = md.slice(0, s) + block + md.slice(e + END.length);
  else { const m = md.match(/^# .*$/m); md = m ? md.slice(0, m.index + m[0].length) + '\n\n' + block + '\n' + md.slice(m.index + m[0].length) : block + '\n\n' + md; }
  write('README.md', md.replace(/\n{3,}/g, '\n\n'));
  console.log(`progress: ${st.pct}% (${st.done}/${st.total}) · ${st.active ? 'phase ' + st.active.n : 'all done'} · README block + docs/progress/progress.svg updated${PROG && PROG.artifactUrl ? '' : ' (linked locally: no artifactUrl yet)'}`);
}

// ---------- scaffold ----------
const DECISIONS_HEAD = `# Decision & action log\n\nUser decisions and actions on this project, oldest first. Maintained by Claude per REPO-STRUCTURE-STANDARD;\nadd entries with \`node scripts/repo-check.mjs log <kind> "<text>" --why "<reason>"\`.\n\n`;
function logEntry(kind, text, why, by) { return `- **${today()}** · ${kind} · ${text.replace(/\s+/g, ' ').trim()}${why ? ` — _why:_ ${why.replace(/\s+/g, ' ').trim()}` : ''}${by && by !== 'user' ? ` _(by ${by})_` : ''}\n`; }
function cmdScaffold() {
  const did = [];
  if (git('rev-parse', '--is-inside-work-tree') !== 'true') { spawnSync('git', ['init', '-b', CFG.defaultBranch], { cwd: ROOT }); did.push(`git init -b ${CFG.defaultBranch}`); }
  for (const d of ['docs', 'docs/images']) if (!exists(d)) { fs.mkdirSync(path.join(ROOT, d), { recursive: true }); did.push(d + '/'); }
  if (!exists('docs/images/.gitkeep') && !fs.readdirSync(path.join(ROOT, 'docs/images')).length) { write('docs/images/.gitkeep', ''); }
  if (!exists(DECISIONS)) { write(DECISIONS, DECISIONS_HEAD + logEntry('action', `Adopted REPO-STRUCTURE-STANDARD ${VERSION}`, 'standard included in the project', 'user')); did.push(DECISIONS); }
  if (!exists('docs/CHANGELOG.md')) { write('docs/CHANGELOG.md', '# Changelog\n\nAll notable changes to this project are documented here. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).\n\n## [Unreleased]\n\n### Added\n\n- Project structure per REPO-STRUCTURE-STANDARD.\n'); did.push('docs/CHANGELOG.md'); }
  if (!exists('.gitattributes')) { write('.gitattributes', '* text=auto eol=lf\n*.png binary\n*.jpg binary\n*.gif binary\n*.webp binary\n*.pdf binary\n'); did.push('.gitattributes'); }
  if (!exists('.gitignore')) { write('.gitignore', '# Secrets\n.env\n.env.*\n!.env.example\n\n# Dependencies and build output\nnode_modules/\ndist/\nbuild/\n__pycache__/\n\n# OS / editor\n.DS_Store\nThumbs.db\n*.log\n\n# Claude local settings\n.claude/settings.local.json\n.claude/*.local.json\n'); did.push('.gitignore'); }
  const fork = forkInfo({ network: true });
  const name = path.basename(ROOT);
  const parts = {
    goals: '## Goals\n\n- TODO: what this project is for and what "done" looks like.\n',
    shots: '## Screenshots\n\nTODO: add a mockup (before release) or screenshot (after) under `docs/images/` and embed it here.\n',
    features: '## Features\n\n- TODO: one bullet per user-visible feature.\n',
    upstream: fork ? `## Upstream\n\nBased on [${fork.repo}](${fork.url}) by its authors, licensed under TODO-LICENSE. Thanks to the upstream maintainers.\n\n### Divergences from upstream\n\n- TODO: one bullet per specific difference from upstream.\n` : '',
    docs: `## Documentation\n\n- [Decision & action log](${DECISIONS})\n- [Changelog](docs/CHANGELOG.md)\n`,
  };
  if (!exists('README.md')) { write('README.md', `# ${name}\n\nTODO: one-paragraph summary of what this is and who it's for.\n\n${Object.values(parts).filter(Boolean).join('\n')}`); did.push('README.md (skeleton)'); }
  else {
    const secs = readmeSections(read('README.md')); const add = [];
    for (const [k, txt] of Object.entries(parts)) if (txt && !findSec(secs, SEC[k])) add.push(txt);
    if (add.length) { const md = read('README.md'); write('README.md', md.replace(/\s*$/, '\n\n') + add.join('\n')); did.push(`README.md (+${add.length} section${add.length > 1 ? 's' : ''})`); }
  }
  const rootAllowed = /^(README\.md|LICENSE(\.\w+)?|LICENCE(\.\w+)?|COPYING(\.\w+)?|NOTICE(\.\w+)?|CLAUDE\.md|AGENTS\.md)$/;
  const moves = fs.readdirSync(ROOT).filter((f) => fs.statSync(path.join(ROOT, f)).isFile() && !rootAllowed.test(f) && !CFG.rootFilesAllowed.includes(f) && /\.(md|markdown|rst|adoc|pdf|docx?|odt|rtf)$/i.test(f))
    .map((f) => /-STANDARD\.md$/.test(f) ? `git mv ${f} docs/standards/${f}` : `git mv ${f} docs/${f}`);
  console.log(did.length ? `scaffold: created ${did.join(', ')}` : 'scaffold: nothing to create');
  if (moves.length) { console.log('scaffold: suggested moves (run after the user agrees; update links afterwards):'); moves.forEach((m) => console.log('  ' + m)); }
  if (hasProgressStandard()) console.log('scaffold: Progress Bar Standard present: run `node scripts/repo-check.mjs progress` once docs/PLAN.md has tasks');
}

function cmdLog(args) {
  const kind = args[0]; const text = args[1];
  const opt = (f) => { const i = args.indexOf(f); return i >= 0 ? args[i + 1] : undefined; };
  if (!LOG_KINDS.includes(kind) || !text) { console.error(`log: usage: log <${LOG_KINDS.join('|')}> "<text>" [--why "<reason>"] [--by user|claude]`); process.exit(2); }
  if (!exists(DECISIONS)) write(DECISIONS, DECISIONS_HEAD);
  const cur = read(DECISIONS);
  write(DECISIONS, cur.replace(/\s*$/, '\n') + logEntry(kind, text, opt('--why'), opt('--by') || 'user'));
  console.log(`log: ${kind} recorded in ${DECISIONS}`);
}

function cmdCheck() {
  const out = check(); const mark = { ok: 'ok  ', fail: 'FAIL', warn: 'warn', info: 'info' };
  console.log(`repo-check (${path.basename(ROOT)}) · REPO-STRUCTURE-STANDARD ${VERSION}`);
  for (const r of out) if (r.level !== 'ok' || process.argv.includes('--all')) console.log(`  ${mark[r.level]} [${r.area}] ${r.msg}`);
  const f = out.filter((r) => r.level === 'fail').length, w = out.filter((r) => r.level === 'warn').length, o = out.filter((r) => r.level === 'ok').length;
  console.log(`check: ${f ? 'NOT COMPLIANT' : 'COMPLIANT'} · ${o} ok · ${f} fail · ${w} warn${process.argv.includes('--all') ? '' : ' (ok lines hidden; --all shows them)'}`);
  process.exit(f ? 1 : 0);
}

const [cmd, ...rest] = process.argv.slice(2);
if (cmd === 'check') cmdCheck();
else if (cmd === 'scaffold') cmdScaffold();
else if (cmd === 'progress') cmdProgress();
else if (cmd === 'log') cmdLog(rest);
else { console.log(fs.readFileSync(new URL(import.meta.url), 'utf8').split('\n').slice(1, 7).join('\n')); process.exit(2); }
```

| Command | Does | Output |
|---|---|---|
| `check` | Audits git, branches, commits, tags, secrets, names, layout, docs, README and progress block | `FAIL` / `warn` / `info` lines (`--all` adds `ok`), then `COMPLIANT` or `NOT COMPLIANT` (exit 1) |
| `scaffold` | Creates what's missing, never overwrites; suggests `git mv` commands | What it created, plus suggested moves |
| `progress` | Writes `docs/progress/progress.svg` and the README block from `docs/PLAN.md` | One summary line |
| `log <kind> "<text>" [--why …] [--by …]` | Appends a dated entry to `docs/DECISIONS.md` | One line |

## 11. Anti-patterns (never)

- Starting feature work before the pre-work process has run.
- Renaming branches, folders or the repo, moving documents, or rewriting history without the user's OK.
- Leaving `TODO` placeholders in the README instead of writing the content or making a mockup.
- Mockups presented as real screenshots, or screenshots that are out of date at release.
- Editing the generated progress block or SVG by hand, or hard-coding progress numbers anywhere.
- Linking the README to a progress page that isn't public, or publishing a second progress artifact
  instead of republishing to the same URL.
- Rewriting or deleting decision-log entries, or logging Claude's own choices as the user's.
- A fork README that doesn't credit upstream, omits its license, or lists commit messages as "divergences".
- Committing secrets, or "fixing" a committed secret without telling the user to rotate it.
- Rewriting `repo-check.mjs` per project; fixes go back into this standard with a version bump.
