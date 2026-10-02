# Wyrm 🐉

**Wyrm** es un gestor de procesos de ultra alto rendimiento escrito en **Rust**, enfocado en **Windows Server**. Mantiene aplicaciones Node.js, Next.js, Astro y Express corriendo como Servicio de Windows nativo con Zero-Config.

- Supervisor real con `JobObjects` (`KILL_ON_JOB_CLOSE`), auto-restart con backoff y restore al arrancar.
- IPC por Named Pipe con protocolo tipado `Request/Response` NDJSON.
- CLI completa: `start/stop/restart/delete/list/status/logs/daemon/top/service`.
- TUI de primer nivel con Ratatui: lista, métricas CPU/MEM, logs en vivo, filtro y acciones.
- Logs por app en `%ProgramData%\wyrm\logs\<name>.log`.
- DB SQLite en `%ProgramData%\wyrm\wyrm.db`.

## Instalación

```powershell
npm install -g wyrm
```

O desde fuente:

```powershell
cargo build --release
```

## Quickstart

```powershell
# En tu proyecto Node (auto-detecta package.json, .next/standalone, Astro, pnpm/yarn/bun)
wyrm start

# Ver estado (funciona con o sin demonio)
wyrm list

# Demonio en foreground (dev)
wyrm daemon

# TUI interactiva
wyrm top
# o simplemente:
wyrm
```

Con demonio corriendo:

```powershell
wyrm stop mi-app
wyrm restart mi-app
wyrm status mi-app
wyrm logs mi-app --lines 100
wyrm logs mi-app --follow
wyrm delete mi-app --yes
```

Multi-app con ecosystem file:

```powershell
wyrm init              # genera wyrm.json del proyecto actual
wyrm start --all       # levanta todas las apps (wyrm.json o ecosystem.json)
wyrm start --all --file ./prod.json
```

`wyrm.json`:

```json
{ "apps": [{ "name": "web", "cwd": ".", "env": { "PORT": "3000" } }] }
```

`executable`/`args` opcionales: si faltan se auto-detectan. `cwd` relativo
al archivo. El `env` del archivo gana sobre `.env`.

Políticas por app (`policy: {...}` en `wyrm.json` o payload `START`):

```json
{
  "policy": {
    "max_restarts": 10,
    "min_uptime_secs": 5,
    "stop_timeout_secs": 5,
    "max_memory_mb": 512,
    "healthcheck_url": "http://127.0.0.1:3000/health",
    "healthcheck_secs": 30
  }
}
```

- Salir antes de `min_uptime_secs` suma crash-loop; al llegar a
  `max_restarts` la app queda `ERRORED` (requiere `wyrm restart` manual).
- `max_memory_mb` reinicia al superar la RSS; `healthcheck_url` con 3 fallos
  seguidos mata el proceso para que el supervisor lo reinicie.
- Logs rotan a 10 MiB conservando 5 históricos (`app.log.1…5`).
- No se persisten en SQLite (solo vive en ecosystem/`START`; el restore usa
  defaults). En Windows el stop es terminate (CTRL+BREAK elegante: futuro).

Diagnóstico:

```powershell
wyrm doctor   # node, demonio, servicio, DB, disco, logs + hints
```

Servicio Windows (producción):

```powershell
wyrm service install
wyrm service uninstall
```

## TUI (`wyrm top`) — mejor que `pm2 monit`

Dashboard dos paneles: lista + detalle + sparklines CPU/MEM + preview logs.
Tabs `1/2/3` o `tab` para Dashboard / Logs / Help.

| Tecla | Acción |
|---|---|
| `1/2/3`, `tab` | cambiar tab |
| `j/k` o `↑/↓` | navegar apps o scroll logs |
| `enter` / `l` | ver logs full de la app |
| `r` | restart |
| `s` / `S` | stop / start (revive STOPPED desde DB, sin cwd) |
| `o` | ciclo orden: nombre → cpu → mem → uptime → restarts |
| `d` luego `y` | delete con confirmación |
| `/` | filtrar apps o buscar en logs |
| `f` / `G` | follow on/off / ir al final en logs |
| `F` luego `y` | vaciar log actual |
| `?` | ayuda |
| `q` / `esc` | salir o volver |

Logs con colores por nivel (error rojo, warn amarillo, ok/ready verde) y
resaltado de búsqueda. Header con demonio, CPU global, MEM y conteo running.
Detalle con pid, executable, cwd, log path y gauge de MEM del sistema.

## Arquitectura

```
CLI (clap) ──Named Pipe NDJSON──> Daemon
  │                                ├─ HashMap<String, ManagedApp>
  │                                ├─ JobObject por proceso
  │                                ├─ logs a archivo (append)
  │                                └─ supervise loop cada 2s + backoff
  └─ fallback DB (SQLite) si demonio off

crates/wyrm/src:
  main.rs      bootstrap (16 líneas)
  cli/         args + start/manage/list/status/logs/init
  ecosystem/   wyrm.json multi-app
  daemon/      state + supervisor + supervise loop
  ipc/         pipe + protocol Request/Response
  store/       SQLite (db) + paths
  runtime/     inspector/process/service Windows
  logs/        read/tail/flush compartido cli+tui
  tui/         state/data/actions/events/theme/views
```

## Desarrollo

```powershell
cargo check -p wyrm
cargo test -p wyrm
cargo fmt
cargo clippy -p wyrm
```
