/**
 * Diagnostics card — view-only snapshot of launcher + daemon state.
 *
 * Triggered from the manager card's "Diagnostics" button. Fetches a
 * `get_diagnostics` snapshot from Rust, renders it as a labeled
 * key/value list, wires Reveal buttons (call `open_path`) for each
 * filesystem path, and offers Refresh + Back controls. Returns to the
 * manager card on Back.
 *
 * No polling — the snapshot is on-demand. Refresh rebuilds it.
 */

import { safeInvoke } from "./tauri-bridge.js";
import { runRollback, showCard } from "./first-run.js";
import {
  humanBytes,
  renderError,
  renderLoading,
  renderSections,
} from "./info-grid.js";

const els = {
  body: () => document.getElementById("diagnostics-body"),
  back: () => document.getElementById("diagnostics-back"),
  refresh: () => document.getElementById("diagnostics-refresh"),
};

/**
 * Render the daemon state with a runtime sub-line. Matches the manager
 * card's state vocabulary; adds PID / last-exit-status when the
 * supervisor reported them.
 */
function renderDaemonState(diag) {
  const stateLabel = {
    "running": "Running",
    "installed": "Service loaded · waiting for port",
    "stopped": "Stopped",
    "not-installed": "Not installed",
  }[diag.daemon_state] ?? diag.daemon_state;

  const sub = [];
  if (diag.runtime?.pid != null) sub.push(`pid ${diag.runtime.pid}`);
  if (diag.port_bound) {
    sub.push(`port ${diag.port} bound`);
  } else if (diag.daemon_state !== "not-installed") {
    sub.push(`port ${diag.port} not bound`);
  }
  if (diag.runtime?.last_exit_status != null) {
    sub.push(`last exit ${diag.runtime.last_exit_status}`);
  }

  const subLine = sub.length
    ? `<div class="info-row__sub">${sub.join(" · ")}</div>`
    : "";
  return `<div class="info-row__value info-row__value--plain">${stateLabel}</div>${subLine}`;
}

/**
 * Reveal-action factory. Wraps open_path with click-disable semantics
 * so multi-clicks don't spawn multiple finder windows.
 */
function revealAction(path) {
  return {
    label: "Reveal",
    async onClick() {
      const { err } = await safeInvoke("open_path", { path });
      if (err) console.warn("[launcher] open_path failed:", err);
    },
  };
}

function renderDiagnostics(diag, history) {
  const body = els.body();
  if (!body) return;

  const sections = [
    {
      heading: "Versions",
      rows: [
        {
          label: "Launcher",
          value: `v${diag.launcher_version}`,
          plainValue: true,
        },
        {
          label: "Psycheros source",
          value: diag.source_version ?? "(not installed)",
          plainValue: true,
        },
      ],
    },
    {
      heading: "Daemon",
      rows: [
        { label: "State", html: renderDaemonState(diag) },
        {
          label: "Mode",
          value: diag.daemon_mode === "manual" ? "Manual" : "Autostart",
          plainValue: true,
        },
        { label: "Service label", value: diag.service_label },
      ],
    },
    {
      heading: "Filesystem",
      rows: [
        {
          label: "Launcher data dir",
          value: diag.paths.launcher_data_dir,
          action: revealAction(diag.paths.launcher_data_dir),
        },
        {
          label: "Entity data dir",
          html: `<div class="info-row__value">${diag.paths.data_dir}</div>` +
            `<div class="info-row__sub">${
              humanBytes(diag.data_dir_size_bytes)
            } on disk</div>`,
          action: revealAction(diag.paths.data_dir),
        },
        {
          label: "Source dir",
          value: diag.paths.source_dir,
          action: revealAction(diag.paths.source_dir),
        },
        {
          label: "Log dir",
          value: diag.paths.log_dir,
          action: revealAction(diag.paths.log_dir),
        },
        { label: "Config file", value: diag.paths.config_path },
      ],
    },
    {
      heading: "Upstream",
      rows: [
        { label: "Repo", value: diag.upstream_repo_url },
        { label: "Tag prefix", value: diag.upstream_tag_prefix },
      ],
    },
  ];

  if (Array.isArray(history) && history.length > 0) {
    sections.push({
      heading: "Update history",
      rows: history.map((entry, idx) => {
        const row = {
          label: idx === 0 ? "Latest" : `# ${history.length - idx}`,
          html:
            `<div class="info-row__value info-row__value--plain">${entry.tag}</div>` +
            `<div class="info-row__sub">${formatHistoryDetail(entry)}</div>`,
        };
        // §5.22: rollback when a snapshot still exists. Skip the
        // most-recent (idx 0) — that IS the current state; rolling
        // "back to current" is a no-op. The first useful rollback
        // target is the entry that was applied before the current.
        if (idx > 0 && entry.snapshot_id) {
          row.action = {
            label: "Roll back",
            danger: true,
            async onClick() {
              // Switch to bootstrap card (progress ticker), run the
              // rollback, then bring the user back to the manager
              // card. The Rust command emits source-update-progress
              // events for the ticker, same as the regular update.
              await runRollback(idx);
              showCard("card-manager");
            },
          };
        }
        return row;
      }),
    });
  }

  renderSections(body, sections);
}

/**
 * Render the sub-line for a history entry: applied-at timestamp,
 * previous tag (when present), and a "rollback available" hint when
 * the entry still has a usable snapshot.
 */
function formatHistoryDetail(entry) {
  const parts = [];
  if (entry.applied_at) parts.push(`applied ${entry.applied_at}`);
  if (entry.previous_tag) parts.push(`from ${entry.previous_tag}`);
  if (entry.snapshot_id) parts.push("rollback available");
  return parts.join(" · ");
}

async function loadDiagnostics() {
  const body = els.body();
  if (!body) return;
  renderLoading(body, "Gathering diagnostics…");

  // Two reads in parallel — diagnostics is slow-ish (disk walk) and
  // history is trivial; doing them concurrently overlaps the network
  // and IO and keeps the perceived render time tight.
  const [diagRes, historyRes] = await Promise.all([
    safeInvoke("get_diagnostics"),
    safeInvoke("get_update_history"),
  ]);
  if (diagRes.err) {
    renderError(body, `Couldn't gather diagnostics: ${diagRes.err}`);
    return;
  }
  if (historyRes.err) {
    console.warn("[launcher] get_update_history failed:", historyRes.err);
  }
  renderDiagnostics(diagRes.ok, historyRes.ok ?? []);
}

let wired = false;

/**
 * Open the diagnostics card. The refresh-button handler is bound once
 * (module-level state) and outlives any single openDiagnostics call.
 * The back-button handler is per-call so the caller's `onBack` closure
 * is what fires; the handler self-removes on click.
 *
 * @param {() => void} onBack — called when user clicks Back.
 */
export async function openDiagnostics(onBack) {
  showCard("card-diagnostics");

  if (!wired) {
    wired = true;
    els.refresh()?.addEventListener("click", () => {
      loadDiagnostics();
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

  await loadDiagnostics();
}
