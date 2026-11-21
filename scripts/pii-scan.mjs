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
