/**
 * Settings card — view-only display of psycheros's general-settings.json
 * (entity name / user name / timezone) plus the launcher-side port.
 *
 * Edits don't happen here. The "Edit in Psycheros" button switches the
 * webview back to chat (`set_view_mode("chat")`), where the user opens
 * Settings → General inside psycheros's own UI. Centralizing edits
 * in one place avoids the launcher and psycheros writing the same
 * file from different code paths — see also the §5.1 refactor that
 * dropped the launcher's cached copies of these fields.
 *
 * Triggered from the manager card's "Settings" footer button. Returns
 * to the manager card on Back.
 */

import { safeInvoke } from "./tauri-bridge.js";
import { runSourceUpdate, showCard } from "./first-run.js";
import { renderError, renderLoading, renderSections } from "./info-grid.js";

const els = {
  body: () => document.getElementById("settings-body"),
  back: () => document.getElementById("settings-back"),
  refresh: () => document.getElementById("settings-refresh"),
  editInPsycheros: () => document.getElementById("settings-edit-in-psycheros"),
};

function renderSettings(settings, diag, channel) {
  const body = els.body();
  if (!body) return;

  const installed = diag?.daemon_state !== "not-installed";

  const sections = [
    {
      heading: "Identity",
      rows: [
        {
          label: "Entity name",
          value: settings?.entityName ?? "(not yet configured)",
          plainValue: true,
        },
        {
          label: "User name",
          value: settings?.userName ?? "(not yet configured)",
          plainValue: true,
        },
      ],
    },
    {
      heading: "Locale",
      rows: [
        {
          label: "Timezone",
          value: settings?.timezone ?? "UTC",
          plainValue: true,
        },
      ],
    },
    {
      heading: "Network",
      rows: [
        {
          label: "Daemon port",
          value: String(diag?.port ?? 3000),
          plainValue: true,
        },
      ],
    },
    {
      heading: "Daemon mode",
      rows: [
        {
          label: "Current mode",
          value: diag?.daemon_mode === "manual"
            ? "Manual — runs only when started"
            : "Autostart — runs at every login",
          plainValue: true,
          action: installed
            ? {
              label: diag?.daemon_mode === "manual"
                ? "Switch to Autostart"
                : "Switch to Manual",
              async onClick() {
                const newMode = diag?.daemon_mode === "manual"
                  ? "autostart"
                  : "manual";
                const { err } = await safeInvoke("set_daemon_mode", {
                  mode: newMode,
                });
                if (err) console.warn("[launcher] set_daemon_mode:", err);
                // Re-fetch so the row reflects the new mode.
                await loadSettings();
              },
            }
            : undefined,
        },
      ],
    },
    {
      heading: "Update channel",
      rows: [
        {
          label: "Current channel",
          value: channel === "beta"
            ? "Beta — tracks psycheros-beta-v* tags"
            : "Stable — tracks psycheros-v* tags",
          plainValue: true,
          action: {
            label: channel === "beta" ? "Switch to Stable" : "Switch to Beta",
            async onClick() {
              const newChannel = channel === "beta" ? "stable" : "beta";
              const { err } = await safeInvoke("set_update_channel", {
                channel: newChannel,
              });
              if (err) console.warn("[launcher] set_update_channel:", err);
              await loadSettings();
            },
          },
        },
      ],
    },
  ];

  renderSections(body, sections);
  appendVersionPicker(body, diag?.source_version);
}

/**
 * §5.17: an extra section beneath the standard rows giving the user
 * a dropdown of every available tag on the current channel + an
 * "Install selected version" button. Lazily loads the tag list
 * (network-bound) so an offline launcher still renders the rest of
 * the settings card.
 */
function appendVersionPicker(body, currentVersion) {
  const heading = document.createElement("h3");
  heading.className = "info-grid__heading";
  heading.textContent = "Source version";
  body.appendChild(heading);

  const row = document.createElement("div");
  row.className = "info-row";

  const labelEl = document.createElement("div");
  labelEl.className = "info-row__label";
  labelEl.textContent = "Pick a tag";
  row.appendChild(labelEl);

  const wrap = document.createElement("div");
  wrap.className = "info-row__valuewrap";

  const select = document.createElement("select");
  select.className = "info-row__select";
  select.disabled = true;
  const loading = document.createElement("option");
  loading.textContent = "Loading available tags…";
  select.appendChild(loading);
  wrap.appendChild(select);

  const hint = document.createElement("div");
  hint.className = "info-row__sub";
  hint.textContent =
    "Switching version reinstalls the source clone and restarts the daemon.";
  wrap.appendChild(hint);

  row.appendChild(wrap);

  const installBtn = document.createElement("button");
  installBtn.type = "button";
  installBtn.className = "info-row__action";
  installBtn.textContent = "Install version";
  installBtn.disabled = true;
  installBtn.addEventListener("click", async () => {
    const tag = select.value;
    if (!tag) return;
    installBtn.disabled = true;
    try {
      await runSourceUpdate({ targetTag: tag });
      showCard("card-manager");
    } catch (err) {
      console.warn("[launcher] install-specific-version failed:", err);
    }
  });
  row.appendChild(installBtn);

  body.appendChild(row);

  // Populate the dropdown asynchronously — list_available_tags is
  // network-bound (git ls-remote against the public repo). UI stays
  // usable while it's in flight.
  (async () => {
    const { ok, err } = await safeInvoke("list_available_tags");
    if (err) {
      select.replaceChildren();
      const opt = document.createElement("option");
      opt.textContent = `(couldn't list tags: ${err})`;
      select.appendChild(opt);
      return;
    }
    select.replaceChildren();
    if (!Array.isArray(ok) || ok.length === 0) {
      const opt = document.createElement("option");
      opt.textContent = "(no tags available)";
      select.appendChild(opt);
      return;
    }
    for (const tag of ok) {
      const opt = document.createElement("option");
      opt.value = tag;
      opt.textContent = tag === currentVersion ? `${tag} (current)` : tag;
      if (tag === currentVersion) opt.selected = true;
      select.appendChild(opt);
    }
    select.disabled = false;
    installBtn.disabled = false;
    // Disable the button when the dropdown matches the current
    // version — installing-the-same is a no-op that just churns.
    const syncEnabled = () => {
      installBtn.disabled = select.value === currentVersion;
    };
    syncEnabled();
    select.addEventListener("change", syncEnabled);
  })();
}

async function loadSettings() {
  const body = els.body();
  if (!body) return;
  renderLoading(body, "Loading settings…");

  // Three reads — psycheros's general-settings.json (entity / user /
  // timezone), the launcher's diagnostics snapshot (port + daemon
  // mode + state), and the persisted update channel. All local IPC;
  // parallelize since they're independent.
  const [settingsRes, diagRes, channelRes] = await Promise.all([
    safeInvoke("read_general_settings"),
    safeInvoke("get_diagnostics"),
    safeInvoke("get_update_channel"),
  ]);

  if (settingsRes.err) {
    renderError(body, `Couldn't read settings: ${settingsRes.err}`);
    return;
  }
  // diag failure is non-fatal — we can still show entity name / user
  // name / timezone. Just default the port display in that case.
  if (diagRes.err) {
    console.warn("[launcher] get_diagnostics failed:", diagRes.err);
  }
  if (channelRes.err) {
    console.warn("[launcher] get_update_channel failed:", channelRes.err);
  }
  renderSettings(settingsRes.ok, diagRes.ok, channelRes.ok ?? "stable");
  syncEditInPsycherosState(diagRes.ok);
}

/**
 * "Edit in Psycheros" navigates to the chat view, where psycheros's
 * own settings UI lives. That only makes sense when the daemon is
 * actually running — otherwise the chat view is just a splash screen.
 * Disable the button (with an explanatory title) when daemon isn't
 * Running.
 */
function syncEditInPsycherosState(diag) {
  const btn = els.editInPsycheros();
  if (!btn) return;
  const running = diag?.daemon_state === "running";
  btn.disabled = !running;
  btn.title = running
    ? "Switch to the chat view and open Psycheros's Settings tab"
    : "Start Psycheros from the manager card before editing settings";
}

let wired = false;

/**
 * Open the settings card.
 *
 * @param {() => void} onBack — called when user clicks Back. Caller
 *   typically restores card-manager.
 */
export async function openSettings(onBack) {
  showCard("card-settings");

  if (!wired) {
    wired = true;
    els.refresh()?.addEventListener("click", () => loadSettings());
    els.editInPsycheros()?.addEventListener("click", async () => {
      // Switching to chat view also navigates the webview to
      // localhost:3000 (if the daemon is up). The user can then click
      // Settings inside psycheros's UI.
      const { err } = await safeInvoke("set_view_mode", { mode: "chat" });
      if (err) console.warn("[launcher] set_view_mode failed:", err);
    });
  }

  const back = els.back();
  if (back) {
    const handler = () => {
      back.removeEventListener("click", handler);
      onBack?.();
    };
    back.addEventListener("click", handler);
  }

  await loadSettings();
}
