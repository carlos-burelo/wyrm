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

Servicio Windows (producción):

```powershell
wyrm service install
wyrm service uninstall
```

## TUI (`wyrm top`)

| Tecla | Acción |
|---|---|
| `j/k` o `↑/↓` | navegar |
| `enter` / `l` | ver logs |
| `r` | restart app |
| `s` | stop app |
| `d` luego `y` | delete con confirmación |
| `/` | filtrar (esc sale) |
| `?` | ayuda |
| `q` / `esc` | salir o volver |

Header muestra estado del demonio, CPU global y MEM usada/total. La tabla muestra CPU% y MEM por proceso.

## Arquitectura

```
CLI (clap) ──Named Pipe NDJSON──> Daemon
  │                                ├─ HashMap<String, ManagedApp>
  │                                ├─ JobObject por proceso
  │                                ├─ logs a archivo (append)
  │                                └─ supervise loop cada 2s + backoff
  └─ fallback DB (SQLite) si demonio off

crates/wyrm/src:
  main.rs      CLI + tablas
  daemon.rs    supervisor
  ipc.rs       cliente/servidor pipe
  protocol.rs  Request/Response
  process.rs   spawn con Job + logs
  db.rs        CRUD SQLite
  inspector.rs auto-detect Node/Next/Astro
  service.rs   install/uninstall + SCM dispatcher
  tui.rs       Ratatui dashboard
```

## Desarrollo

```powershell
cargo check -p wyrm
cargo test -p wyrm
cargo fmt
cargo clippy -p wyrm
```
