// Ciclo de uso real sin demonio (fallback DB): init → start → list →
// status → stop → delete. Corre con ProgramData aislado, sin tocar
// C:\ProgramData\wyrm. Sin dependencias: solo node:test + assert.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { E2E_DIR, copyDir, isolatedEnv, rmrf, runWyrm } from './helpers.mjs';

const APP = `wyrm-e2e-${process.pid}`;
let workdir;
let iso;

test.before(() => {
  iso = isolatedEnv('wyrm-e2e-cli');
  workdir = fs.mkdtempSync(path.join(os.tmpdir(), 'wyrm-e2e-app-'));
  copyDir(path.join(E2E_DIR, 'fixtures', 'node-app'), workdir);
});

test.after(() => {
  try {
    runWyrm(['delete', APP, '--yes'], { env: iso.env });
  } catch {}
  rmrf(workdir);
  rmrf(iso.dir);
});

test('init genera wyrm.json del proyecto', () => {
  const r = runWyrm(['init', '--force'], { cwd: workdir, env: iso.env });
  assert.equal(r.status, 0, `init exit!=0: ${r.stderr}`);
  const eco = JSON.parse(fs.readFileSync(path.join(workdir, 'wyrm.json'), 'utf8'));
  assert.ok(Array.isArray(eco.apps) && eco.apps.length >= 1, 'wyrm.json sin apps');
});

test('start registra la app en DB aunque el demonio esté caído', () => {
  const r = runWyrm(['start', '--name', APP, '--cwd', workdir], { env: iso.env });
  assert.equal(r.status, 0, `start exit!=0: ${r.stderr}`);
  assert.match(r.stdout, new RegExp(`Registrando aplicaci.n: ${APP}`));
});

test('list --json incluye la app (daemon off)', () => {
  const r = runWyrm(['list', '--json'], { env: iso.env });
  assert.equal(r.status, 0, `list exit!=0: ${r.stderr}`);
  const rows = JSON.parse(r.stdout);
  const names = rows.map((x) => x.name);
  assert.ok(names.includes(APP), `app ausente en list: ${r.stdout}`);
});

test('status y logs responden sin tumbar el CLI', () => {
  const s = runWyrm(['status', APP], { env: iso.env });
  assert.equal(s.status, 0, `status exit!=0: ${s.stderr}`);
  const l = runWyrm(['logs', APP, '--lines', '5'], { env: iso.env });
  assert.equal(l.status, 0, `logs exit!=0: ${l.stderr}`);
});

test('stop marca STOPPED en DB sin demonio', () => {
  const r = runWyrm(['stop', APP], { env: iso.env });
  assert.equal(r.status, 0, `stop exit!=0: ${r.stderr}`);
  const list = runWyrm(['list', '--json'], { env: iso.env });
  const rows = JSON.parse(list.stdout);
  const row = rows.find((x) => x.name === APP);
  assert.ok(row, 'app desapareció tras stop');
  assert.match(row.status, /STOPPED/, `estado inesperado: ${row.status}`);
});

test('delete --yes elimina la app y list ya no la muestra', () => {
  const r = runWyrm(['delete', APP, '--yes'], { env: iso.env });
  assert.equal(r.status, 0, `delete exit!=0: ${r.stderr}`);
  const list = runWyrm(['list', '--json'], { env: iso.env });
  const rows = JSON.parse(list.stdout);
  assert.ok(!rows.some((x) => x.name === APP), 'app sigue en list tras delete');
});
