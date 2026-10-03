// Release de wyrm en un comando: `pnpm release [X.Y.Z] [--dry-run]`
// Sin versión: auto-bump (patch sobre lo último publicado en el registry).
// Verifica sesión npm + maintainer ANTES de testear/compilar: si el publish
// va a fallar con 404 (identidad sin permiso), lo sabes en segundos.
// El único paso manual es el OAuth de npm en `pnpm publish` (abre el navegador).
// Sin dependencias: solo builtins de node. Falla rápido ante cualquier error.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const NPM_DIR = path.join(ROOT, 'npm');
const SCOPE = '@carlos-burelo/wyrm';

const DRY = process.argv.includes('--dry-run');
const argVer = process.argv.slice(2).find((a) => !a.startsWith('--'));

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, {
    cwd: opts.cwd ?? ROOT,
    env: { ...process.env, ...(opts.env ?? {}) },
    encoding: 'utf8',
    timeout: opts.timeout ?? 600000,
  });
  return { status: r.status ?? 1, out: (r.stdout ?? '') + (r.stderr ?? '') };
}

// En win32 los .cmd/.exe se invocan con string única (evita DEP0190).
function sh(cmdline, opts = {}) {
  const r = spawnSync(cmdline, {
    cwd: opts.cwd ?? ROOT,
    env: { ...process.env, ...(opts.env ?? {}) },
    encoding: 'utf8',
    timeout: opts.timeout ?? 600000,
    shell: true,
  });
  return { status: r.status ?? 1, out: (r.stdout ?? '') + (r.stderr ?? '') };
}

function step(name) {
  console.log(`\n== ${name} ==`);
}

function fail(msg) {
  console.error(`\nFALLO: ${msg}`);
  process.exit(1);
}

function must(res, msg) {
  if (res.status !== 0) fail(`${msg}\n${res.out.slice(-2000)}`);
  return res.out;
}

function readJson(p) {
  return JSON.parse(fs.readFileSync(p, 'utf8'));
}

function parseVer(v) {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec((v ?? '').trim());
  return m ? [+m[1], +m[2], +m[3]] : null;
}

function cmpVer(a, b) {
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] - b[i];
  return 0;
}

function fmtVer([a, b, c]) {
  return `${a}.${b}.${c}`;
}

// Último publicado en el registry (null si no hay red o no existe).
function latestPublished() {
  const r = sh(`pnpm view "${SCOPE}" dist-tags.latest`);
  if (r.status !== 0) return null;
  const tok = r.out.trim().split(/\s+/).pop();
  return parseVer(tok) ? tok : null;
}

function tokenFromNpmrc() {
  const files = [
    path.join(process.env.USERPROFILE ?? process.env.HOME ?? '', '.npmrc'),
    path.join(ROOT, '.npmrc'),
    path.join(NPM_DIR, '.npmrc'),
  ];
  for (const f of files) {
    if (!fs.existsSync(f)) continue;
    const m = fs.readFileSync(f, 'utf8').match(/_authToken\s*=\s*(.+)/);
    if (!m) continue;
    let t = m[1].trim().replace(/^["']|["']$/g, '');
    const envRef = t.match(/^\$\{(.+)\}$/);
    if (envRef) t = process.env[envRef[1]] ?? '';
    if (t) return t;
  }
  return '';
}

// Quién eres en npm + si puedes publicar el paquete. Falla rápido antes
// de compilar/testear en vano (un 404 al publicar casi siempre es identidad
// sin permiso como maintainer, no problema de versión).
async function authPreflight() {
  const token = process.env.NPM_TOKEN || process.env.NODE_AUTH_TOKEN || tokenFromNpmrc();
  if (!token) {
    fail('sin sesión npm (no hay authToken). Corre `pnpm login`, completa el OAuth y reintenta');
  }
  const headers = { Authorization: `Bearer ${token}` };
  let me;
  try {
    const r = await fetch('https://registry.npmjs.org/-/whoami', { headers });
    if (r.status === 401) fail('token npm inválido o expirado. Corre `pnpm login` de nuevo');
    if (!r.ok) fail(`registry whoami -> HTTP ${r.status}`);
    me = (await r.json()).username;
  } catch (e) {
    fail(`sin red hacia registry.npmjs.org o registry caído: ${e.message}`);
  }
  console.log(`sesión npm: @${me}`);
  let maintainers = [];
  try {
    const pkg = await fetch(`https://registry.npmjs.org/${SCOPE.replace('/', '%2f')}`);
    if (pkg.ok) maintainers = ((await pkg.json()).maintainers ?? []).map((m) => m.name);
  } catch {
    console.log('aviso: no se pudo leer maintainers (sigo de todos modos)');
  }
  if (maintainers.length > 0 && !maintainers.includes(me)) {
    fail(`@${me} no es maintainer de ${SCOPE} (maintainers: ${maintainers.join(', ')}). El registry responde 404 en ese caso. Logueate con la cuenta dueña: \`pnpm login\``);
  }
}

function cargoVersion() {
  const m = fs
    .readFileSync(path.join(ROOT, 'crates', 'wyrm', 'Cargo.toml'), 'utf8')
    .match(/^version\s*=\s*"([^"]+)"/m);
  if (!m) fail('version no encontrada en crates/wyrm/Cargo.toml');
  return m[1];
}

function setCargoVersion(v) {
  const p = path.join(ROOT, 'crates', 'wyrm', 'Cargo.toml');
  const s = fs.readFileSync(p, 'utf8').replace(/^version\s*=\s*"[^"]+"/m, `version = "${v}"`);
  fs.writeFileSync(p, s);
}

// --- 1. Versión objetivo ----------------------------------------------------
// Sin arg: auto-bump (patch sobre lo último publicado; respeta bump local
// si ya es mayor). Con arg: esa versión exacta.
const npmPkgPath = path.join(NPM_DIR, 'package.json');
const current = readJson(npmPkgPath).version;
if (!parseVer(current)) fail(`versión local inválida: ${current}`);
const PUB = latestPublished();
let VER;
if (argVer) {
  if (!parseVer(argVer)) fail(`versión inválida: ${argVer} (usa X.Y.Z)`);
  VER = argVer;
} else if (!PUB) {
  VER = current;
} else if (cmpVer(parseVer(current), parseVer(PUB)) > 0) {
  VER = current;
} else {
  const [a, b, c] = parseVer(PUB);
  VER = fmtVer([a, b, c + 1]);
}
console.log(`registry latest: ${PUB ?? '?'} | local: ${current} | target: ${VER}${DRY ? ' (dry-run)' : ''}`);

// --- 2. Precondiciones -------------------------------------------------------
step('precondiciones');
must(run('git', ['rev-parse', '--is-inside-work-tree']), 'no es repo git');
const branch = run('git', ['branch', '--show-current']).out.trim();
if (branch !== 'main') fail(`rama actual: ${branch || '?'} (usa main)`);
for (const t of ['cargo', 'pnpm', 'node', 'git']) {
  const probe = process.platform === 'win32' ? sh(`where ${t}`) : run('sh', ['-c', `command -v ${t}`]);
  if (probe.status !== 0) fail(`herramienta faltante en PATH: ${t}`);
}
if (run('git', ['status', '--porcelain']).out.trim() !== '') {
  fail('árbol sucio: commitea o stashea antes del release');
}

// --- 2b. Sesión npm primero: si el publish va a fallar, que sea ahora -------
step('sesión npm');
await authPreflight();

// --- 3. Sync de versiones + check ya publicado --------------------------------
step('versiones');
if (current !== VER || cargoVersion() !== VER) {
  if (DRY) {
    console.log(`dry-run: sincronizaría Cargo.toml + npm/package.json -> ${VER}`);
  } else {
    const j = readJson(npmPkgPath);
    j.version = VER;
    fs.writeFileSync(npmPkgPath, JSON.stringify(j, null, 2) + '\n');
    setCargoVersion(VER);
    must(sh('cargo check -p wyrm'), 'cargo check tras bump (Cargo.lock)');
    console.log(`versiones -> ${VER}`);
  }
} else {
  console.log(`sync ok: Cargo == npm == ${VER}`);
}
const view = sh(`pnpm view "${SCOPE}@${VER}" version`);
if (view.status === 0) fail(`${SCOPE}@${VER} ya publicado; sube la versión`);

// --- 4. Tests -----------------------------------------------------------------
step('unit tests (cargo)');
must(sh('cargo test -p wyrm'), 'cargo test falló');

step('e2e (tests/e2e/)');
must(sh('node --test "tests/e2e/*.test.mjs"'), 'e2e falló');

// --- 5. Build + vendor + smoke --------------------------------------------------
step('build release');
must(sh('cargo build --release'), 'cargo build falló');
fs.mkdirSync(path.join(NPM_DIR, 'vendor'), { recursive: true });
fs.copyFileSync(
  path.join(ROOT, 'target', 'release', process.platform === 'win32' ? 'wyrm.exe' : 'wyrm'),
  path.join(NPM_DIR, 'vendor', 'wyrm.exe'),
);

step('smoke wrapper + tarball');
const cli = path.join(NPM_DIR, 'bin', 'cli.js');
const v = must(run(process.execPath, [cli, '--version']), 'wrapper no responde');
if (!v.trim().endsWith(VER)) fail(`wrapper dice ${v.trim()}, esperado wyrm ${VER}`);
must(run(process.execPath, [cli, 'doctor']), 'doctor falló');
const pack = must(sh('pnpm --dir npm pack --dry-run'), 'pack falló');
for (const f of ['bin/cli.js', 'vendor/wyrm.exe', 'package.json']) {
  if (!pack.includes(f)) fail(`tarball sin ${f}`);
}
console.log('tarball ok: bin/cli.js + vendor/wyrm.exe');

// --- 6. Commit + tag + push -------------------------------------------------------
step('git tag + push');
if (DRY) {
  console.log('dry-run: aquí commitearía bump, crearía v' + VER + ' y pushearía main + tag');
} else {
  if (run('git', ['status', '--porcelain']).out.trim() !== '') {
    must(run('git', ['add', 'crates/wyrm/Cargo.toml', 'Cargo.lock', 'npm/package.json']), 'git add');
    must(run('git', ['commit', '-m', `chore(release): v${VER}`]), 'git commit');
  }
  must(run('git', ['push', 'origin', 'main']), 'push main falló');
  run('git', ['tag', '-d', `v${VER}`]); // re-release del mismo número si hizo falta
  must(run('git', ['tag', `v${VER}`]), 'tag falló');
  if (run('git', ['push', 'origin', `v${VER}`]).status !== 0) {
    console.log('tag remoto avanzó: force-push (el script es dueño del tag de release)');
    must(run('git', ['push', '--force', 'origin', `v${VER}`]), 'push tag falló');
  }
  console.log(`tag v${VER} pusheado: CI construye el GitHub Release`);
}

// --- 7. Publish (único paso manual: OAuth) ------------------------------------------
step('npm publish');
if (DRY) {
  console.log('dry-run: aquí correría `pnpm --dir npm publish --access public` (OAuth en navegador)');
} else {
  console.log('si pide login, completa el OAuth en el navegador y el publish continúa');
  must(
    sh('pnpm --dir npm publish --no-git-checks --access public'),
    'publish falló (¿OAuth incompleto? reintenta el mismo comando)',
  );
  console.log(`\nOK ${SCOPE}@${VER} publicado`);
}
