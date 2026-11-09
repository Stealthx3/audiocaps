# Phone Alerts Standard (v1)

> **Instruction to Claude:** If this file is in a project, the user must get a **push notification on
> their phone whenever the project needs them**: a permission prompt, a question, a decision, a blocked
> step. On first use, make the environment ready per §3 (install the hooks, check readiness, fix or
> report gaps). From then on a session-start hook checks readiness automatically and asks for a
> one-line reminder about Remote Control on the phone app: pass it on in your first reply (§4). When you
> need the user, follow §5. Nobody has to ask again.

---

## 1. What it is

A small, automatic setup that makes sure "Claude needs you" reaches the user's phone, and reminds the
user each session to confirm the phone side is ready.

| Piece | Path | Role |
|---|---|---|
| Alerts script | `scripts/phone-alerts.mjs` | Copied **verbatim** from §8. Checks readiness, installs hooks, runs as the hooks. |
| Project hooks | `.claude/settings.json` | A `SessionStart` hook (readiness + reminder) and a `Notification` hook (backup channel). Written by `install`. |
| Config | `.claude/phone-alerts.json` | Reminder mode and which events alert. Committed. |
| Private config | `.claude/phone-alerts.local.json` | Optional backup-channel URL and token. Git-ignored by `install`. |

The script only reads settings and sends notifications; it executes no project code, so it runs on the
host under the VM Lab Standard §1.

## 2. How alerts reach the phone

| Path | When it pushes | Needs |
|---|---|---|
| **Claude Code Remote Control** (primary) | "Push when actions required": permission prompts and questions. "Push when Claude decides": e.g. a long task finished, or when asked ("notify me when the tests finish"). | Claude app on the phone, signed in to the same account, notifications allowed; the session connected with Remote Control; the toggles on in `/config` (`inputNeededNotifEnabled`, `agentPushNotifEnabled`). The computer stays on with Claude Code running. |
| **Backup channel** (optional) | Every `Notification` hook event in `.claude/phone-alerts.json` → `events` (default: `permission_prompt`, `elicitation_dialog`, `elicitation_url_dialog`, `agent_needs_input`, `quota_auto_resume_stale`, `quota_auto_resume_disabled`). | An ntfy topic (ntfy.sh or self-hosted) or any webhook (e.g. Home Assistant) in `.claude/phone-alerts.local.json`. Works without Remote Control. |
| **Cowork / Claude app sessions** | When Claude is blocked on the user and they may be away. | The `PushNotification` tool, when the session has it (scheduled and remote runs usually do). |

The `Notification` hook also emits a desktop notification in terminals that support OSC 777
(Ghostty, Warp, urxvt); other terminals ignore it.

Things that silently disable Remote Control, and so all primary pushes:
`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` or `DISABLE_GROWTHBOOK` set in the shell or in a settings
file's `env` block, `disableRemoteControl: true`, or a project/local `remoteControlAtStartup: false`.
`check` looks for all of them.

## 3. Setup (Claude does this automatically, once per project)

1. Copy §8 to `scripts/phone-alerts.mjs` verbatim. Don't rewrite or "improve" it per project.
2. Run `node scripts/phone-alerts.mjs install`. It merges the two hooks into `.claude/settings.json`
   (keeping every existing setting and hook, idempotent), creates `.claude/phone-alerts.json`, and adds
   `.claude/phone-alerts.local.json` to `.gitignore`.
3. Run `node scripts/phone-alerts.mjs check` and act on the result:
   - **FAIL** lines: fix what's in the project; for anything in the user's environment (a shell
     variable, user settings), tell the user the one-line fix.
   - **warn** lines about Remote Control auto-connect or push toggles: ask the user once whether to turn
     them on for every session. On yes, run `node scripts/phone-alerts.mjs install-user` (sets
     `remoteControlAtStartup`, `inputNeededNotifEnabled` and `agentPushNotifEnabled` in
     `~/.claude/settings.json`, with a backup of the old file). Never change user settings without that yes.
4. Ask whether they want a backup channel (useful when Remote Control isn't connected). If yes, write
   `.claude/phone-alerts.local.json` (§6) and run `node scripts/phone-alerts.mjs test`.
5. Give the user the **phone checklist** (they must do these themselves):
   - Install the Claude app (iOS/Android) and sign in to the **same account** used for Claude Code.
   - Allow notifications for Claude (iOS: Settings → Notifications → Claude, and check Focus modes and
     notification summaries; Android: exempt Claude from battery optimization).
   - Open the app once after enabling push, so it registers (`/config` shows "No mobile registered" until it does).
   - Start sessions with `claude --remote-control` (or `/remote-control`) unless auto-connect is on, and
     confirm the session appears in the app under **Code**.
   - Test: in a connected session, ask Claude to send a push notification now.
6. Add one line to `CLAUDE.md`: `Phone alerts: see PHONE-ALERTS-STANDARD; hooks installed, check with node scripts/phone-alerts.mjs check.`
7. Commit `scripts/phone-alerts.mjs`, `.claude/settings.json`, `.claude/phone-alerts.json` and `.gitignore`.

Hooks take effect from the next session. The first time Claude Code opens the repo interactively, the
user accepts the workspace trust dialog.

## 4. Every session (automatic)

The `SessionStart` hook runs `session-start` on startup, resume and `/clear`. It:

- re-runs the readiness checks (fast, no network);
- shows the user a one-line status and the Remote Control reminder (as a hook message);
- adds the same status to Claude's context, with the user's request for a reminder.

Claude's part: in the **first reply of the session**, include one line reminding the user to confirm
Remote Control is set up in the phone app (session visible under Code, notifications allowed, same
account). If the status says **NOT ready** or **partly ready**, say what's wrong in that same line or the
next and offer the fix. Don't repeat the reminder later in the session.

`.claude/phone-alerts.json` → `"remind": "issues-only"` silences the reminder when everything is ready;
the default `"every-session"` always shows it (the user asked for that).

## 5. When Claude needs the user

- **Use the real prompt mechanisms** (permission prompts, `AskUserQuestion`), not a question buried in
  prose at the end of a long turn. Those are what trigger "Push when actions required" and the
  `Notification` hook.
- **Batch.** Collect independent questions into one prompt instead of interrupting several times.
- **Don't block on what you can decide.** Use sensible defaults for reversible choices and state them;
  stop and ask (and so push) only for decisions that are the user's, irreversible, or risky.
- **Long tasks:** when starting something that runs a long time unattended (a large build, a test matrix,
  a VM rebuild), tell the user you'll push when it's done or blocked, and do it.
- **Cowork and other Claude app sessions:** when blocked on the user and they may be away, send a
  `PushNotification` (if available) whose first sentence says what's needed, then ask.
- **Never** put secrets, tokens or personal data in a notification; titles and messages may show on a
  lock screen.

## 6. `.claude/phone-alerts.json` and `.claude/phone-alerts.local.json`

```json
{
  "standard": "PHONE-ALERTS-STANDARD v1",
  "remind": "every-session",
  "events": ["permission_prompt", "elicitation_dialog", "elicitation_url_dialog", "agent_needs_input", "quota_auto_resume_stale", "quota_auto_resume_disabled"]
}
```

`.claude/phone-alerts.local.json` (git-ignored; same keys, overrides the committed file):

```json
{ "backup": { "type": "ntfy", "url": "https://ntfy.example.net/claude-alerts", "token": "tk_…", "priority": "high" } }
```

- `type: "ntfy"` (default): POSTs the message as the body with `Title`, `Priority` and `Tags` headers,
  and a `Bearer` token if given.
- `type: "webhook"`: POSTs JSON `{ "title", "message", "project" }` (e.g. a Home Assistant webhook that
  forwards to the companion app).
- Add `idle_prompt` to `events` only if you also want a ping whenever Claude finishes a turn and you
  haven't replied for a minute. It's noisy.
- After changing `events`, run `node scripts/phone-alerts.mjs install` again to update the hook matcher.
- Backup sends time out after 5 seconds and never block or break the session.

## 7. Troubleshooting

| Symptom | Fix |
|---|---|
| `/config` shows "No mobile registered" | Open the Claude app on the phone; it refreshes its push token, and the warning clears next time Remote Control connects. |
| Pushes delayed or missing on iOS | Focus modes or notification summaries: Settings → Notifications → Claude. |
| Pushes delayed on Android | Exempt the Claude app from battery optimization. |
| Session not in the app's Code tab | Remote Control isn't connected: start with `claude --remote-control`, run `/remote-control`, or turn on auto-connect (`install-user`). Check the blockers in §2. |
| No reminder at session start | Hooks not installed or workspace not trusted: run `check`, then open the repo interactively once and accept the trust dialog. |
| Backup channel silent | Run `node scripts/phone-alerts.mjs test`; it prints the HTTP status or error. |

## 8. `scripts/phone-alerts.mjs` (copy verbatim)

Node ≥18, no dependencies. Run from the repo root (hooks pass `CLAUDE_PROJECT_DIR`).

```js
#!/usr/bin/env node
// phone-alerts.mjs — Phone Alerts Standard v1. Node >=18, no dependencies. Run from the repo root.
//   node scripts/phone-alerts.mjs check           readiness report (exit 1 if something blocks phone alerts)
//   node scripts/phone-alerts.mjs install         add the SessionStart + Notification hooks to .claude/settings.json
//   node scripts/phone-alerts.mjs install-user    turn on Remote Control auto-connect + push toggles in ~/.claude/settings.json (ask the user first)
//   node scripts/phone-alerts.mjs test            send a test alert through the backup channel, if one is configured
//   node scripts/phone-alerts.mjs session-start   (hook) readiness summary + Remote Control reminder at session start
//   node scripts/phone-alerts.mjs notify          (hook) forward a "Claude needs you" event to the backup channel
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';

const VERSION = 'v1';
const ROOT = process.env.CLAUDE_PROJECT_DIR || process.cwd();
const HOME = os.homedir();
const SCRIPT_ARG = '${CLAUDE_PROJECT_DIR}/scripts/phone-alerts.mjs';
const FILES = {
  managed: { darwin: '/Library/Application Support/ClaudeCode/managed-settings.json', win32: 'C:\\Program Files\\ClaudeCode\\managed-settings.json' }[process.platform] || '/etc/claude-code/managed-settings.json',
  local: path.join(ROOT, '.claude/settings.local.json'),
  project: path.join(ROOT, '.claude/settings.json'),
  user: path.join(HOME, '.claude/settings.json'),
};
const PRECEDENCE = ['managed', 'local', 'project', 'user']; // highest first
const DEFAULT_EVENTS = ['permission_prompt', 'elicitation_dialog', 'elicitation_url_dialog', 'agent_needs_input', 'quota_auto_resume_stale', 'quota_auto_resume_disabled'];
const RC_BLOCKERS = ['CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC', 'DISABLE_GROWTHBOOK'];
const REMINDER = 'Reminder: in the Claude phone app, open Code and check this session appears there (Remote Control), notifications for Claude are allowed, and you are signed in to the same account.';

const parseErrors = [];
function readJSON(file, label) {
  try { return JSON.parse(fs.readFileSync(file, 'utf8')); }
  catch (e) { if (e.code !== 'ENOENT') parseErrors.push(`${label || file}: ${e.message}`); return null; }
}
const settings = () => Object.fromEntries(PRECEDENCE.map((k) => [k, readJSON(FILES[k], k + ' settings')]));
function config() {
  const base = readJSON(path.join(ROOT, '.claude/phone-alerts.json'), 'phone-alerts.json') || {};
  const local = readJSON(path.join(ROOT, '.claude/phone-alerts.local.json'), 'phone-alerts.local.json') || {};
  return { remind: 'every-session', events: DEFAULT_EVENTS, backup: null, ...base, ...local };
}
const effective = (S, key) => { for (const k of PRECEDENCE) if (S[k] && S[k][key] !== undefined) return { value: S[k][key], from: k }; return { value: undefined, from: null }; };
const hasHook = (s, event) => ((s && s.hooks && s.hooks[event]) || []).some((g) => (g.hooks || []).some((h) => JSON.stringify(h).includes('phone-alerts.mjs')));
const short = (s, n = 200) => { s = String(s ?? '').replace(/\s+/g, ' ').trim(); return s.length > n ? s.slice(0, n - 1) + '…' : s; };

function checks({ cli = false } = {}) {
  const S = settings(); const C = config(); const out = [];
  const add = (level, msg) => out.push({ level, msg });
  const major = +process.versions.node.split('.')[0];
  add(major >= 18 ? 'ok' : 'fail', `Node ${process.versions.node}${major >= 18 ? '' : ' (needs 18+)'}`);
  if (cli) {
    const r = spawnSync('claude', ['--version'], { encoding: 'utf8', timeout: 15000 });
    add(r.status === 0 ? 'ok' : 'warn', r.status === 0 ? `Claude Code ${short(r.stdout, 60)}` : 'claude CLI not found on PATH (fine if this project only runs in Cowork)');
  }
  for (const v of RC_BLOCKERS) {
    if (process.env[v]) add('fail', `${v} is set in the environment: it disables Remote Control, so no phone pushes`);
    for (const k of PRECEDENCE) if (S[k] && S[k].env && S[k].env[v] !== undefined) add('fail', `${v} is set in ${k} settings (env block): it disables Remote Control`);
  }
  const dis = effective(S, 'disableRemoteControl');
  if (dis.value === true) add('fail', `disableRemoteControl is true in ${dis.from} settings`);
  const rcUser = [S.managed, S.user].some((s) => s && s.remoteControlAtStartup === true);
  const rcOff = [S.local, S.project].some((s) => s && s.remoteControlAtStartup === false);
  if (rcOff) add('fail', 'this repo sets remoteControlAtStartup false, so sessions here never auto-connect');
  else if (rcUser) add('ok', 'Remote Control auto-connects every session (remoteControlAtStartup)');
  else add('warn', 'Remote Control is not automatic: start with `claude --remote-control`, or turn on "Enable Remote Control for all sessions" in /config');
  const need = effective(S, 'inputNeededNotifEnabled');
  if (need.value === true) add('ok', `push when Claude is waiting on you: on (${need.from} settings)`);
  else if (need.value === false) add('fail', `push when Claude is waiting on you: off (${need.from} settings)`);
  else add('warn', 'push when Claude is waiting on you: not set in any settings file; turn on "Push when actions required" in /config');
  const agent = effective(S, 'agentPushNotifEnabled');
  add(agent.value === true ? 'ok' : 'info', `push when Claude decides (e.g. long task done): ${agent.value === true ? 'on' : 'off/not set (optional)'}`);
  const ss = hasHook(S.project, 'SessionStart'), nh = hasHook(S.project, 'Notification');
  add(ss && nh ? 'ok' : 'fail', ss && nh ? 'project hooks installed (.claude/settings.json)' : 'project hooks missing: run `node scripts/phone-alerts.mjs install`');
  if (C.backup && C.backup.url) add('ok', `backup channel: ${C.backup.type || 'ntfy'} via ${(() => { try { return new URL(C.backup.url).host; } catch { return 'invalid URL'; } })()}`);
  else add('info', 'no backup channel (optional: ntfy or webhook in .claude/phone-alerts.local.json)');
  for (const e of parseErrors) add('fail', `unreadable JSON: ${e}`);
  add('manual', REMINDER);
  return { out, C };
}

async function readStdin(ms = 2000) {
  if (process.stdin.isTTY) return {};
  return new Promise((resolve) => {
    let data = ''; const t = setTimeout(() => resolve(parse(data)), ms);
    const parse = (d) => { try { return JSON.parse(d); } catch { return {}; } };
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', (c) => (data += c));
    process.stdin.on('end', () => { clearTimeout(t); resolve(parse(data)); });
    process.stdin.on('error', () => { clearTimeout(t); resolve({}); });
  });
}

async function send(C, title, message, priority) {
  const b = C.backup;
  if (!b || !b.url) return 'no backup channel configured';
  const ctl = new AbortController(); const t = setTimeout(() => ctl.abort(), 5000);
  try {
    let res;
    if ((b.type || 'ntfy') === 'ntfy') {
      const headers = { Title: title.replace(/[^\x20-\x7e]/g, '-'), Priority: String(priority || b.priority || 'high'), Tags: 'robot' };
      if (b.token) headers.Authorization = `Bearer ${b.token}`;
      res = await fetch(b.url, { method: 'POST', headers, body: message, signal: ctl.signal });
    } else {
      const headers = { 'Content-Type': 'application/json' };
      if (b.token) headers.Authorization = `Bearer ${b.token}`;
      res = await fetch(b.url, { method: 'POST', headers, body: JSON.stringify({ title, message, project: path.basename(ROOT) }), signal: ctl.signal });
    }
    return res.ok ? 'sent' : `HTTP ${res.status}`;
  } catch (e) { return e.name === 'AbortError' ? 'timed out' : e.message; } finally { clearTimeout(t); }
}

function writeJSON(file, obj) { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, JSON.stringify(obj, null, 2) + '\n'); }

function cmdInstall() {
  const file = FILES.project;
  const s = readJSON(file, '.claude/settings.json');
  if (parseErrors.length) { console.error(`install: ${parseErrors[0]}; fix it by hand first, nothing changed`); process.exit(1); }
  const C = config();
  const S = s || {};
  S.hooks = S.hooks || {};
  const put = (event, group) => {
    const list = (S.hooks[event] || []).filter((g) => !(g.hooks || []).some((h) => JSON.stringify(h).includes('phone-alerts.mjs')));
    S.hooks[event] = [...list, group];
  };
  put('SessionStart', { matcher: 'startup|resume|clear', hooks: [{ type: 'command', command: 'node', args: [SCRIPT_ARG, 'session-start'], timeout: 10 }] });
  put('Notification', { matcher: C.events.join('|'), hooks: [{ type: 'command', command: 'node', args: [SCRIPT_ARG, 'notify'], timeout: 15 }] });
  writeJSON(file, S);
  const cfg = path.join(ROOT, '.claude/phone-alerts.json');
  if (!fs.existsSync(cfg)) writeJSON(cfg, { standard: `PHONE-ALERTS-STANDARD ${VERSION}`, remind: 'every-session', events: DEFAULT_EVENTS });
  const gi = path.join(ROOT, '.gitignore'); const line = '.claude/phone-alerts.local.json';
  const cur = fs.existsSync(gi) ? fs.readFileSync(gi, 'utf8') : '';
  if (!cur.split('\n').includes(line)) fs.writeFileSync(gi, cur + (cur && !cur.endsWith('\n') ? '\n' : '') + line + '\n');
  console.log('install: hooks written to .claude/settings.json (SessionStart, Notification); config .claude/phone-alerts.json; .gitignore covers phone-alerts.local.json');
  console.log('install: hooks take effect in the next session. Run `node scripts/phone-alerts.mjs check` next.');
}

function cmdInstallUser() {
  const s = readJSON(FILES.user, '~/.claude/settings.json');
  if (parseErrors.length) { console.error(`install-user: ${parseErrors[0]}; fix it by hand first, nothing changed`); process.exit(1); }
  const S = s || {}; const changed = [];
  for (const [k, v] of [['remoteControlAtStartup', true], ['inputNeededNotifEnabled', true], ['agentPushNotifEnabled', true]]) if (S[k] !== v) { S[k] = v; changed.push(k); }
  if (!changed.length) { console.log('install-user: already set, nothing changed'); return; }
  if (fs.existsSync(FILES.user)) fs.copyFileSync(FILES.user, FILES.user + '.bak-phone-alerts');
  writeJSON(FILES.user, S);
  console.log(`install-user: set ${changed.join(', ')} in ~/.claude/settings.json${fs.existsSync(FILES.user + '.bak-phone-alerts') ? ' (backup: settings.json.bak-phone-alerts)' : ''}`);
  console.log('install-user: confirm in /config that "Enable Remote Control for all sessions" and "Push when actions required" show as on.');
}

function cmdCheck() {
  const { out } = checks({ cli: true });
  const mark = { ok: 'ok  ', fail: 'FAIL', warn: 'warn', info: 'info', manual: 'YOU ' };
  console.log(`phone-alerts check (${path.basename(ROOT)})`);
  for (const r of out) console.log(`  ${mark[r.level]} ${r.msg}`);
  const fails = out.filter((r) => r.level === 'fail').length, warns = out.filter((r) => r.level === 'warn').length;
  console.log(fails ? `check: NOT READY (${fails} blocking, ${warns} warnings)` : warns ? `check: READY WITH WARNINGS (${warns})` : 'check: READY');
  process.exit(fails ? 1 : 0);
}

async function cmdSessionStart() {
  const input = await readStdin();
  const { out, C } = checks();
  const fails = out.filter((r) => r.level === 'fail'), warns = out.filter((r) => r.level === 'warn');
  if (C.remind === 'issues-only' && !fails.length && !warns.length) return;
  const status = fails.length ? `Phone alerts NOT ready: ${fails[0].msg}${fails.length > 1 ? ` (+${fails.length - 1} more)` : ''}.`
    : warns.length ? `Phone alerts partly ready: ${warns[0].msg}${warns.length > 1 ? ` (+${warns.length - 1} more)` : ''}.` : 'Phone alerts ready.';
  const detail = [...fails, ...warns].map((r) => `- ${r.level}: ${r.msg}`).join('\n');
  const context = `Phone Alerts Standard (session ${input.source || 'start'}): ${status}${detail ? '\n' + detail : ''}\nThe user wants a one-line reminder in Claude's first reply this session to confirm Claude Remote Control is set up in the Claude phone app (Code tab shows this session, notifications allowed, same account).${fails.length || warns.length ? ' The issues above can be checked with `node scripts/phone-alerts.mjs check`.' : ''}`;
  process.stdout.write(JSON.stringify({ systemMessage: short(`${status} ${REMINDER}`, 600), hookSpecificOutput: { hookEventName: 'SessionStart', additionalContext: context } }));
}

async function cmdNotify() {
  const input = await readStdin();
  const C = config();
  const title = `Claude · ${path.basename(ROOT)}`;
  const message = short(input.message || 'Claude needs your attention', 300);
  if (C.backup && C.backup.url) await send(C, title, message, input.notification_type === 'permission_prompt' ? 'high' : undefined);
  const clean = (s) => String(s).replace(/[\x00-\x1f\x7f;]/g, ' ');
  process.stdout.write(JSON.stringify({ terminalSequence: `\x1b]777;notify;${clean(title)};${clean(message)}\x07` }));
}

async function cmdTest() {
  const C = config();
  const r = await send(C, `Claude · ${path.basename(ROOT)}`, 'Test alert from phone-alerts.mjs: if you see this on your phone, the backup channel works.', 'default');
  console.log(`test: backup channel: ${r}`);
  console.log('test: to test Remote Control pushes, open a session with Remote Control on and ask Claude: "send me a push notification now".');
  process.exit(r === 'sent' || r === 'no backup channel configured' ? 0 : 1);
}

const cmd = process.argv[2];
try {
  if (cmd === 'check') cmdCheck();
  else if (cmd === 'install') cmdInstall();
  else if (cmd === 'install-user') cmdInstallUser();
  else if (cmd === 'session-start') await cmdSessionStart();
  else if (cmd === 'notify') await cmdNotify();
  else if (cmd === 'test') await cmdTest();
  else { console.log(fs.readFileSync(new URL(import.meta.url), 'utf8').split('\n').slice(1, 8).join('\n')); process.exit(2); }
} catch (e) {
  // Hooks must never break a session: report and exit 0 for hook commands.
  console.error(`phone-alerts: ${e.message}`);
  process.exit(['session-start', 'notify'].includes(cmd) ? 0 : 1);
}
```

| Command | Does | Output |
|---|---|---|
| `check` | Full readiness report, including the installed Claude Code version | One line per check (`ok` / `FAIL` / `warn` / `info` / `YOU`), then `READY` / `READY WITH WARNINGS` / `NOT READY` (exit 1) |
| `install` | Merges the hooks into `.claude/settings.json`, creates the config, updates `.gitignore` | Two lines |
| `install-user` | Turns on Remote Control auto-connect and both push toggles in `~/.claude/settings.json` (backup kept) | What changed |
| `test` | Sends a test through the backup channel | Result, plus how to test Remote Control pushes |
| `session-start` | Hook: readiness status and reminder as hook JSON | JSON only |
| `notify` | Hook: forwards the event to the backup channel, emits a terminal notification | JSON only |

## 9. Anti-patterns (never)

- Changing `~/.claude/settings.json` (`install-user`) without the user's explicit yes.
- Replacing `.claude/settings.json` wholesale instead of merging, or hand-editing the hooks so they no
  longer reference `scripts/phone-alerts.mjs` (the checks look for it).
- Committing a backup-channel URL or token: they go in `.claude/phone-alerts.local.json` only.
- Hooks that can fail a session: both hook commands always exit 0 and print only JSON.
- Skipping or repeating the session reminder: once, in the first reply.
- Asking questions in a way that doesn't trigger a prompt, then waiting silently for an answer.
- Notifying for routine progress, or putting secrets or personal data in a notification.
- Rewriting `phone-alerts.mjs` per project; fixes go back into this standard with a version bump.
