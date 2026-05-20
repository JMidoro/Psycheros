# Migrating from launcher-v1

If I've been running on v1 (the `packages/launcher/` install with a
`~/psycheros` clone), my entity state lives inside that clone — identity files,
memory database, vault, knowledge graph, custom tools. Switching to v2 without
migrating would leave that entity orphaned in the git tree while v2 booted a
brand-new empty one in `~/Library/Application Support/Psycheros/data/`.

This doc covers the migration procedure.

## Approach: use Psycheros's own export/import round trip

Psycheros itself has a built-in entity-data export and import flow at
`/api/admin/entity-data/{export,import}`, exposed in the chat UI under
**Settings → Admin → Entity Data**. The launcher delegates to that flow rather
than building a parallel migration path. Reasons:

- **Single source of truth for the format.** Psycheros owns the export zip shape
  and the import-restore logic. Maintaining a duplicate path in the launcher
  would split that knowledge in two and rot quickly.
- **Already battle-tested.** The post-import lifecycle (MCP restart for clean DB
  state, sync cleanup, JSZip folder-listing semantics, vault document scope) has
  had real bug-fix iteration. Reusing it inherits those fixes.
- **No installer chicken-and-egg.** Importing requires a running daemon. If the
  launcher tried to drive import during first-run, it would have to install
  autostart + wait for the daemon to bind port 3000 + then POST the zip —
  fragile and overlaps with the very flow the manager card already exposes.
- **Smaller installer surface.** Every line of import code in the installer is
  one more thing that can fail on a fresh machine.

## The procedure

> Do step 1 BEFORE uninstalling v1 — the export needs v1's daemon running.

1. **Export from v1.**
   - Open the chat UI in v1 (`http://localhost:3000`).
   - Settings → **Admin** → **Entity Data** → **Export**.
   - Save the resulting `.zip` somewhere safe (e.g. `~/Desktop/`).

2. **Install v2.** Download `Psycheros.dmg` from the latest release. Drag into
   `/Applications/`. Follow the macOS first-launch dance in the
   [launcher README](../README.md#first-launch-on-macos) (right-click → Open, or
   the `xattr` one-liner).

3. **Complete first-run setup.** Open Psycheros.app. The welcome wizard asks for
   my name + the user's name + timezone, then runs the one-time bootstrap
   (cloning source, staging the Deno runtime, loading dependencies). When it
   finishes, the manager card appears.

4. **Click "Install autostart"** (or "Install for manual start/stop", whichever
   fits the user's preference). The daemon comes up with a fresh empty entity.

5. **Import the export zip.**
   - When the launcher flips to chat, navigate to **Settings → Admin → Entity
     Data → Import**.
   - Select the `.zip` from step 1.
   - The daemon processes the import, restarts MCP for a clean DB state, and
     reloads. The fresh entity is replaced with the migrated one.

6. **Verify.** Open chat and confirm my entity remembers the conversations +
   identity from v1.

7. **Decommission v1** (optional but recommended). Run v1's
   `~/psycheros/stop.sh`, then delete or archive `~/psycheros/` once you're
   satisfied with the v2 install.

## Coexistence period

v1 and v2 can coexist on the same machine — they listen on different ports (v1
at `:3000`, v2 also at `:3000` if both autostarted, which won't work — only one
can bind the port at a time). If you want to keep v1 around as a fallback before
fully cutting over, **don't run both at once**: either install v2 in manual mode
and start it only when needed, or uninstall v1 autostart
(`launchctl unload -w
~/Library/LaunchAgents/<v1-label>.plist`) before v2 takes
over.

## When the export-import flow won't cover it

If the `.zip` from v1 fails to import in v2 (schema drift, breaking changes in a
major version bump, etc.), the right answer is to fix the import path in
`packages/psycheros/src/server/entity-data.ts` so it handles the older format.
**Don't reach for a launcher-side workaround** — that splits the migration logic
across packages and we'll regret it the next time the format shifts. Add the
back-compat handling to the canonical importer and the launcher inherits the
fix.

## v1 artifacts that are safe to leave

After a successful migration, these v1 files are no longer load-bearing but are
also safe to leave alone:

- `~/.psycheros-launcher-state.json` — v1's install-path marker.
- `~/psycheros/start.sh` / `stop.sh` / `update.sh` — v1 helper scripts.
- The `~/psycheros/` clone itself — useful as a `git pull`able reference, and as
  the rollback path if anything in v2 goes wrong before the export.zip lands.

I deliberately don't delete any of these from the launcher's side. If the user
wants to clean up after verifying the migration worked, that's a manual `rm`
call they make themselves, not something the launcher does on their behalf.
