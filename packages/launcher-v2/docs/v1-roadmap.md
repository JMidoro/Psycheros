# launcher-v2 — v1.0 roadmap (SHIPPED)

> **Status: complete.** Every item below shipped between 2026-05-19 and
> 2026-05-19, in 11 staging commits from `0620ecf` (foundation) through the
> documentation pass. Per-item descriptions of what shipped live in
> [`packages/launcher-v2/CHANGELOG.md`](../CHANGELOG.md) under `[Unreleased]`.
> This file is preserved as the historical artifact of the planning + execution
> — what was scoped, what was decided, what shipped. Future revisions track via
> CHANGELOG entries rather than a rolling roadmap.

**Original status note (2026-05-19):** comprehensive scope-and-roadmap pass. The
"v1.0 shipping bar" is everything below — no items deferred to a later release
except items with **real external dependencies** (cross-platform hardware, paid
certificates the operator has declined). Each scope-line is part of the same
shipping target; order of execution is fine, deferral is not.

Cross-reference for fresh sessions: this doc + the package CLAUDE.md + the
memory file at `~/.claude/projects/<project>/memory/` should be enough to pick
up cold.

## 0. Premise

Ship a `.dmg` that a non-technical user can download, double-click (right-click
→ Open for unsigned Gatekeeper), and end up with a working persistent Psycheros
entity they can manage without ever touching a terminal. That means install,
daily use, update, recovery when broken, backup, restore, and uninstall — all
in-app.

## 1. State of the world after 2026-05-19's body of work

What's shipped to `Psycheros-staging/main`:

| Commit    | Subject                                                                        |
| --------- | ------------------------------------------------------------------------------ |
| `a7460cb` | fix(launcher-v2): propagate wizard inputs to psycheros's general-settings.json |
| `76cba97` | docs(launcher-v2): refresh for git-clone source model + manual-mode flow       |
| `feee755` | feat(launcher-v2): source-as-git-clone, manual mode, live log panel            |
| `2b84cbf` | fix(mcp): build entity-core argv as an array, not a space-joined string        |

What's working end-to-end:

- Install autostart / Install manual on a fresh macOS box.
- Universal Start / Stop daemon in both modes (mode-aware semantics).
- Uninstall with confirm modal.
- Source provisioning via `git clone --depth 1 --branch psycheros-v*` pinned to
  the latest semver-sorted public tag.
- Background tag-tracked update detection (~3h poll) with in-window toast
  injection + manager-card banner.
- One-click `apply_source_update` (fetch + reset to FETCH_HEAD + warm cache +
  supervisor.restart + stamp new tag in config).
- Live daemon log tail (1.5s poll, INFO/WARN/ERROR coloring, auto-scroll, Clear
  button).
- First-run wizard (entity name + user name + IANA timezone dropdown, bootstrap
  progress ticker).
- Wizard inputs propagated to `<data>/.psycheros/general-settings.json` so
  psycheros's init substitutes them and the scheduler picks up the timezone
  (verified:
  `[Memory] Timezone-aware scheduling: daily
  summary at 5:00 America/Los_Angeles`).
- Loud action-feedback UX (card-level progress banner + per-button busy state +
  sibling-disable mis-click protection).
- In-app confirm modal replacing window.confirm (Tauri 2 blocks the native one
  silently).
- Gatekeeper workaround documented in README.

## 2. Known psycheros-side bug we surfaced (not launcher work)

On a fresh install where entity-core has no stored identity, the daemon's
`[MCP] Pulled identity from entity-core` line runs AFTER psycheros's `[Init]`
step has templated identity files from
`<source>/packages/psycheros/templates/identity/` with `{{entityName}}`
substituted. The pull treats entity-core's empty state as authoritative and
wipes the local files. Net result: empty
`<data>/identity/{self,user,relationship,custom}/` directories even when the
wizard correctly seeded the entity name.

Fix lives in psycheros, not launcher. Options for the fix:

- Treat empty-entity-core as "no opinion" rather than authoritative deletion in
  the pull path.
- Push templated identity to entity-core on first init, then pull.
- Skip pull entirely on first daemon boot (detect via "no prior successful MCP
  connection in this install").

This is item **§5.10** in the launcher work list — it's the only non-launcher
item but it's in scope for the same shipping target.

## 3. Architectural decisions locked in (do not revisit)

- **Source provisioning: git clone of the public mirror at the latest
  `psycheros-v*` tag.** Not embedded tarball. Not rolling-main. Not HEAD. Only
  tagged releases are user-visible updates.
  ([source-provisioning.md](source-provisioning.md))
- **OS supervision, not launcher-owned process.** Closing the launcher never
  stops the daemon. Closing the daemon (uninstall) is a deliberate manager-card
  action with confirm. ([architecture.md](architecture.md))
- **Two install modes: autostart and manual.** Mode is persisted in
  `LauncherConfig.daemon_mode`. Universal Start/Stop work in both; the semantic
  difference is whether the daemon comes back at next login.
- **Unsigned by deliberate decision.** No Apple Developer ID. The Gatekeeper
  right-click → Open dance is documented in README. Code signing is the one
  legitimately-deferred item — paid cert the operator has declined; revisit if
  user count grows.
- **No native OS notifications.** Replaced earlier with an in-window
  webview-eval-injected toast (operator preference against native banners).
- **In-app modal, not window.confirm.** Tauri 2 silently blocks native confirm;
  never use it.
- **Migration story uses Psycheros's own export/import endpoints**, not a
  parallel launcher-side script. ([migration.md](migration.md))

## 4. Ownership / separation of concerns

The bright line:

| Concern                                                   | Owner     | Why                        |
| --------------------------------------------------------- | --------- | -------------------------- |
| `.app` shell, sidecar Deno, source provisioning           | launcher  | Filesystem deployment      |
| OS supervisor integration                                 | launcher  | Filesystem deployment      |
| Install / start / stop / restart / uninstall              | launcher  | Lifecycle                  |
| Source updates (detection + apply)                        | launcher  | Lifecycle                  |
| Shell-binary updates (Tauri plugin updater)               | launcher  | Lifecycle                  |
| Health monitoring                                         | launcher  | "Is the deployment OK?"    |
| Data-dir management (wipe / backup / restore at FS level) | launcher  | Daemon may be down         |
| Diagnostics (paths, versions, state)                      | launcher  | Support                    |
| Self-repair (when daemon is wedged)                       | launcher  | Daemon may not be running  |
| Migration hooks for breaking changes                      | launcher  | Coordinates with daemon    |
| Entity identity content (self.md etc.)                    | psycheros | The entity IS its identity |
| LLM/provider config                                       | psycheros | Per-entity                 |
| Tool config (API keys, enable/disable)                    | psycheros | Per-entity                 |
| Memory / RAG / consolidation                              | psycheros | Per-entity                 |
| Discord / Pulse / Vault / Custom Tools                    | psycheros | Per-entity                 |
| Chat history / message UI                                 | psycheros | Per-entity                 |

**Rule for the fuzzy items** (`general-settings.json` fields — entityName /
userName / timezone): functionally owned by psycheros. The launcher's wizard
captures them ONCE at first-run as a convenience and writes straight to
psycheros's settings file. **The launcher should not cache them in
`LauncherConfig` afterward** — single source of truth in psycheros's data dir,
read on demand if the launcher needs to display them. This eliminates drift.

## 5. Work list — comprehensive scope, ordered by dependency

Order is dependency-respecting (later items build on earlier). All in scope for
the same shipping target. **Items are not deferrable except where explicitly
marked "external dep — out of scope."**

### Foundation

1. **Remove cached wizard fields from `LauncherConfig`.** Drop `entity_name`,
   `user_name`, `timezone` from the struct. Wizard writes directly to
   `<data>/.psycheros/general-settings.json` via the existing seed function.
   Anywhere the launcher displays the entity name (e.g., manager card title,
   "Setting up Atlas…" copy) reads from general-settings.json on demand.
2. **Wizard pre-fill from existing general-settings.json.** If a user somehow
   ends up in the wizard again (e.g., reinstall after wipe), pre-fill from
   whatever's currently in general-settings.json so they don't have to retype.

### Manager-card additions

3. **Diagnostics card** in the manager. View-only display of:
   - Launcher version (from `CARGO_PKG_VERSION`)
   - Psycheros source version (from `config.bundled_source_version`)
   - Daemon state + PID + last exit status
   - Mode (autostart / manual)
   - Data dir path with "Open in Finder" button
   - Source dir path with "Open in Finder" button
   - Disk usage of data dir (recursive size)
   - Network/upstream URL the launcher tracks
4. **Settings card** in the manager. Show current values of entity_name /
   user_name / timezone / port (read from psycheros's general-settings.json +
   the daemon's plist). Don't edit them in the launcher — provide an "Edit in
   Psycheros (admin UI)" button that navigates the webview to psycheros's admin
   page.
5. **Log panel enhancements.** Add filter toggles (INFO / WARN / ERROR), search
   box, "Save to file" / "Copy all" button. Also: a way to load logs from prior
   daemon runs (the stderr log is append-only, so this is basically scroll-back;
   for older runs, read the log file from byte 0 in chunks).

### Data management

6. **One-click backup.** Calls psycheros's `POST /api/admin/entity-data/export`
   endpoint, streams the zip to the user's `~/Downloads/` with a timestamped
   filename. Shown as a "Back up Psycheros" button in the manager card (always
   available when daemon is Running).
7. **One-click restore.** File picker → `POST /api/admin/entity-data/import`.
   Daemon restarts post-import. Visible as "Restore from backup" in the manager
   card.
8. **Data wipe / factory reset** with confirm modal. Two confirms (the modal + a
   typed-confirm "type ATLAS to confirm" for true destructive operations —
   pattern from GitHub). Stops daemon → wipes `<data>/data/` → optionally wipes
   `<data>/source/` too → user can then re-run first-run flow.
9. **"Re-init Psycheros"** self-repair button. Uninstall daemon → wipe source
   dir + identity dir → reinstall. Useful when the source clone is corrupted or
   when the user wants a clean re-templating after entity-core's identity store
   has settled.

### Self-repair affordances

10. **Fix the psycheros MCP-pull-from-empty bug.** Lives in
    `packages/psycheros/src/mcp-client/mod.ts`. Make pullIdentity treat an empty
    entity-core identity store as "no opinion" rather than authoritative wipe.
    Without this, item §3 in §1 is user-visible-broken on every fresh install.
11. **MCP-down surface in the log panel.** Detect the "MCP error -32000:
    Connection closed" pattern + the "Falling back to local files mode" line;
    render a card-level warning in manager view with a "Restart daemon" button.
12. **Crashloop detection.** If daemon transitions Installed → Running →
    Installed within N seconds repeatedly, render a persistent warning ("Atlas
    keeps crashing — see logs") with a "Re-init Psycheros" button.
13. **Port-conflict detection.** If install reports
    `port 3000
    already in use` (some other process bound it), surface a
    clear error explaining what's bound + how to free it. (Detect via
    `lsof -i :3000` or similar on macOS.)
14. **`git` missing remediation.** Already have the error message; add a
    "Install Xcode Command Line Tools" affordance that runs
    `xcode-select --install` for the user.

### Update lifecycle expansion

15. **Mode switching post-install.** Manager card surface to switch autostart ↔
    manual without uninstall/reinstall. Implementation: overwrite the plist with
    the new mode's content + reload.
16. **Update channel selection.** Two channels: stable (`psycheros-v*`) and beta
    (`psycheros-beta-v*` or similar). User picks in settings; default is stable.
    Affects what tag pattern the update watcher queries.
17. **Pin to specific version / roll back update.** Manager card button to
    choose a specific tag from the list of available `psycheros-v*` tags. Useful
    for testing + recovering from a bad release.
18. **Update history viewer.** Track applied updates in `config.update_history`
    with timestamps + tag names. Show in diagnostics card.
19. ~~**Wire `tauri-plugin-updater` for shell-binary updates.**~~ **MOVED to
    "out of scope — legitimate deferrals" 2026-05-19.** Misjudged in the
    original plan: there's no meaningful "wiring in place" without a real
    Ed25519 keypair + a manifest endpoint
    - a CI signing step. Placeholders crash the plugin at startup (it refuses to
      deserialize a null config), so a "no-op wiring" state literally doesn't
      exist. The plugin's Cargo dep stays in place; the four-step maintainer
      checklist lives in `src/lib.rs` next to where the `.plugin()` call
      belongs. Pairs naturally with the code-signing deferral — same external
      dep (paid + keygen work not yet done).

### Migration framework (future-proof for breaking changes)

20. **Migration runner.** When `apply_source_update` fetches a new tag, before
    restarting the daemon, check if the cloned source has a
    `migrations/<from-version>-to-<target-version>.ts` file. If yes, run it via
    `deno run -A migrations/<file>.ts <data_dir>`. Stream output to the update
    progress ticker.
21. **Snapshot before update.** Before applying any update, take a snapshot of
    `<data>/.psycheros/` (DB + settings) into
    `<data>/.snapshots/<timestamp>-pre-<tag>/`. Provides rollback target.
22. **Rollback affordance** in update history viewer. Pick a prior snapshot,
    click "Roll back to this state" → confirm modal → daemon stops → snapshot
    restored → daemon restarts on the matching tag.

### Branding + release infrastructure

23. **CI release job for launcher-v2.** New `.github/workflows/` job that builds
    the `.dmg` per-triple, uploads to GitHub Releases. Replaces the existing v1
    job at the right time.
24. **Real icon set.** The operator provides a 1024² source PNG; we generate the
    icon set via `cargo tauri icon`. Replaces the placeholder violet squares.
25. **User-facing README.** Currently dev-flavored. Replace with a user-focused
    install/use/troubleshoot doc; the dev-flavored content moves to a separate
    CONTRIBUTING.md.

### Documentation pass

26. **Operations runbook** at `docs/runbook.md`. Common failure modes + how to
    recover: daemon won't start, MCP won't connect, port conflict, source
    corrupted. Linked from the diagnostics card.
27. **`v1-roadmap.md` (this file)** marked complete + archived in favor of
    CHANGELOG.md entries.

### Out of scope — legitimate deferrals

- **Code signing / notarization** — paid cert the operator has declined.
  Documented in README.
- **`tauri-plugin-updater` initialization** — needs the same kind of external
  work (keygen + signed publishes) that code signing needs. See §5.19 above for
  the four-step maintainer checklist.
- **Real icon set** — blocked on a 1024² source PNG from the design side. Two-
  minute job once the PNG lands; see CHANGELOG "Known gaps."
- **Linux + Windows supervisor implementations** — requires switching to those
  machines; cross-platform work resumes there.
- **Custom port configuration** — leave at 3000. Advanced users edit config.json
  manually. Adding UI for this isn't worth the configuration surface area.

## 6. Execution order

The order in §5 IS the recommended order. Foundation items (1-2) first, then
visible manager-card additions (3-5), then data management (6-9), then
self-repair (10-14), then update lifecycle (15-22), then release infrastructure
(23-25), then docs (26-27). The psycheros-side fix (10) can run in parallel with
launcher work since they don't touch the same files.

Estimated total effort: 4-6 focused days of work. The wedge that unblocks
shipping is items 1, 6, 7, 8, 10, 23 — those alone are the minimum coherent
product. The rest brings the launcher to "feels finished" rather than "feels
minimal."

## 7. After this revision ships

Tag `launcher-v0.3.0` (or whatever scheme is current). Build a real `.dmg`. Hand
to 2-3 trusted users. Collect feedback on user-visible gaps that the
gap-inventory missed. Iterate on UX based on real-user friction, not
internal-team imagination.

Cross-platform supervisors (Linux systemd-user, Windows Task Scheduler) are the
next phase after macOS v1.0 ships to humans.
