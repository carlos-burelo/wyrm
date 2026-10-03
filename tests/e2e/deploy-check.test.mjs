// Puerta de despliegue: valida que el pipeline tag→release→npm no repita
// el fallo de v1.0.1 (check idempotente contra `wyrm` sin scope, que hizo
// skip del publish de `@carlos-burelo/wyrm@1.0.1` aunque no existía).
// Además valida sync de versiones, tarball y smoke del binario/wrapper.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {
  NPM_DIR,
  ROOT,
  SCOPE,
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

test('check idempotente apunta al paquete con scope', () => {
  const y = yml();
  assert.ok(
    y.includes(`${SCOPE}@$TAG_VERSION`) || y.includes(`${SCOPE}@${'$'}{TAG_VERSION}`),
    'publish idempotente debe consultar @carlos-burelo/wyrm@$TAG_VERSION',
  );
  const bare = y.match(/npm view\s+["']?wyrm@/g) ?? [];
  const pnpmBare = y.match(/pnpm view\s+["']?wyrm@/g) ?? [];
  assert.equal(bare.length, 0, `referencia al paquete sin scope (ajeno): ${bare}`);
  assert.equal(pnpmBare.length, 0, `referencia al paquete sin scope (ajeno): ${pnpmBare}`);
});

test('workflow valida formato de tag y hace staging del binario', () => {
  const y = yml();
  assert.ok(y.includes('^v[0-9]'), 'falta validación de tag vX.Y.Z');
  assert.ok(y.includes('vendor-staging'), 'falta staging vendor desde artifact');
  assert.ok(y.includes('vendor/wyrm.exe'), 'falta copia a npm/vendor/wyrm.exe');
  assert.ok(y.includes('test -s'), 'falta verificación binario no vacío');
});

test('workflow tiene gate de tarball antes de publicar', () => {
  const y = yml();
  assert.ok(y.includes('pack --dry-run'), 'falta gate `pack --dry-run` previo a publish');
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
