const fs = require('fs');
const path = require('path');
const https = require('https');

const VERSION = 'v1.0.0';
const REPO = 'carlos-burelo/wyrm';
const BIN_DIR = path.join(__dirname, '..', 'vendor');
const BIN_PATH = path.join(BIN_DIR, 'wyrm.exe');

if (!fs.existsSync(BIN_DIR)) {
  fs.mkdirSync(BIN_DIR, { recursive: true });
}

const url = `https://github.com/${REPO}/releases/download/${VERSION}/wyrm-x86_64-pc-windows-msvc.exe`;

console.log(`[wyrm] Descargando binario nativo de Windows (${VERSION})...`);

function download(fileUrl) {
  https.get(fileUrl, (response) => {
    if (response.statusCode === 302 || response.statusCode === 301) {
      download(response.headers.location);
    } else if (response.statusCode === 200) {
      const file = fs.createWriteStream(BIN_PATH);
      response.pipe(file);
      file.on('finish', () => {
        file.close();
        console.log('[wyrm] Instalación nativa completada exitosamente.');
      });
    } else {
      console.error(`[wyrm] Error al descargar binario: HTTP ${response.statusCode}`);
      process.exit(1);
    }
  }).on('error', (err) => {
    fs.unlink(BIN_PATH, () => {});
    console.error(`[wyrm] Error de red: ${err.message}`);
    process.exit(1);
  });
}

download(url);