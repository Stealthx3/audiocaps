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
