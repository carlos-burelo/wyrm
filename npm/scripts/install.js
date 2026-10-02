// Postinstall de wyrm: deja vendor/wyrm.exe listo.
// - Versión desde package.json (la sincroniza el workflow de release).
// - Salta si ya hay binario local (npm run build) o WYRM_SKIP_DOWNLOAD.
// - Verifica SHA256 del asset .sha256 cuando existe.
const fs = require('fs');
const path = require('path');
const https = require('https');
const crypto = require('crypto');

const REPO = 'carlos-burelo/wyrm';
const BIN_DIR = path.join(__dirname, '..', 'vendor');
const BIN_NAME = 'wyrm.exe';
const BIN_PATH = path.join(BIN_DIR, BIN_NAME);

const TARGETS = {
  'win32-x64': 'x86_64-pc-windows-msvc',
};

function pkgVersion() {
  const { version } = require('../package.json');
  if (!version) throw new Error('package.json sin version');
  return String(version).replace(/^v/, '');
}

function targetTriple(platform = process.platform, arch = process.arch) {
  const triple = TARGETS[`${platform}-${arch}`];
  if (!triple) {
    throw new Error(
      `Plataforma no soportada: ${platform}-${arch} (wyrm v1 solo publica win32-x64)`
    );
  }
  return triple;
}

function assetName(triple) {
  return `wyrm-${triple}.exe`;
}

function download(url, dest, redirects = 5) {
  return new Promise((resolve, reject) => {
    https
      .get(url, (res) => {
        const { statusCode, headers } = res;
        if ((statusCode === 301 || statusCode === 302) && headers.location) {
          if (redirects <= 0) return reject(new Error('demasiadas redirecciones'));
          res.resume();
          return resolve(download(headers.location, dest, redirects - 1));
        }
        if (statusCode !== 200) {
          res.resume();
          return reject(new Error(`HTTP ${statusCode} en ${url}`));
        }
        const file = fs.createWriteStream(dest);
        res.pipe(file);
        file.on('finish', () => file.close(resolve));
        file.on('error', reject);
      })
      .on('error', reject);
  });
}

function sha256File(p) {
  return new Promise((resolve, reject) => {
    const h = crypto.createHash('sha256');
    fs.createReadStream(p)
      .on('data', (d) => h.update(d))
      .on('end', () => resolve(h.digest('hex')))
      .on('error', reject);
  });
}

async function install() {
  if (process.env.WYRM_SKIP_DOWNLOAD) {
    console.log('[wyrm] WYRM_SKIP_DOWNLOAD=1: salto la descarga.');
    return;
  }
  if (fs.existsSync(BIN_PATH)) {
    console.log('[wyrm] Binario local existente, salto la descarga.');
    return;
  }
  const version = pkgVersion();
  const triple = targetTriple();
  const base = `https://github.com/${REPO}/releases/download/v${version}`;
  const asset = assetName(triple);
  fs.mkdirSync(BIN_DIR, { recursive: true });

  console.log(`[wyrm] Descargando ${asset} v${version}...`);
  try {
    await download(`${base}/${asset}`, BIN_PATH);
  } catch (e) {
    try {
      fs.unlinkSync(BIN_PATH);
    } catch (_) {}
    throw new Error(
      `No se pudo descargar el binario (${e.message}). ¿Existe el release v${version}?`
    );
  }

  try {
    await download(`${base}/${asset}.sha256`, `${BIN_PATH}.sha256`);
    const expected = fs.readFileSync(`${BIN_PATH}.sha256`, 'utf8').trim().toLowerCase();
    const actual = await sha256File(BIN_PATH);
    if (expected !== actual) {
      fs.unlinkSync(BIN_PATH);
      throw new Error('SHA256 no coincide: binario corrupto o manipulado.');
    }
    fs.unlinkSync(`${BIN_PATH}.sha256`);
    console.log('[wyrm] SHA256 verificado.');
  } catch (e) {
    if (/corrupto|manipulado/.test(e.message)) throw e;
    console.warn(`[wyrm] Aviso: sin checksum (${e.message}), continúo sin verificar.`);
  }
  console.log('[wyrm] Instalación nativa completada.');
}

if (require.main === module) {
  install().catch((e) => {
    console.error(`[wyrm] ${e.message}`);
    process.exit(1);
  });
}

module.exports = { pkgVersion, targetTriple, assetName, REPO, BIN_PATH };
