# PII Redaction Standard (v1)

> **Instruction to Claude:** If this file is in a project, nothing in the project may identify the user
> or their test environment: no personal names, postal or email addresses, phone numbers, usernames or
> handles, IP or network details, or machine details. On first use, run the setup in §3 (identity file,
> public identity, scan, fix, hooks, history check). From then on, write placeholders from the start
> (§5), scan everything before it leaves the machine (§4), and apply the overrides in §6 to what other
> standards generate. Nobody has to ask again.

---

## 1. What it is

A standing rule plus a scanner that keeps personal and environment-identifying information out of every
file, commit, message and published page of the project.

**What counts** (about the user, people in their life, and the machines and networks they test on):

| Category | Examples of what gets scrubbed |
|---|---|
| Names | Real name and its variants, family or client names, the name in `LICENSE` or `package.json` "author" |
| Addresses | Postal addresses, email addresses, phone numbers |
| Usernames | OS login names, home-directory paths (`/home/<you>`, `C:\Users\<you>`), GitHub/forum handles not chosen as public, `user@host` prompts |
| Network | Public and private IPv4/IPv6 addresses and subnets, MAC addresses, gateway and DNS addresses, home-lab domains (`*.lan`, `*.local`, `*.internal`), Wi-Fi SSIDs, VPN endpoints |
| Machine | Hostnames, machine IDs, serial numbers, CPU model, kernel build, disk/volume and mount names (`/mnt/<drive>`), VM host details, hardware UUIDs |
| Images and files | EXIF/PNG metadata, and anything visible in screenshots (terminal prompts, title bars, browser tabs, desktop names) |

Public identity is the exception: the handle and no-reply email the user chooses to publish under (§3)
are allowed everywhere.

| Piece | Path | Role |
|---|---|---|
| Scanner | `scripts/pii-scan.mjs` | Copied **verbatim** from §10. Finds, replaces, strips metadata, checks history, installs hooks. |
| Identity file | `~/.config/pii-redaction/identity.json` | The real values to hunt for. **Outside every repo**, mode 600, never committed, never published. |
| Project config | `.pii-redaction.json` | Public identity, allow-list, skips (§9). Contains only public values; committed. |
| Git hooks | `.git/hooks/pre-commit`, `commit-msg` | Block commits whose files or message contain PII. Local, installed by `install-hooks`. |

The scanner reads files and git history and executes no project code, so it runs on the host under the
VM Lab Standard §1.

## 2. How detection works

`pii-scan.mjs` combines three sources:

1. **The identity file:** values the user listed (names, addresses, phones, handles, SSIDs, domains,
   serials…), matched literally, whole-word and (for names, emails, hosts and handles) case-insensitively.
2. **Live discovery on every run:** login name, hostname, home directory, every non-loopback IP and MAC
   address, machine ID, CPU model, kernel release, `/mnt`·`/media` mount points, and the global git
   name and email. So even before `init`, the machine Claude is running on can't leak.
3. **Patterns:** any email address, IPv4/IPv6 address, MAC address, home-directory path, North
   American phone number, plus *warnings* for street-address shapes and `.lan`/`.local`/`.internal`
   hostnames.

Excluded automatically: documentation and reserved values (192.0.2.0/24, 198.51.100.0/24,
203.0.113.0/24, 2001:db8::/32, 00:00:5e:00:53:xx, 555-01xx numbers, `example.com/.net/.org`),
loopback, `0.0.0.0`, netmasks, no-reply and GitHub no-reply addresses, generic usernames (`user`, `root`,
`runner`…), the public identity and allow-list, lines containing `pii-scan: allow`, the ai-assets
`*-STANDARD.md` files and the scanner itself. Output shows values **masked** (`d******p`), so a scan
never repeats the PII into the conversation or a log.

## 3. Setup (once per machine, then once per project)

1. Copy §10 to `scripts/pii-scan.mjs` verbatim. Don't rewrite or "improve" it per project.
2. Run `node scripts/pii-scan.mjs init`. It writes discovered values into the identity file (outside the
   repo) and prints counts only.
3. **Ask the user once** for what discovery can't see, and add their answers to the identity file's
   lists (`names`, `emails`, `phones`, `addresses`, `usernames`, `hostnames`, `domains`, `ips`, `macs`,
   `paths`, `machine`, `other`). This covers name variants and nicknames, other email addresses, phone
   numbers, street address, handles used elsewhere, home-lab domain and hostnames of other machines and
   VMs, public IP or DDNS name, Wi-Fi SSIDs, NAS and drive names, and serial numbers. Write them only to
   that file. Never echo them back in full; refer to them masked.
4. **Agree a public identity** and record it in `.pii-redaction.json` → `publicIdentity`: a handle
   (e.g. the GitHub username, if the user is happy for it to appear) and a GitHub no-reply email
   (`<id>+<handle>@users.noreply.github.com`, from GitHub → Settings → Emails). Then set it **for this
   repo**: `git config user.name "<handle>"` and `git config user.email "<noreply>"`. `LICENSE`
   copyright, `package.json` "author" and similar fields use the public identity.
5. `node scripts/pii-scan.mjs install-hooks`. Existing hooks are kept and still run.
6. `node scripts/pii-scan.mjs scan`, then `node scripts/pii-scan.mjs fix` (dry run). Review what it
   will change, then `fix --apply`. Re-scan until `CLEAN`.
7. Handle the warnings by hand (§8): look at every image (open it and view it), review PDFs and other
   binary documents, and check flagged street addresses and `.local`/`.lan` names.
8. `node scripts/pii-scan.mjs history`. If history contains PII, follow §7: report it, and rewrite only
   with the user's approval.
9. If the Repo Structure Standard is present, log the decisions (public identity chosen, history
   rewritten or not) in `docs/DECISIONS.md` without the real values.

## 4. Every time something leaves the machine

The hooks guard commits. Everything else is Claude's job:

| Before… | Do |
|---|---|
| Committing | The hooks run `scan --staged` and `scan --message`. Never bypass them (`--no-verify`) except with the user's explicit OK for a specific commit, and log why. |
| Pushing, tagging, releasing | `node scripts/pii-scan.mjs scan` on the whole tree; release notes and tag messages contain no PII. |
| Publishing an artifact, PDF or report | `node scripts/pii-scan.mjs scan <file>` on the exact file being published; fix and regenerate first. |
| Writing a PR, issue, review comment or discussion post | Write with placeholders; check the text against the categories in §1. |
| Sending a notification (Phone Alerts) | No PII in the title or message; lock screens show them. |
| Attaching logs or command output anywhere | Save to a file, `fix --apply` it, then attach. |

## 5. Write placeholders from the start

Use these in code, docs, tests, fixtures, examples and screenshots, so there's nothing to scrub later.
`fix` uses the same values.

| Real value | Placeholder |
|---|---|
| Person's name | the public handle, or `Project Maintainer` |
| Email | the public no-reply address, or `maintainer@example.com` |
| Phone | `555-0100` (555-0100 to 555-0199 are fictional) |
| Postal address | `123 Example St, Anytown` |
| Username / home path | `user`, `/home/user`, `C:\Users\user`, `~` |
| Hostname / machine | `lab-host`; VMs keep their generic `lab-<project>-<role>` names |
| Domain | `example.net`; home-lab names as `nas.example.net` |
| IPv4 | 192.0.2.x, 198.51.100.x, 203.0.113.x (RFC 5737); keep the prefix length (`192.0.2.0/24`) |
| IPv6 | `2001:db8::…` (RFC 3849) |
| MAC | `00:00:5e:00:53:xx` (RFC 7042) |
| Paths on the user's drives | `/path/to/…`, or relative paths |
| Serial, machine ID, CPU, kernel | `[redacted-machine-info]`, or leave the field out |
| SSID, other | `[redacted]` |

`fix` maps each real IP or MAC address to the **same** documentation address every time, so diagrams and
configs stay consistent after redaction.

Test data and fixtures are synthetic: generated names (`Alex Example`), documentation IPs, fake
addresses. Never copy real data from the user's machine, accounts or network into tests.

## 6. Overrides for other standards' outputs

When this standard is present, it wins over anything another standard would write:

| Standard | What it would expose | What to do instead |
|---|---|---|
| VM Lab Standard §9 (`docs/LAB.md`) | Host CPUs/RAM, libvirt version, network ranges, `doctor` output, guest IPs | Record generic facts only ("host meets requirements", `192.0.2.0/24`). Put real values, if needed at all, in git-ignored `docs/LAB.local.md`. Redact pasted `doctor`, `virsh` or `ip` output. |
| VM Lab guests | Guest hostnames, cloud-init users | Keep the generic `lab-*` names and `lab` user (already generic). |
| Post-Mortem Standard §4 | `metrics.json` paths and transcript folders, evidence quotes, the PDF | Run `fix --apply` on `docs/postmortem/` after `mine` and before `report`; `scan` the PDF's `report.html` before delivering. Quote evidence with placeholders. |
| Progress Bar / Repo Structure Standard §8 | The public progress page and its task text; README; screenshots; decision log | Task text in `docs/PLAN.md` uses placeholders; scan `docs/progress/index.html` before every republish; screenshots and mockups use §5 values; decision-log entries never contain real values. |
| Phone Alerts Standard §6 | Backup-channel URL and token | Stay in git-ignored `.claude/phone-alerts.local.json`; notification text has no PII. |
| Any standard writing `CLAUDE.md` or docs | Absolute paths from the user's machine | Use repo-relative paths. |

## 7. Git history and published copies

- `node scripts/pii-scan.mjs history` scans every commit on every branch: authors and committers (your
  own names and emails only; other contributors and bots are left alone), messages, and added lines.
- If it finds anything: tell the user what kinds were found and where (masked), and that removing it
  means **rewriting history**: new commit IDs, a force-push, and everyone else re-cloning. Ask before
  doing it.
- On approval: `history --write-rewrite-files` writes `replacements.txt` and `mailmap` to the private
  config folder (never the repo). Then, in a **fresh clone**:
  `git filter-repo --replace-text …/replacements.txt --replace-message …/replacements.txt --mailmap …/mailmap`
  (install with `pip install git-filter-repo`). Re-run `history` in the clone until it's `CLEAN`, then
  force-push only after the user confirms, and log the decision.
- Be honest about limits: forks, clones, caches, PR pages and search indexes may keep old copies. For
  GitHub, the user can ask GitHub Support to purge cached views. Anything that was a secret (tokens,
  passwords) must be rotated; redaction doesn't un-leak it.

## 8. What the scanner can't see (Claude checks by hand)

- **Image content.** Open and look at every screenshot or mockup before committing (view the image
  itself). Look for prompts (`user@host`), title bars, file paths, IPs, browser tabs and bookmarks,
  usernames, desktop and dock items, faces and addresses. Retake the screenshot with generic data, crop
  it, or use a mockup. Set a generic prompt (`PS1='$ '`) and hostname view before capturing terminals.
- **PDFs, office documents and archives.** Review their text and properties (author, company), and
  regenerate from clean sources.
- **Values nobody listed.** A coworker's name, a client's address, a nickname. If Claude meets one,
  replace it and ask whether to add it to the identity file.
- **Indirect identifiers.** Unique combinations (exact hardware model, employer and town, timezone in a
  public log) can identify someone even with every listed value scrubbed. Prefer leaving such details
  out entirely.

## 9. `.pii-redaction.json` (committed; public values only)

```json
{
  "publicIdentity": {
    "name": "your-handle",
    "handle": "your-handle",
    "email": "1234567+your-handle@users.noreply.github.com"
  },
  "allow": ["Example Corp"],
  "skip": ["vendor/**", "third_party/**", "test/fixtures/upstream/**"]
}
```

- `allow`: strings that look like PII but are fine to publish (a company's public name, a public
  maintainer of an upstream project).
- `skip`: glob paths to leave unscanned (vendored code). Prefer a `pii-scan: allow` comment on a single
  line over skipping a whole file.
- Never put the user's real values here; they belong in the identity file.

## 10. `scripts/pii-scan.mjs` (copy verbatim)

Node ≥18, no dependencies. Run from the repo root. The private identity folder honors
`XDG_CONFIG_HOME`.

```js
#!/usr/bin/env node
// pii-scan.mjs — PII Redaction Standard v1. Node >=18, no dependencies. Run from the repo root.
//   node scripts/pii-scan.mjs init                      discover this machine's identifying values into the private identity file (outside the repo)
//   node scripts/pii-scan.mjs scan [paths…]             scan the working tree (or given files) for PII; exit 1 on findings
//   node scripts/pii-scan.mjs scan --staged             scan what is staged for commit (used by the pre-commit hook)
//   node scripts/pii-scan.mjs scan --message <file>     scan a commit message (used by the commit-msg hook)
//   node scripts/pii-scan.mjs fix [paths…] [--apply]    replace PII with placeholders and strip image metadata (dry run without --apply)
//   node scripts/pii-scan.mjs history [--write-rewrite-files]   scan all git history; optionally write git-filter-repo inputs (outside the repo)
//   node scripts/pii-scan.mjs install-hooks             install pre-commit and commit-msg hooks that block PII
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';

const VERSION = 'v1';
const ROOT = process.cwd();
const CONF_DIR = path.join(process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config'), 'pii-redaction');
const IDENTITY = path.join(CONF_DIR, 'identity.json');
const KINDS = ['names', 'emails', 'phones', 'addresses', 'usernames', 'hostnames', 'domains', 'ips', 'macs', 'paths', 'machine', 'other'];
const GENERIC_USERS = new Set(['root', 'user', 'users', 'admin', 'ubuntu', 'debian', 'fedora', 'runner', 'node', 'app', 'www-data', 'nobody', 'vscode', 'codespace', 'claude', 'lab', 'docker', 'git', 'test', 'build', 'ci', 'guest', 'shared', 'public', 'default', 'home']);

function readJSON(f) { try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch { return null; } }
const PROJECT = { publicIdentity: {}, allow: [], skip: [], ...(readJSON(path.join(ROOT, '.pii-redaction.json')) || {}) };
const git = (...a) => { const r = spawnSync('git', a, { cwd: ROOT, encoding: 'utf8', maxBuffer: 256 << 20 }); return r.status === 0 ? r.stdout : null; };
const isRepo = () => (git('rev-parse', '--is-inside-work-tree') || '').trim() === 'true';
const mask = (v) => { v = String(v); return v.length <= 4 ? '*'.repeat(v.length) : v[0] + '*'.repeat(Math.min(v.length - 2, 8)) + v.at(-1); };
const h = (s) => crypto.createHash('sha256').update(String(s).toLowerCase()).digest();
const reEsc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

// ---------- identity: private file + live discovery ----------
function discover() {
  const d = Object.fromEntries(KINDS.map((k) => [k, []]));
  const ui = (() => { try { return os.userInfo(); } catch { return {}; } })();
  if (ui.username) d.usernames.push(ui.username);
  d.hostnames.push(os.hostname());
  if (os.hostname().includes('.')) d.hostnames.push(os.hostname().split('.')[0]);
  d.paths.push(os.homedir());
  for (const [, addrs] of Object.entries(os.networkInterfaces())) for (const a of addrs || []) {
    if (a.internal) continue;
    d.ips.push(a.address.split('%')[0]);
    if (a.mac && a.mac !== '00:00:00:00:00:00') d.macs.push(a.mac);
  }
  for (const f of ['/etc/machine-id', '/var/lib/dbus/machine-id']) { try { const v = fs.readFileSync(f, 'utf8').trim(); if (v) d.machine.push(v); } catch {} }
  try { const m = fs.readFileSync('/proc/cpuinfo', 'utf8').match(/^model name\s*:\s*(.+)$/m); if (m) d.machine.push(m[1].trim()); } catch {}
  d.machine.push(os.release());
  try { for (const l of fs.readFileSync('/proc/mounts', 'utf8').split('\n')) { const mp = (l.split(' ')[1] || '').replace(/\\040/g, ' '); if (/^\/(mnt|media|run\/media)\/[^/]+/.test(mp)) d.paths.push(mp.split('/').slice(0, mp.startsWith('/run/media') ? 5 : 3).join('/')); } } catch {}
  const gn = (git('config', '--global', 'user.name') || '').trim(), ge = (git('config', '--global', 'user.email') || '').trim();
  if (gn) d.names.push(gn); if (ge) d.emails.push(ge);
  return d;
}
function identity() {
  const file = readJSON(IDENTITY) || {};
  const live = discover();
  const id = Object.fromEntries(KINDS.map((k) => [k, [...new Set([...(file[k] || []), ...((file.discovered || {})[k] || []), ...live[k]])].filter(Boolean)]));
  const allow = new Set([...(PROJECT.allow || []), PROJECT.publicIdentity.name, PROJECT.publicIdentity.email, PROJECT.publicIdentity.handle].filter(Boolean).map((s) => s.toLowerCase()));
  for (const k of KINDS) id[k] = id[k].filter((v) => !allow.has(v.toLowerCase()));
  id.emails = id.emails.filter((e) => !PATTERNS[0].skip(e));
  id.usernames = id.usernames.filter((u) => u.length >= 4 && !GENERIC_USERS.has(u.toLowerCase()));
  id.hostnames = id.hostnames.filter((v) => v.length >= 4 && !/^(localhost|ubuntu|debian|fedora|runner)$/i.test(v));
  id.paths = id.paths.filter((p) => !/^\/(root|home|Users|mnt|media)?\/?$/.test(p));
  id.machine = id.machine.filter((v) => v.length >= 6);
  id.ips = id.ips.filter((ip) => !docIP(ip) && !/^(127\.|0\.0\.0\.0$|::1$)/.test(ip));
  return { id, allow, fromFile: !!readJSON(IDENTITY) };
}

// ---------- replacements ----------
const DOC4 = ['192.0.2', '198.51.100', '203.0.113'];
function docIP(ip) { return DOC4.some((p) => ip.startsWith(p + '.')) || /^2001:db8:/i.test(ip); }
function mapIP(ip) {
  if (ip.includes(':')) { const b = h(ip); return `2001:db8::${b.readUInt16BE(0).toString(16)}:${b.readUInt16BE(2).toString(16)}`; }
  const b = h(ip); return `${DOC4[b[0] % 3]}.${(b[1] % 253) + 1}`;
}
const mapMAC = (m) => { const b = h(m); return `00:00:5e:00:53:${b[0].toString(16).padStart(2, '0')}`; };
function replacementFor(kind, value) {
  const pub = PROJECT.publicIdentity || {};
  switch (kind) {
    case 'names': return pub.name || 'Project Maintainer';
    case 'emails': return pub.email || 'maintainer@example.com';
    case 'phones': return '555-0100';
    case 'addresses': return '123 Example St, Anytown';
    case 'usernames': return 'user';
    case 'hostnames': return 'lab-host';
    case 'domains': return 'example.net';
    case 'ips': return mapIP(value);
    case 'macs': return mapMAC(value);
    case 'paths': return '/path/to';
    case 'machine': return '[redacted-machine-info]';
    default: return '[redacted]';
  }
}

// ---------- detection ----------
const B = '(?<![A-Za-z0-9_])', A = '(?![A-Za-z0-9_])';
const PATTERNS = [
  { kind: 'emails', level: 'fail', re: /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g,
    skip: (v) => /@(example\.(com|org|net)|users\.noreply\.github\.com|localhost)$/i.test(v) || /(^|[._-])(no-?reply|noreply)/i.test(v) || /^git@/i.test(v) || /\.(png|jpe?g|gif|svg|webp)$/i.test(v) },
  { kind: 'ips', level: 'fail', re: /(?<![\w.:-])(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)(?![\w.]|\.\d)/g,
    skip: (v) => docIP(v) || /^(127\.|0\.0\.0\.0$|255\.|224\.)/.test(v) || /^\d+\.0\.0\.0$/.test(v) },
  { kind: 'macs', level: 'fail', re: /(?<![\w:-])(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}(?![\w:-])/gi, skip: (v) => /^00[:-]00[:-]5e[:-]00[:-]53/i.test(v) || /^(00[:-]){5}00$/.test(v) },
  { kind: 'ips', level: 'fail', re: /(?<![\w:.])(?:[0-9a-f]{1,4}:){2,7}(?::?[0-9a-f]{1,4}){1,7}(?![\w:])|(?<![\w:])fe80::[0-9a-f:]+(?![\w:])/gi,
    skip: (v) => docIP(v) || /^([0-9a-f]{2}:){5}[0-9a-f]{2}$/i.test(v) || /^::1?$/.test(v) || !/[a-f]/i.test(v) && v.split(':').length < 4 || /^\d{1,2}:\d{2}(:\d{2})?$/.test(v) },
  { kind: 'paths', level: 'fail', re: /(?:\/home\/|\/Users\/|[A-Za-z]:\\\\?Users\\\\?)([A-Za-z0-9._-]+)/g, group: 1, skip: (v) => GENERIC_USERS.has(v.toLowerCase()) },
  { kind: 'phones', level: 'fail', re: /(?<![\w.])(?:\+?1[ .-])?\(?[2-9]\d{2}\)?[ .-]\d{3}[ .-]\d{4}(?![\w.])/g, skip: (v) => /555[ .-]01\d\d$/.test(v) },
  { kind: 'addresses', level: 'warn', re: /\b\d{2,6}\s+(?:[NSEW]\.?\s+)?[A-Z][a-z]+(?:\s+[A-Z][a-z]+){0,3}\s+(?:St|Street|Ave|Avenue|Rd|Road|Blvd|Boulevard|Dr|Drive|Ln|Lane|Way|Ct|Court|Pl|Place|Pkwy|Hwy|Terrace|Ter|Cir|Circle)\b\.?/g, skip: (v) => /Example/.test(v) },
  { kind: 'domains', level: 'warn', re: /(?<![\w.(!\[{-])[a-z0-9][a-z0-9-]*(?:\.[a-z0-9-]+)*\.(?:lan|home|internal|localdomain|home\.arpa|local)(?![.\w(\-)\]};])/gi, skip: (v) => /^(example|lab-host|host)\./i.test(v) },
];
function literalRules(id) {
  const rules = [];
  for (const k of KINDS) for (const v of id[k]) {
    const ci = ['names', 'emails', 'hostnames', 'domains', 'usernames', 'macs'].includes(k);
    const wordy = /^[A-Za-z0-9]/.test(v) && /[A-Za-z0-9]$/.test(v);
    rules.push({ kind: k, value: v, re: new RegExp((wordy ? B : '') + reEsc(v) + (wordy ? A : ''), ci ? 'gi' : 'g') });
  }
  return rules.sort((a, b) => b.value.length - a.value.length);
}
function findIn(text, rules, allow, { patterns = true } = {}) {
  const out = []; const lines = text.split('\n');
  lines.forEach((line, i) => {
    if (/pii-scan:\s*allow/.test(line)) return;
    const taken = [];
    const hit = (kind, level, value, start, end) => {
      if (allow.has(value.toLowerCase()) || taken.some(([a, b]) => start < b && end > a)) return;
      taken.push([start, end]); out.push({ line: i + 1, kind, level, value });
    };
    for (const r of rules) for (const m of line.matchAll(r.re)) hit(r.kind, 'fail', m[0], m.index, m.index + m[0].length);
    for (const p of patterns ? PATTERNS : []) for (const m of line.matchAll(p.re)) {
      const v = p.group ? m[p.group] : m[0];
      if (p.skip && p.skip(v)) continue;
      const start = p.group ? m.index + m[0].indexOf(v, m[0].length - v.length) : m.index;
      hit(p.kind, p.level, v, start, start + v.length);
    }
  });
  return out;
}
function replaceIn(text, rules) {
  let n = 0;
  const lines = text.split('\n').map((line) => {
    if (/pii-scan:\s*allow/.test(line)) return line;
    for (const r of rules) line = line.replace(r.re, (m) => { n++; return replacementFor(r.kind, m); });
    for (const p of PATTERNS) {
      if (p.level !== 'fail') continue;
      line = line.replace(p.re, (...args) => {
        const m = args[0]; const v = p.group ? args[p.group] : m;
        if (p.skip && p.skip(v)) return m;
        n++; return p.group ? m.slice(0, m.length - v.length) + replacementFor('usernames', v) : replacementFor(p.kind, v);
      });
    }
    return line;
  });
  return { text: lines.join('\n'), n };
}

// ---------- files ----------
const IMG = /\.(png|jpe?g|webp|gif|tiff?|heic)$/i;
function projectFiles(paths) {
  let files = paths.length ? paths : isRepo() ? (git('ls-files', '--cached', '--others', '--exclude-standard') || '').split('\n').filter(Boolean) : walk('.');
  const skipRe = (PROJECT.skip || []).map((g) => new RegExp('^' + g.split('**').map((s) => s.split('*').map(reEsc).join('[^/]*')).join('.*') + '$'));
  return files.filter((f) => fs.existsSync(path.join(ROOT, f)) && fs.statSync(path.join(ROOT, f)).isFile()
    && !/(^|\/)(node_modules|\.git)\//.test(f) && !/(^|\/)[A-Z0-9-]+-STANDARD\.md$/.test(f) && !/(^|\/)scripts\/pii-scan\.mjs$/.test(f) && !skipRe.some((re) => re.test(f)));
}
function walk(dir, out = []) {
  for (const e of fs.readdirSync(path.join(ROOT, dir), { withFileTypes: true })) {
    if (['.git', 'node_modules'].includes(e.name)) continue;
    const p = path.posix.join(dir, e.name).replace(/^\.\//, '');
    if (e.isDirectory()) walk(p, out); else out.push(p);
  }
  return out;
}
const isBinary = (buf) => buf.subarray(0, 8000).includes(0);
function imageMeta(buf, name) {
  if (/\.jpe?g$/i.test(name)) { for (let i = 2; i + 4 < buf.length && buf[i] === 0xff;) { const mk = buf[i + 1], len = buf.readUInt16BE(i + 2); if (mk === 0xe1) return 'EXIF/XMP metadata (can hold GPS, device, user names)'; if (mk === 0xda) break; i += 2 + len; } }
  if (/\.png$/i.test(name)) { for (let i = 8; i + 8 < buf.length;) { const len = buf.readUInt32BE(i), t = buf.toString('latin1', i + 4, i + 8); if (['tEXt', 'iTXt', 'zTXt', 'eXIf'].includes(t)) return `PNG ${t} metadata`; if (t === 'IEND') break; i += 12 + len; } }
  return null;
}
function stripMeta(buf, name) {
  if (/\.jpe?g$/i.test(name)) { const parts = [buf.subarray(0, 2)]; let i = 2; while (i + 4 < buf.length && buf[i] === 0xff) { const mk = buf[i + 1]; if (mk === 0xda) break; const len = buf.readUInt16BE(i + 2); if (mk !== 0xe1 && mk !== 0xed) parts.push(buf.subarray(i, i + 2 + len)); i += 2 + len; } parts.push(buf.subarray(i)); return Buffer.concat(parts); }
  if (/\.png$/i.test(name)) { const parts = [buf.subarray(0, 8)]; let i = 8; while (i + 8 <= buf.length) { const len = buf.readUInt32BE(i), t = buf.toString('latin1', i + 4, i + 8); if (!['tEXt', 'iTXt', 'zTXt', 'eXIf', 'tIME'].includes(t)) parts.push(buf.subarray(i, i + 12 + len)); i += 12 + len; if (t === 'IEND') break; } return Buffer.concat(parts); }
  return buf;
}

// ---------- commands ----------
function report(findings, label) {
  const fails = findings.filter((f) => f.level === 'fail'), warns = findings.filter((f) => f.level === 'warn');
  for (const f of findings.slice(0, 60)) console.log(`  ${f.level === 'fail' ? 'FAIL' : 'warn'} ${f.file}${f.line ? ':' + f.line : ''} [${f.kind}] ${f.note || mask(f.value)}`);
  if (findings.length > 60) console.log(`  … ${findings.length - 60} more`);
  const byKind = {}; for (const f of findings) byKind[f.kind] = (byKind[f.kind] || 0) + 1;
  console.log(`pii-scan ${label}: ${fails.length ? 'PII FOUND' : 'CLEAN'} · ${fails.length} fail · ${warns.length} warn${findings.length ? ' · ' + Object.entries(byKind).map(([k, n]) => `${k} ${n}`).join(', ') : ''}`);
  return fails.length;
}
function gitIdentityFindings(id) {
  if (!isRepo()) return [];
  const out = []; const name = (git('config', 'user.name') || '').trim(), email = (git('config', 'user.email') || '').trim();
  const bad = (v, list) => v && list.some((x) => x.toLowerCase() === v.toLowerCase());
  if (bad(name, id.names)) out.push({ file: 'git config', kind: 'names', level: 'fail', value: name, note: `commit author name is personal: run git config user.name "${PROJECT.publicIdentity.name || '<public handle>'}"` });
  if (bad(email, id.emails) || (email && !/@(users\.noreply\.github\.com|example\.(com|org|net))$/i.test(email) && !(PROJECT.allow || []).includes(email) && email !== PROJECT.publicIdentity.email))
    out.push({ file: 'git config', kind: 'emails', level: 'fail', value: email, note: `commit email ${mask(email)} would be published: use a noreply address (git config user.email "${PROJECT.publicIdentity.email || '<id>+<handle>@users.noreply.github.com'}")` });
  return out;
}
function cmdScan(args) {
  const { id, allow, fromFile } = identity(); const rules = literalRules(id); const findings = [];
  if (args.includes('--message')) {
    const f = args[args.indexOf('--message') + 1]; const text = fs.readFileSync(f, 'utf8').split('\n').filter((l) => !l.startsWith('#')).join('\n');
    for (const x of findInText(text, rules, allow)) findings.push({ ...x, file: 'commit message' });
    process.exit(report(findings, 'message') ? 1 : 0);
  }
  const staged = args.includes('--staged');
  const paths = args.filter((a) => !a.startsWith('--'));
  const files = staged ? (git('diff', '--cached', '--name-only', '--diff-filter=ACMR') || '').split('\n').filter(Boolean).filter((f) => projectFiles([f]).length || !fs.existsSync(path.join(ROOT, f))) : projectFiles(paths);
  for (const f of files) {
    const buf = staged ? spawnSync('git', ['show', `:${f}`], { cwd: ROOT, maxBuffer: 256 << 20 }).stdout : fs.readFileSync(path.join(ROOT, f));
    if (!buf) continue;
    if (IMG.test(f)) { const m = imageMeta(buf, f); if (m) findings.push({ file: f, kind: 'machine', level: 'fail', value: '', note: `${m}: run fix --apply` }); findings.push({ file: f, kind: 'image', level: 'warn', value: '', note: 'image: check visually for usernames, hostnames, IPs, paths, faces, addresses' }); continue; }
    if (isBinary(buf)) { if (/\.(pdf|docx?|odt|xlsx?|pptx?|zip|tar|gz)$/i.test(f)) findings.push({ file: f, kind: 'binary', level: 'warn', value: '', note: 'binary document: review its text and metadata manually' }); continue; }
    if (buf.length > 5 * 1024 * 1024) { findings.push({ file: f, kind: 'large', level: 'warn', value: '', note: 'text file over 5 MB not scanned' }); continue; }
    for (const x of findInText(buf.toString('utf8'), rules, allow)) findings.push({ ...x, file: f });
  }
  findings.push(...gitIdentityFindings(id));
  if (!fromFile && !staged) console.log(`  info no identity file at ${IDENTITY.replace(os.homedir(), '~')}: using live discovery only (run init, then add names/addresses/phones)`);
  process.exit(report(findings, staged ? 'staged' : paths.length ? 'files' : 'tree') ? 1 : 0);
}
const findInText = (text, rules, allow) => findIn(text, rules, allow);

function cmdFix(args) {
  const apply = args.includes('--apply'); const { id } = identity(); const rules = literalRules(id);
  const files = projectFiles(args.filter((a) => !a.startsWith('--'))); let total = 0, changed = 0;
  for (const f of files) {
    const p = path.join(ROOT, f); const buf = fs.readFileSync(p);
    if (IMG.test(f)) { if (imageMeta(buf, f)) { changed++; console.log(`  ${apply ? 'fixed' : 'would fix'} ${f}: strip image metadata`); if (apply) fs.writeFileSync(p, stripMeta(buf, f)); } continue; }
    if (isBinary(buf) || buf.length > 5 * 1024 * 1024) continue;
    const { text, n } = replaceIn(buf.toString('utf8'), rules);
    if (n) { total += n; changed++; console.log(`  ${apply ? 'fixed' : 'would fix'} ${f}: ${n} replacement(s)`); if (apply) fs.writeFileSync(p, text); }
  }
  console.log(`fix: ${changed} file(s), ${total} replacement(s)${apply ? ' applied' : ' (dry run: add --apply)'}. Warn-level items (street addresses, .lan/.local names, images) need a manual look; re-run scan.`);
}

function cmdHistory(args) {
  if (!isRepo()) { console.error('history: not a git repository'); process.exit(2); }
  const { id, allow } = identity(); const rules = literalRules(id); const findings = [];
  const people = new Set((git('log', '--all', '--format=%an%x09%ae%n%cn%x09%ce') || '').split('\n').filter(Boolean));
  const flagged = new Set();
  for (const p of people) for (const x of findIn(p.replace('\t', ' <') + '>', rules, allow, { patterns: false })) { flagged.add(p); findings.push({ ...x, file: 'author/committer', line: 0 }); }
  const msgs = git('log', '--all', '--format=%H%n%B%n--END--') || ''; let commit = '';
  for (const l of msgs.split('\n')) { if (/^[0-9a-f]{40}$/.test(l)) { commit = l.slice(0, 9); continue; } if (l === '--END--') continue; for (const x of findIn(l, rules, allow)) findings.push({ ...x, file: `message ${commit}`, line: 0 }); }
  const patch = git('log', '--all', '-p', '--no-color', '--format=@@COMMIT %h') || ''; let cur = '', fileName = '';
  for (const l of patch.split('\n')) {
    if (l.startsWith('@@COMMIT ')) { cur = l.slice(9); continue; }
    if (l.startsWith('+++ b/')) { fileName = l.slice(6); continue; }
    if (!l.startsWith('+') || l.startsWith('+++') || /(^|\/)[A-Z0-9-]+-STANDARD\.md$/.test(fileName) || /(^|\/)scripts\/pii-scan\.mjs$/.test(fileName)) continue;
    for (const x of findIn(l.slice(1), rules, allow)) findings.push({ ...x, file: `${cur}:${fileName}`, line: 0 });
  }
  const seen = new Set(); const uniq = findings.filter((f) => { const k = f.file + f.kind + f.value; if (seen.has(k)) return false; seen.add(k); return true; });
  const n = report(uniq, 'history');
  if (args.includes('--write-rewrite-files') && uniq.length) {
    const dir = path.join(CONF_DIR, 'rewrite-' + path.basename(ROOT)); fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
    const vals = [...new Map(uniq.filter((f) => f.value && f.level === 'fail').map((f) => [f.value, f.kind])).entries()].sort((a, b) => b[0].length - a[0].length);
    fs.writeFileSync(path.join(dir, 'replacements.txt'), vals.map(([v, k]) => `${v}==>${replacementFor(k, v)}`).join('\n') + '\n', { mode: 0o600 });
    const pub = PROJECT.publicIdentity; const mm = [...flagged].map((p) => { const [nm, em] = p.split('\t'); return `${pub.name || 'Project Maintainer'} <${pub.email || 'maintainer@example.com'}> ${nm} <${em}>`; });
    fs.writeFileSync(path.join(dir, 'mailmap'), [...new Set(mm)].join('\n') + '\n', { mode: 0o600 });
    console.log(`history: rewrite inputs written to ${dir.replace(os.homedir(), '~')} (outside the repo). After the user approves, in a fresh clone:`);
    console.log(`  git filter-repo --replace-text ${dir.replace(os.homedir(), '~')}/replacements.txt --replace-message ${dir.replace(os.homedir(), '~')}/replacements.txt --mailmap ${dir.replace(os.homedir(), '~')}/mailmap`);
  }
  process.exit(n ? 1 : 0);
}

function cmdInit() {
  fs.mkdirSync(CONF_DIR, { recursive: true, mode: 0o700 });
  const cur = readJSON(IDENTITY) || { version: 1, ...Object.fromEntries(KINDS.map((k) => [k, []])) };
  cur.discovered = discover(); cur.discoveredAt = new Date().toISOString();
  fs.writeFileSync(IDENTITY, JSON.stringify(cur, null, 2) + '\n', { mode: 0o600 });
  fs.chmodSync(CONF_DIR, 0o700); fs.chmodSync(IDENTITY, 0o600);
  console.log(`init: identity file ${IDENTITY.replace(os.homedir(), '~')} (mode 600, never inside a repo)`);
  for (const k of KINDS) { const n = (cur[k] || []).length, d = cur.discovered[k].length; if (n || d) console.log(`  ${k.padEnd(10)} ${d} discovered, ${n} added by user${d ? ` (e.g. ${mask(cur.discovered[k][0])})` : ''}`); }
  console.log('init: add what discovery can\'t see (real name variants, street addresses, phone numbers, other usernames/handles, home-lab domains, SSIDs, serial numbers) to the top-level lists in that file.');
}

function cmdInstallHooks() {
  if (!isRepo()) { console.error('install-hooks: not a git repository'); process.exit(2); }
  const dir = (git('rev-parse', '--git-path', 'hooks') || '').trim(); fs.mkdirSync(path.join(ROOT, dir), { recursive: true });
  const hooks = { 'pre-commit': 'scan --staged', 'commit-msg': 'scan --message "$1"' };
  for (const [name, args] of Object.entries(hooks)) {
    const p = path.join(ROOT, dir, name); const marker = '# pii-scan hook (PII-REDACTION-STANDARD)';
    if (fs.existsSync(p) && !fs.readFileSync(p, 'utf8').includes(marker)) fs.renameSync(p, p + '.local');
    const chain = fs.existsSync(p + '.local') ? `"$(dirname "$0")/${name}.local" "$@" || exit $?\n` : '';
    fs.writeFileSync(p, `#!/bin/sh\n${marker}\n${chain}node "$(git rev-parse --show-toplevel)/scripts/pii-scan.mjs" ${args} || { echo "pii-scan: blocked. Fix with: node scripts/pii-scan.mjs fix --apply (then re-stage)"; exit 1; }\n`, { mode: 0o755 });
    console.log(`install-hooks: ${name} installed${chain ? ' (existing hook kept as ' + name + '.local and still runs)' : ''}`);
  }
}

const [cmd, ...rest] = process.argv.slice(2);
if (cmd === 'init') cmdInit();
else if (cmd === 'scan') cmdScan(rest);
else if (cmd === 'fix') cmdFix(rest);
else if (cmd === 'history') cmdHistory(rest);
else if (cmd === 'install-hooks') cmdInstallHooks();
else { console.log(fs.readFileSync(new URL(import.meta.url), 'utf8').split('\n').slice(1, 9).join('\n')); process.exit(2); }
```

| Command | Does | Output |
|---|---|---|
| `init` | Writes discovered values into the private identity file (mode 600) | Counts per category, masked examples |
| `scan [paths…]` | Scans the tree, or given files; checks the repo's git author identity | Masked findings, then `CLEAN` or `PII FOUND` (exit 1) |
| `scan --staged` / `scan --message <file>` | What the hooks run | Same |
| `fix [paths…] [--apply]` | Replaces PII with §5 placeholders; strips JPEG EXIF/XMP and PNG text metadata. Dry run by default | Files and replacement counts |
| `history [--write-rewrite-files]` | Scans all branches' history; optionally writes `git filter-repo` inputs outside the repo | Masked findings, and the rewrite command |
| `install-hooks` | Installs `pre-commit` and `commit-msg` hooks, keeping existing ones | One line per hook |

## 11. Anti-patterns (never)

- Committing the identity file, or copying real values into `.pii-redaction.json`, `CLAUDE.md`, docs,
  the decision log, a post-mortem register or test fixtures.
- Pasting full scan output or identity values into chats, PRs or issues unmasked.
- Bypassing the hooks with `--no-verify` without the user's explicit OK.
- Publishing an artifact, PDF or report without scanning that exact file.
- Committing screenshots without looking at them.
- Rewriting history or force-pushing without the user's approval, or claiming a rewrite removed copies
  that live in forks and caches.
- "Redacting" by encoding, hashing or abbreviating a real value (`D. W.`, base64): replace it.
- Using real network ranges "because they're private": 10.x, 172.16–31.x and 192.168.x addresses still
  describe the user's network.
- Rewriting `pii-scan.mjs` per project; fixes go back into this standard with a version bump.
