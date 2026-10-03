// Puerta de despliegue: el fallo de v1.0.1 fue un check idempotente contra
// `wyrm` sin scope (paquete ajeno) que hizo skip del publish real.
// Decisión: CI nunca publica a npm (sin token); el publish es manual.
// Este archivo valida que CI siga sin publicar, versiones sync, tarball
// y smoke del binario/wrapper.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {
  NPM_DIR,
  WORKFLOW,
  cargoVersion,
  findWyrmBin,
  npmVersion,
  runNode,
  runPnpm,
  runWyrm,
} from './helpers.mjs';

const yml = () => fs.readFileSync(WORKFLOW, 'utf8');

test('workflow existe', () => {
  assert.ok(fs.existsSync(WORKFLOW), 'falta .github/workflows/release.yml');
});

test('CI no publica a npm: sin publish, sin tokens, sin registry', () => {
  const y = yml();
  assert.ok(!y.includes('npm publish'), 'CI no debe publicar');
  assert.ok(!y.includes('pnpm publish'), 'CI no debe publicar');
  assert.ok(!y.includes('secrets.NPM_TOKEN'), 'CI no debe usar tokens npm');
  assert.ok(!y.includes('NODE_AUTH_TOKEN'), 'CI no debe autenticarse al registry');
  assert.ok(!y.includes('registry-url'), 'CI no debe configurarse contra un registry');
});

test('CI valida, compila, testea e2e y sube el binario al Release', () => {
  const y = yml();
  assert.ok(y.includes('cargo test -p wyrm'), 'falta cargo test en CI');
  assert.ok(y.includes('pnpm test:e2e'), 'falta gate e2e en CI');
  assert.ok(y.includes('wyrm-x86_64-pc-windows-msvc.exe'), 'falta artifact del binario');
  assert.ok(y.includes('action-gh-release'), 'falta GitHub Release');
});

test('versiones sincronizadas: Cargo.toml == npm/package.json', () => {
  assert.equal(npmVersion(), cargoVersion(), 'desync: actualiza ambas a la versión del tag');
});

test('smoke binario: --version coincide y doctor exit 0', () => {
  const bin = findWyrmBin();
  const v = runWyrm(['--version'], { bin });
  assert.equal(v.status, 0, `--version exit!=0: ${v.stderr}`);
  assert.match(v.stdout.trim(), new RegExp(`^wyrm ${npmVersion()}`), `versión inesperada: ${v.stdout}`);
  const d = runWyrm(['doctor'], { bin });
  assert.equal(d.status, 0, `doctor exit!=0: ${d.stderr}`);
});

test('wrapper npm/bin/cli.js delega al binario', () => {
  const cli = path.join(NPM_DIR, 'bin', 'cli.js');
  const vendor = path.join(NPM_DIR, 'vendor', 'wyrm.exe');
  const r = runNode([cli, '--version']);
  if (fs.existsSync(vendor)) {
    assert.equal(r.status, 0, `cli.js exit!=0: ${r.stderr}`);
    assert.match(r.stdout.trim(), new RegExp(`^wyrm ${npmVersion()}`));
  } else {
    assert.notEqual(r.status, 0, 'sin vendor el wrapper debería fallar');
    assert.match(r.stderr + r.stdout, /Falta vendor/);
  }
});

test('pack --dry-run del wrapper pasa (pnpm)', () => {
  const r = runPnpm(['pack', '--dry-run'], { cwd: NPM_DIR });
  assert.equal(r.status, 0, `pack --dry-run falló: ${r.stderr} ${r.error ?? ''}`);
  const out = r.stdout + r.stderr;
  assert.match(out, /bin\/cli\.js/, 'tarball sin bin/cli.js');
  if (fs.existsSync(path.join(NPM_DIR, 'vendor', 'wyrm.exe'))) {
    assert.match(out, /vendor\/wyrm\.exe/, 'tarball sin vendor/wyrm.exe (run `pnpm run build`)');
  }
});
