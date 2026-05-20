# Service supervisors

Per-OS implementations of
[`ServiceSupervisor`](../src-tauri/src/supervisor/mod.rs). All three platforms
implement the same trait surface; the launcher's UI and command handlers never
branch on OS.

## The trait

```rust
trait ServiceSupervisor: Send + Sync {
    // Registration — dual-mode (autostart vs manual). Both immediately
    // start the daemon as a side effect.
    fn install_autostart(&self, cfg: &DaemonConfig) -> Result<(), SupervisorError>;
    fn install_manual(&self, cfg: &DaemonConfig) -> Result<(), SupervisorError>;
    fn uninstall(&self) -> Result<(), SupervisorError>;

    // State queries.
    fn is_installed(&self) -> bool;   // service definition on disk
    fn is_loaded(&self) -> bool;      // record active in the OS supervisor

    // Lifecycle.
    fn start_daemon(&self) -> Result<(), SupervisorError>;
    fn stop_daemon(&self) -> Result<(), SupervisorError>;
    fn restart(&self) -> Result<(), SupervisorError>;

    // Reporting surface.
    fn log_paths(&self) -> Vec<PathBuf>;
    fn label(&self) -> &str;
    fn query_runtime_info(&self) -> RuntimeInfo { RuntimeInfo::default() }
}
```

Every method is **idempotent**: calling `install_*` when already installed must
succeed (overwriting the plist/unit); `uninstall` when not installed must
succeed; `start_daemon`/`stop_daemon` are no-ops in the already-target state.
This makes the manager UI robust against inconsistent on-disk state (e.g. plist
exists but isn't loaded after a crashloop unload).

The autostart-vs-manual split affects only what gets written to the service
definition. After install, both modes accept the same `start_daemon` /
`stop_daemon` / `restart` calls — the manager's Start and Stop buttons work
identically in either mode. The semantic difference shows up at **login time**
(autostart re-launches; manual stays off) and after a crash (autostart restarts
via `KeepAlive`; manual does not).

`query_runtime_info` is the only method with a default impl — it returns the
supervisor's best-effort PID + last-exit-status, and the trait defaults to "no
info" so platforms whose supervisors can't expose those cheaply don't have to
implement it.

## macOS — launchd (full impl)

[`supervisor/launchd.rs`](../src-tauri/src/supervisor/launchd.rs)

- **Where:** `~/Library/LaunchAgents/ai.psycheros.daemon.plist`
- **Domain:** user agent (loads at user login, not boot)
- **Privilege:** none required, ever (no sudo, no auth prompt)
- **Autostart mode:** `RunAtLoad=true` + `KeepAlive=true` — launches at login
  and revives on crash.
- **Manual mode:** `RunAtLoad=false`, `KeepAlive` omitted — only runs when the
  user explicitly hits Start; stays off across login/logout cycles until they
  hit Start again.
- **Logs:** flat files at `StandardOutPath` / `StandardErrorPath` —
  `<data_dir>/logs/daemon.stdout.log` and `daemon.stderr.log`
- **Status check:** `launchctl list <label>` exit code (0 = loaded, 113 = not
  loaded). The stdout text format varies across macOS versions; the exit code is
  stable.

### Plist contract

The launcher hand-rolls the plist XML (no plist crate dep) because the surface
is small and the format is stable. EnvironmentVariables block sets
`PSYCHEROS_DATA_DIR`, `PSYCHEROS_PORT`, `PSYCHEROS_ENTITY_CORE_DATA_DIR`, plus
`HOME` and `PATH` (launchd starts processes with no shell context, so PATH must
be set explicitly or `deno` won't be findable for the MCP subprocess spawn).

### Stop semantics

`launchctl stop <label>` is a no-op against `KeepAlive=true` (autostart-mode)
daemons — launchd revives the process within ~2 seconds. The supervisor's
`stop_daemon` therefore uses session-scoped `launchctl unload` (no `-w`), which
detaches the service for the current login session without flipping the
persistent enable state. The autostart-mode daemon comes back at next login; the
manual-mode daemon stays off because nothing tells launchd to re-load it. From
the user's perspective both surfaces share one Stop button with mode-aware copy
explaining what "Stop" means right now.

`uninstall` uses `launchctl unload -w` to flip the persistent enable state off
and remove the plist file in one go.

## Linux — systemd user unit (stub)

[`supervisor/systemd.rs`](../src-tauri/src/supervisor/systemd.rs)

- **Where:** `~/.config/systemd/user/psycheros.service`
- **Privilege:** user-level; one-time `sudo loginctl enable-linger $USER` for
  the daemon to survive logout (see below)
- **Restart on crash:** `Restart=on-failure` with `RestartSec=2`
- **Start at login:** `WantedBy=default.target` + `systemctl --user enable`
- **Logs:** systemd journal — `journalctl --user -u psycheros.service`
- **Status check:** `systemctl --user is-enabled psycheros.service` exit 0 =
  enabled

### Implementation notes for the next implementer

1. Write the unit file to `~/.config/systemd/user/`.
2. Run `systemctl --user daemon-reload` after each write so systemd picks up the
   new file.
3. `systemctl --user enable --now psycheros.service` to register and start in
   one step.
4. `is_loaded()`: shell out to
   `systemctl --user is-enabled
   psycheros.service`; parse exit code.
5. `uninstall()`: `systemctl --user disable --now psycheros.service` then `rm`
   the unit file + another `daemon-reload`.

### Logs aren't files

Unlike launchd, systemd captures stdout/stderr into the journal, not flat files.
The manager's "View logs" affordance on Linux must shell out to
`journalctl --user -u psycheros.service --since "1 hour ago"` rather than
tailing files. `log_paths()` returns an empty vec on Linux; the manager checks
for that and renders a journalctl-based view instead.

### The lingering caveat

By default, systemd user services stop when the user's last login session ends —
i.e., the daemon dies when the user logs out. That violates the "persistent
entity" model.

The fix is `loginctl enable-linger $USER`, which keeps user services alive
across sessions. This is the **only** sudo step in the entire Linux launcher
flow. Two design choices:

- **Document it (current plan).** First-run wizard tells the user to paste a
  one-liner into a terminal. Single command, clear purpose, no app-managed
  escalation. The downside is users have to do it manually.
- **Fall back to `~/.config/autostart/<file>.desktop`** when linger isn't
  available — gives "starts at login" but loses crash-restart (XDG autostart
  fires once and doesn't supervise).

We default to documenting linger. The autostart-desktop fallback is worth
considering for users who refuse the sudo prompt, but it's secondary.

## Windows — Task Scheduler (stub)

[`supervisor/task_scheduler.rs`](../src-tauri/src/supervisor/task_scheduler.rs)

- **Where:** Task Scheduler — task name `Psycheros`
- **Privilege:** user-level task (no admin needed)
- **Restart on crash:** task settings — `RestartCount=3`, `RestartInterval=PT5S`
- **Start at login:** trigger "At log on" + "Any user"
- **Logs:** no journal equivalent — daemon writes to flat files via a wrapper
  script
- **Status check:** `schtasks /query /tn Psycheros /fo csv /nh` exit 0 =
  registered

### Implementation notes for the next implementer

1. Use PowerShell's `Register-ScheduledTask` cmdlet (cleaner than `schtasks.exe`
   for complex trigger/action setups).
2. Action: `<deno_path> run -A src\main.ts` with working directory
   `<source_dir>` and environment variables via the task properties.
3. To capture stdout/stderr to flat files, the action needs a wrapper: create a
   small PowerShell launcher that does
   `deno.exe run -A src\main.ts 1> stdout.log 2> stderr.log` and point the task
   action at it.
4. `is_loaded()`: shell out to `schtasks /query /tn Psycheros`.
5. `uninstall()`: `schtasks /delete /tn Psycheros /f`.

### Weaker supervision than launchd/systemd

Task Scheduler's restart-on-failure is genuinely less robust:

- No equivalent to launchd's crash-loop throttling (after N rapid crashes,
  launchd stops trying; Task Scheduler keeps trying forever per the
  RestartCount, then gives up silently).
- "Failure" is defined narrowly (non-zero exit code). A process that hangs
  without exiting is not considered failed by Task Scheduler.

The manager surface should poll daemon status more aggressively on Windows (e.g.
every 1s instead of 2s) and surface "daemon stopped" states with manual-restart
affordances more prominently.

### SmartScreen

Unsigned `.exe` and `.msi` files trigger SmartScreen warnings on first run. The
documented workaround: right-click → Properties → Unblock, then "More info → Run
anyway." Same posture as macOS Gatekeeper. See [`release.md`](release.md).

## Cross-platform integration testing

Once Linux and Windows impls land, the integration test surface should exercise
the trait contract for each:

- Install when not installed → loaded, daemon running within timeout
- Install when already installed → idempotent (no error, still loaded)
- Uninstall when installed → unloaded, no orphan processes
- Uninstall when not installed → idempotent (no error)
- `is_loaded` matches install/uninstall state
- `log_paths()` returns sensible values (files on macOS/Windows, empty on Linux
  where journalctl is used)

Per-OS specifics (plist content, unit file format, task settings) are tested via
golden-file comparisons of the rendered output, not via running the real
supervisor.
