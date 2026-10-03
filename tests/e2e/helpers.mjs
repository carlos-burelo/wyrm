import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
export const E2E_DIR = here;
export const ROOT = path.resolve(here, '..', '..');
export const NPM_DIR = path.join(ROOT, 'npm');
export const WORKFLOW = path.join(ROOT, '.github', 'workflows', 'release.yml');
export const SCOPE = '@carlos-burelo/wyrm';

// Binario release (local) o vendor (empaquetado). Orden: build fresco primero.
export function findWyrmBin() {
  const cands = [
    path.join(ROOT, 'target', 'release', 'wyrm.exe'),
    path.join(ROOT, 'target', 'x86_64-pc-windows-msvc', 'release', 'wyrm.exe'),
    path.join(NPM_DIR, 'vendor', 'wyrm.exe'),
  ];
  for (const c of cands) if (fs.existsSync(c)) return c;
  throw new Error(`wyrm.exe no encontrado; corre \`pnpm run build:rust\`. Buscado: ${cands.join(', ')}`);
}

export function runWyrm(args, opts = {}) {
  const bin = opts.bin ?? findWyrmBin();
  const r = spawnSync(bin, args, {
    cwd: opts.cwd ?? ROOT,
    env: { ...process.env, ...(opts.env ?? {}) },
    encoding: 'utf8',
    timeout: opts.timeout ?? 30000,
  });
  return { status: r.status ?? 1, stdout: r.stdout ?? '', stderr: r.stderr ?? '', error: r.error };
}

export function runNode(args, opts = {}) {
  const r = spawnSync(process.execPath, args, {
    cwd: opts.cwd ?? ROOT,
    env: { ...process.env, ...(opts.env ?? {}) },
    encoding: 'utf8',
    timeout: opts.timeout ?? 60000,
  });
  return { status: r.status ?? 1, stdout: r.stdout ?? '', stderr: r.stderr ?? '', error: r.error };
}

export function runPnpm(args, opts = {}) {
  const base = {
    cwd: opts.cwd ?? ROOT,
    env: { ...process.env, ...(opts.env ?? {}) },
    encoding: 'utf8',
    timeout: opts.timeout ?? 120000,
  };
  // .cmd/.exe vía shell con string única: evita DEP0190 y citación rota.
  const q = (a) => (/[\s"]/.test(a) ? `"${a}"` : a);
  const r =
    process.platform === 'win32'
      ? spawnSync(['pnpm', ...args].map(q).join(' '), { ...base, shell: true })
      : spawnSync('pnpm', args, base);
  return { status: r.status ?? 1, stdout: r.stdout ?? '', stderr: r.stderr ?? '', error: r.error };
}

// ProgramData aislado: la DB y logs del e2e no tocan C:\ProgramData\wyrm.
export function isolatedEnv(prefix = 'wyrm-e2e') {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `${prefix}-`));
  return { dir, env: { ProgramData: dir } };
}

export function rmrf(p) {
  fs.rmSync(p, { recursive: true, force: true });
}

export function copyDir(src, dst) {
  fs.mkdirSync(dst, { recursive: true });
  for (const e of fs.readdirSync(src, { withFileTypes: true })) {
    const s = path.join(src, e.name);
    const d = path.join(dst, e.name);
    if (e.isDirectory()) copyDir(s, d);
    else fs.copyFileSync(s, d);
  }
}

export function readJson(p) {
  return JSON.parse(fs.readFileSync(p, 'utf8'));
}

export function cargoVersion() {
  const toml = fs.readFileSync(path.join(ROOT, 'crates', 'wyrm', 'Cargo.toml'), 'utf8');
  const m = toml.match(/^version\s*=\s*"([^"]+)"/m);
  if (!m) throw new Error('version no encontrada en crates/wyrm/Cargo.toml');
  return m[1];
}

export function npmVersion() {
  return readJson(path.join(NPM_DIR, 'package.json')).version;
}
