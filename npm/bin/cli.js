#!/usr/bin/env node

const fs = require('fs');
const { spawn } = require('child_process');
const path = require('path');

const binPath = path.join(__dirname, '..', 'vendor', 'wyrm.exe');

if (!fs.existsSync(binPath)) {
  console.error(
    '[wyrm] Falta vendor/wyrm.exe. Reinstala (npm i -g @carlos-burelo/wyrm) o compila local con `npm run build`.'
  );
  process.exit(1);
}

const child = spawn(binPath, process.argv.slice(2), {
  stdio: 'inherit',
  windowsHide: true,
});

child.on('error', (err) => {
  console.error(`[wyrm] No se pudo lanzar el binario: ${err.message}`);
  process.exit(1);
});

child.on('exit', (code) => {
  process.exit(code || 0);
});
