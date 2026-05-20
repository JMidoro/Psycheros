# launcher-v2 — agent card

Tauri 2.x desktop app that installs Psycheros as an **OS-supervised service**
(launchd / systemd-user / Task Scheduler), renders the chat UI inline by
navigating the webview to the daemon's `localhost:3000`, and provides an in-app
manager surface for install/uninstall, status, logs, and updates. Replaces the
v1 `packages/launcher` HTTP-server-in-a-browser-tab shape.

First-person convention applies — see [root CLAUDE.md](../../CLAUDE.md). The
launcher itself is utility surface, not entity surface, so this convention
mainly affects user-facing copy in `frontend/` (titles, error messages), not
Rust internals.

## Commands

```bash
# Dev (from inside packages/launcher-v2/):
./scripts/setup.sh                        # one-time: stage Deno + icons
npx --yes @tauri-apps/cli@^2.0 dev        # or cargo install tauri-cli && cargo tauri dev

# Build a distributable (.app / .msi / .deb / .AppImage):
npx --yes @tauri-apps/cli@^2.0 build

# Rust gates:
cd src-tauri && cargo check
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy -- -D warnings
```

Psycheros source is NOT embedded in the launcher binary — it gets cloned at
first-run from the public repo at the latest `psycheros-v*` tag. See
[`docs/source-provisioning.md`](docs/source-provisioning.md).

This package is **not in the Deno workspace** — it's Rust + plain HTML/JS/CSS.
The root `deno.json` workspace list intentionally omits it.

## Architectural pillars (do not violate)

1. **The launcher does not own the daemon process.** It installs an OS service
   definition; the OS supervises. Closing/crashing the launcher never touches
   the daemon. See [`docs/architecture.md`](docs/architecture.md).
2. **Cross-platform via trait, not `#[cfg]` everywhere.** All daemon lifecycle
   goes through [`supervisor::ServiceSupervisor`]. The macOS impl is full;
   Linux + Windows are stubs with explicit `NotImplemented`. See
   [`docs/supervisors.md`](docs/supervisors.md).
3. **One window, two surfaces.** Chat and manager render in the same `main`
   window. `Cmd+,` toggles. The webview navigates between `tauri://localhost/`
   (manager) and `http://localhost:3000/` (chat).
4. **Navigation is driven from Rust, not JS.** Cross-origin restrictions prevent
   JS in either context from reliably navigating to the other. `webview.eval()`
   from Rust sidesteps this. See
   [`daemon::navigation`](src-tauri/src/daemon/navigation.rs).
5. **Frontend never directly polls the daemon HTTP.** It calls Rust commands;
   Rust does the TCP probe + supervisor query. This avoids webview CORS issues
   entirely.

## Module structure (`src-tauri/src/`)

```
lib.rs                    Tauri builder; entry from main.rs
main.rs                   Thin binary wrapper around lib::run()

paths.rs                  Per-OS path resolution (app data, source, deno, logs)

supervisor/
  mod.rs                  ServiceSupervisor trait + DaemonConfig + DefaultSupervisor alias
  launchd.rs              macOS: dual plist (autostart/manual) + start/stop/restart
  systemd.rs              Linux: stub
  task_scheduler.rs       Windows: stub

daemon/
  mod.rs                  Public surface — DAEMON_PORT fallback const, re-exports
  status.rs               DaemonState enum (NotInstalled / Stopped / Installed /
                          Running) + probe() — file + supervisor + HTTP /health
                          identity check (rejects non-Psycheros port owners)
  navigation.rs           webview.eval-based navigation driven by Rust

app/
  mod.rs                  spawn_status_watcher (2s poll, emits daemon-status-changed)
  state.rs                AppState (user_summoned, splash_url, last_navigated)
  menu.rs                 Native menu (Preferences = Cmd+,)
  tray.rs                 macOS menu-bar tray icon + state-aware context menu
                          (Start/Stop/View logs/Open manager/Quit), template
                          image so it auto-tints to menu-bar theme
  update_watcher.rs       3h poll of upstream tags; emits update-available;
                          injects in-window toast on rising-edge transitions
  log_tailer.rs           1.5s poll of daemon stderr; emits daemon-log-line for
                          the manager's live log panel

bundle/                   Source provisioning — clone_or_fetch_source,
                          query_latest_tag (semver), stage_bundled_deno,
                          warm_deno_cache. See docs/source-provisioning.md.
config/                   LauncherConfig + DaemonMode + load/save
http.rs                   Minimal hand-rolled HTTP/1.1 client for the localhost
                          backup/restore endpoints (no reqwest/tokio overhead)

commands.rs               #[tauri::command] surface — JS RPC entry points
```

The frontend (`frontend/`) is plain HTML/CSS/JS — no bundler. Tauri's
`withGlobalTauri: true` exposes the IPC API on `window.__TAURI__` so JS can
`invoke()` Rust commands and `listen()` for events without a build step. The
split is intentional: keep the build surface small until product needs demand
otherwise.

## State machine: daemon state

`daemon::probe()` combines three signals — plist file exists (`is_installed`),
service loaded in launchctl (`is_loaded`), and TCP port bound — into a single
`DaemonState`:

| installed | loaded | port | state          | manager card shows                       |
| --------- | ------ | ---- | -------------- | ---------------------------------------- |
| no        | no     | no   | `NotInstalled` | Install autostart \| Install manual      |
| yes       | no     | no   | `Stopped`      | Start daemon \| Uninstall                |
| yes       | yes    | no   | `Installed`    | (transient — booting or crashlooping)    |
| _         | _      | yes  | `Running`      | Back to chat \| Stop daemon \| Uninstall |

`Stopped` is distinct from `NotInstalled`: the user clicked Stop, but the
service definition is still on disk. At next login (autostart) or next manual
Start (either mode) the daemon comes back. Port-bound implies `Running`
regardless of supervisor state, so users who run `deno task start` from a
terminal still get a working chat UI.

Daemon mode (`DaemonMode::Autostart | Manual`) lives in `config.daemon_mode` and
affects the plist content (`RunAtLoad`/`KeepAlive`) at install time, plus
mode-aware copy in the manager card. `Stop` is universal — for autostart it's a
session-scoped unload (daemon back at next login); for manual it stays stopped.

## State machine: view mode

`AppState.user_summoned` is the only flag distinguishing "splash because daemon
is down" from "splash because user pressed Cmd+,":

| state        | user_summoned | what the user sees           | on daemon → Running  |
| ------------ | ------------- | ---------------------------- | -------------------- |
| Running      | false         | chat UI                      | (already there)      |
| Running      | true          | manager (user wants it)      | **stays on manager** |
| Installed    | false         | manager, "daemon starting…"  | auto-flips to chat   |
| Installed    | true          | same                         | **stays on manager** |
| NotInstalled | false         | manager, "install autostart" | auto-flips to chat   |
| NotInstalled | true          | same                         | **stays on manager** |

When daemon goes Running → not-Running, the launcher always auto-flips to the
manager (regardless of `user_summoned`) so the user has a recovery affordance
instead of a frozen chat window.

## Traps that bite

- **`window.__TAURI__` is undefined unless `withGlobalTauri: true`.** Tauri 2's
  default is `false`. Frontend will silently fail every IPC call if this isn't
  set in `tauri.conf.json`. Already set; don't remove it.
- **`window.url()` returns different URLs in dev vs production.** Dev uses a
  random local port (`http://127.0.0.1:<random>/`); production uses
  `tauri://localhost/`. We capture it once at startup into `AppState.splash_url`
  to navigate back to from any origin.
- **`location.replace(sameURL)` triggers a hard reload.** Wipes splash JS state
  and looks like a glitch. `daemon::navigation::drive` de-dupes via
  `AppState.last_navigated`.
- **Tauri's icon validator requires RGBA, not RGB.** `cargo tauri dev` panics
  with "icon is not RGBA" if you pass color type 2 (RGB). Color type 6 (RGBA)
  only. `scripts/setup.sh` generates the right format.
- **`launchctl list <label>` exits 0 = loaded, 113 = not loaded.** Parse the
  exit code, not the stdout text — the latter varies across macOS versions and
  is meant for humans.
- **`KeepAlive=true` makes "Stop" useless.** `launchctl stop` is a no-op against
  KeepAlive — the daemon comes right back. The only real off switch is
  `launchctl unload` (session-scoped, used by Stop) or `launchctl unload -w`
  (persistent, used by Uninstall).
- **`window.confirm` / `window.alert` are silently blocked in Tauri 2 webviews**
  — they return `undefined`. Don't use them. The launcher has a themed in-app
  modal (`#confirm-modal`) wired via `confirmDialog()` in
  `frontend/js/manager.js`.
- **`git reset --hard origin/<branch>` breaks for tags.** Tags don't always get
  a remote-tracking ref. Use `FETCH_HEAD` instead — works for both branches and
  tags, and peels annotated tags to their commit automatically. See
  `bundle::clone_or_fetch_source`.
- **`--progress` flag is rejected by `git reset`** (exit 129). Only pass it to
  commands that accept it (`clone`, `fetch`); don't blanket-append.
- **Annotated tag SHA ≠ commit SHA.** `query_latest_tag` returns the tag name
  (e.g. `psycheros-v0.3.3`) which is the stable identifier; the actual SHA is
  exposed only for diagnostics.

## Cross-platform considerations

| Platform | Supervisor           | Sudo needed?              | Logs                                |
| -------- | -------------------- | ------------------------- | ----------------------------------- |
| macOS    | launchd (user agent) | Never                     | Files at `<data>/logs/daemon.*.log` |
| Linux    | systemd user unit    | Once, for `enable-linger` | `journalctl --user -u psycheros`    |
| Windows  | Task Scheduler       | Never (user-level task)   | Redirected stdout files via wrapper |

The launchd impl is the reference. Other OSes follow the same trait contract but
the under-the-hood mechanics differ — see per-OS module doc comments and
[`docs/supervisors.md`](docs/supervisors.md) for the full picture.

## Deep references

| Topic                                         | Doc                                                        |
| --------------------------------------------- | ---------------------------------------------------------- |
| Overall architecture, daemon ownership model  | [docs/architecture.md](docs/architecture.md)               |
| Per-OS service supervisor design + impl notes | [docs/supervisors.md](docs/supervisors.md)                 |
| Source provisioning + bundle composition      | [docs/source-provisioning.md](docs/source-provisioning.md) |
| Frontend conventions, view modes, brand       | [docs/frontend.md](docs/frontend.md)                       |
| CI matrix, signing posture, distribution      | [docs/release.md](docs/release.md)                         |
| v1 → v2 migration story                       | [docs/migration.md](docs/migration.md)                     |
| Operations runbook (symptom → recovery)       | [docs/runbook.md](docs/runbook.md)                         |
| v1.0 roadmap (historical, shipped)            | [docs/v1-roadmap.md](docs/v1-roadmap.md)                   |
| Dev setup + build commands                    | [CONTRIBUTING.md](CONTRIBUTING.md)                         |

## Companion packages

This package lives in the [Psycheros monorepo](../../README.md). It manages the
lifecycle of the sibling [`psycheros`](../psycheros/) daemon. Psycheros source
is NOT bundled into the launcher binary — it gets cloned at first run from the
public mirror at the latest `psycheros-v*` tag. It does not manage
[`entity-loom`](../entity-loom/) — Loom is a separate utility with its own
distribution story.

The v1 [`launcher`](../launcher/) is being replaced by this package. Delete v1
once v2 reaches feature parity per [`docs/migration.md`](docs/migration.md).
