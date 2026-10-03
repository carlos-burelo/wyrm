// Simula uso en prod: empaqueta el wrapper, lo instala en un dir limpio
// vía pnpm (como un usuario Windows) y ejerce el CLI instalado.
// Replica el staging de vendor que hace CI (checkout sin vendor + copia
// del artifact), así el e2e valida el mismo tarball que llegará a npm.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { NPM_DIR, SCOPE, isolatedEnv, npmVersion, rmrf, runNode, runPnpm } from './helpers.mjs';

let packDir;
let prodDir;
let iso;

test.before(function () {
  packDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wyrm-e2e-pack-'));
  prodDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wyrm-e2e-prod-'));
  iso = isolatedEnv('wyrm-e2e-proddata');
});

test.after(() => {
  rmrf(packDir);
  rmrf(prodDir);
  rmrf(iso.dir);
});

test('pack genera tarball instalable', () => {
  const r = runPnpm(['pack', '--pack-destination', packDir], { cwd: NPM_DIR });
  assert.equal(r.status, 0, `pnpm pack falló: ${r.stderr}`);
  const tgz = fs.readdirSync(packDir).filter((f) => f.endsWith('.tgz'));
  assert.equal(tgz.length, 1, `tarball inesperado: ${tgz}`);
});

test('instalación limpia con pnpm + staging vendor (como CI)', () => {
  const tgz = fs.readdirSync(packDir).find((f) => f.endsWith('.tgz'));
  fs.writeFileSync(
    path.join(prodDir, 'package.json'),
    JSON.stringify({ name: 'wyrm-prod-sim', version: '0.0.0', private: true }),
  );
  const add = runPnpm(['add', path.join(packDir, tgz)], { cwd: prodDir });
  assert.equal(add.status, 0, `pnpm add tarball falló: ${add.stderr}`);

  // CI hace checkout sin vendor/ y copia el artifact: mismo staging aquí.
  const installed = path.join(prodDir, 'node_modules', ...SCOPE.split('/'));
  assert.ok(fs.existsSync(path.join(installed, 'bin', 'cli.js')), 'instalado sin bin/cli.js');
  const vendorSrc = path.join(NPM_DIR, 'vendor', 'wyrm.exe');
  assert.ok(fs.existsSync(vendorSrc), 'falta npm/vendor/wyrm.exe local para staging');
  fs.mkdirSync(path.join(installed, 'vendor'), { recursive: true });
  fs.copyFileSync(vendorSrc, path.join(installed, 'vendor', 'wyrm.exe'));
});

test('CLI instalado responde --version/--help/doctor/list', () => {
  const cli = path.join(prodDir, 'node_modules', ...SCOPE.split('/'), 'bin', 'cli.js');
  const v = runNode([cli, '--version'], { env: iso.env });
  assert.equal(v.status, 0, `--version exit!=0: ${v.stderr}`);
  assert.match(v.stdout.trim(), new RegExp(`^wyrm ${npmVersion()}`));

  const h = runNode([cli, '--help'], { env: iso.env });
  assert.equal(h.status, 0, `--help exit!=0: ${h.stderr}`);
  assert.match(h.stdout, /Usage: wyrm/);

  const d = runNode([cli, 'doctor'], { env: iso.env });
  assert.equal(d.status, 0, `doctor exit!=0: ${d.stderr}`);
  assert.match(d.stdout, /node/);

  const l = runNode([cli, 'list', '--json'], { env: iso.env });
  assert.equal(l.status, 0, `list exit!=0: ${l.stderr}`);
  assert.ok(Array.isArray(JSON.parse(l.stdout)), 'list --json no es arreglo');
});
