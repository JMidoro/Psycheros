---

### macOS

Download `Psycheros_<version>_aarch64.dmg`, double-click to mount, drag the
Psycheros app into `Applications/`.

The build is **unsigned** by deliberate decision. macOS Gatekeeper will refuse
to open it on first launch with "can't be verified." The dance to bypass this is
one-time per install:

1. Right-click the Psycheros app in Applications → **Open**.
2. The OS shows a confirmation dialog with an **Open** button (vs. only
   **Cancel** in the default double-click flow).
3. Future launches work normally.

Once installed, the app supervises a persistent Psycheros daemon via launchd —
closing the window doesn't stop me. See the in-app Diagnostics card for paths
and the Settings card for entity config.

`Psycheros.app.tar.gz` is the auto-updater bundle format — not useful for
first-install; it's consumed by `tauri-plugin-updater` when the launcher checks
for shell-binary updates.

### Windows

Download `Psycheros_<version>_x64_en-US.msi` and run it.

Windows SmartScreen will flag the installer as unrecognized because it is
**unsigned**. Click **More info** → **Run anyway** to proceed. This is one-time
per version.

The installer places Psycheros in `C:\Program Files\Psycheros\`. At the end of
the wizard, the launcher registers a Task Scheduler job that supervises the
daemon — closing the launcher window or the system-tray icon does **not** stop
the daemon. The daemon starts at login and restarts automatically on crash.

`Psycheros_<version>_x64-setup.exe` is the NSIS-based alternative installer with
the same content — use whichever you prefer.
