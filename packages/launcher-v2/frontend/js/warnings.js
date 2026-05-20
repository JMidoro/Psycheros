/**
 * Self-repair warnings panel.
 *
 * Surfaces conditions the launcher's basic state probes don't directly
 * expose — MCP connectivity is down, the daemon keeps crashing, or a
 * port the daemon needs is held by another process. Each warning
 * renders as its own card-like entry with a short title, body, and
 * remediation buttons.
 *
 * The panel is hidden when there are no active warnings. Warnings
 * dedupe by key (e.g. "mcp-down" only renders once even if 50 error
 * lines stream through) and clear themselves when the underlying
 * condition reverses.
 */

import { listen, safeInvoke } from "./tauri-bridge.js";

const els = {
  panel: () => document.getElementById("warnings-panel"),
};

// Active warnings, keyed by stable id. Re-rendered on every change.
const warnings = new Map();

// Daemon-status transition history, used for crashloop detection.
// Keeps the last STATUS_HISTORY_WINDOW_MS worth of events.
const STATUS_HISTORY_WINDOW_MS = 60_000;
// Number of Running → Installed transitions within the window that
// counts as a crashloop. Three cycles in a minute is a strong signal
// even with launchd's KeepAlive throttling.
const CRASHLOOP_TRANSITION_THRESHOLD = 3;
// How long a daemon can sit in "installed" (loaded but not bound)
// before we surface the port-conflict diagnostic. Normal boot takes
// 2-8 seconds; anything past 20s is genuinely stuck.
const STUCK_INSTALLED_MS = 20_000;

const statusHistory = [];
let lastState = null;
let stuckInstalledTimer = null;

// MCP-error log patterns. These are the exact (or near-exact) strings
// psycheros's mcp-client emits when entity-core stops talking back —
// see packages/psycheros/src/mcp-client/mod.ts. Adding new patterns
// here is the right place when psycheros's logging changes.
const MCP_ERROR_PATTERNS = [
  /MCP error -32000/,
  /\[MCP\] Pull failed/,
  /\[MCP\] Connection closed/,
  /Falling back to local files mode/,
];
const MCP_OK_PATTERNS = [
  /\[MCP\] Pulled identity from entity-core/,
  /\[MCP\] entity-core has no identity files; keeping local templates/,
  /\[MCP\] Connected to entity-core/,
];

// --------------------------------------------------------------------------
// Render
// --------------------------------------------------------------------------

function render() {
  const panel = els.panel();
  if (!panel) return;

  if (warnings.size === 0) {
    panel.hidden = true;
    panel.replaceChildren();
    return;
  }

  panel.hidden = false;
  panel.replaceChildren();

  for (const warning of warnings.values()) {
    const entry = document.createElement("div");
    entry.className = warning.danger
      ? "warning-entry warning-entry--danger"
      : "warning-entry";

    const title = document.createElement("div");
    title.className = "warning-entry__title";
    title.textContent = warning.title;
    entry.appendChild(title);

    const body = document.createElement("div");
    body.className = "warning-entry__body";
    body.textContent = warning.body;
    entry.appendChild(body);

    if (warning.actions?.length) {
      const actionsRow = document.createElement("div");
      actionsRow.className = "warning-entry__actions";
      for (const action of warning.actions) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.textContent = action.label;
        if (action.danger) btn.classList.add("danger");
        btn.addEventListener("click", async () => {
          btn.disabled = true;
          try {
            await action.onClick();
          } finally {
            if (btn.isConnected) btn.disabled = false;
          }
        });
        actionsRow.appendChild(btn);
      }
      entry.appendChild(actionsRow);
    }

    panel.appendChild(entry);
  }
}

function setWarning(key, warning) {
  warnings.set(key, warning);
  render();
}

function clearWarning(key) {
  if (warnings.delete(key)) render();
}

// --------------------------------------------------------------------------
// MCP-down detection (§5.11)
// --------------------------------------------------------------------------

function handleLogLine(line) {
  if (typeof line !== "string") return;
  if (MCP_ERROR_PATTERNS.some((re) => re.test(line))) {
    setWarning("mcp-down", {
      title: "Memory sync is offline.",
      body: "I can't reach entity-core right now, so I'm running on my " +
        "local identity and memory cache. New memories may not persist " +
        "across instances until the connection recovers. A daemon " +
        "restart often clears this.",
      actions: [
        {
          label: "Restart daemon",
          async onClick() {
            // Stop + restart is the simplest user-visible recovery.
            // stop_daemon is session-scoped (autostart returns at
            // next login), so we follow with start_daemon explicitly.
            const { err: stopErr } = await safeInvoke("stop_daemon");
            if (stopErr) console.warn("[launcher] stop failed:", stopErr);
            const { err: startErr } = await safeInvoke("start_daemon");
            if (startErr) console.warn("[launcher] start failed:", startErr);
            // The status watcher will re-poll log lines and clear the
            // warning when MCP comes back. Leave the warning in place
            // until then so the user sees the action took effect.
          },
        },
      ],
    });
    return;
  }
  if (MCP_OK_PATTERNS.some((re) => re.test(line))) {
    clearWarning("mcp-down");
  }
}

// --------------------------------------------------------------------------
// Crashloop + port-conflict detection (§5.12 + §5.13)
// --------------------------------------------------------------------------

function recordStatusTransition(state) {
  const now = Date.now();
  if (lastState && lastState !== state) {
    statusHistory.push({ from: lastState, to: state, at: now });
    // Trim history to the rolling window.
    while (
      statusHistory.length > 0 &&
      now - statusHistory[0].at > STATUS_HISTORY_WINDOW_MS
    ) {
      statusHistory.shift();
    }
  }
  lastState = state;
}

function countRecentCrashTransitions() {
  return statusHistory.filter(
    (t) => t.from === "running" && t.to === "installed",
  ).length;
}

async function handleStatus(status) {
  if (!status) return;
  recordStatusTransition(status.state);

  // Crashloop: too many Running → Installed cycles in the window.
  // Once detected, the warning sticks around until the daemon has
  // been Running for the full window without a transition.
  const crashCount = countRecentCrashTransitions();
  if (crashCount >= CRASHLOOP_TRANSITION_THRESHOLD) {
    setWarning("crashloop", {
      title: "I keep crashing.",
      body: `Detected ${crashCount} restart cycle(s) in the last minute. ` +
        "Check the log panel below for the most recent crash. " +
        "Re-init clears my source clone and identity dir and walks " +
        "you back through first-run — usually fixes corruption-style " +
        "failures.",
      danger: true,
      actions: [
        {
          label: "Open Data → Re-init",
          async onClick() {
            // Cheap nudge — programmatically click the manager's
            // Data button if present. Falls back to a console hint
            // if the footer hasn't rendered yet.
            const dataBtn = Array.from(document.querySelectorAll(".meta__tool"))
              .find((b) => b.textContent === "Data");
            if (dataBtn) dataBtn.click();
            else console.warn("[launcher] data button not in DOM yet");
          },
        },
      ],
    });
  } else if (status.state === "running") {
    // We just hit Running. If the last entry in history was a
    // Running → Installed, the crashloop counter still has it;
    // only fully-clean (no recent crash transitions) clears.
    if (crashCount === 0) clearWarning("crashloop");
  }

  // Port conflict: daemon stuck in Installed past STUCK_INSTALLED_MS.
  // Start a timer when entering Installed; clear it on any other
  // state. When the timer fires we run check_port_conflict.
  if (stuckInstalledTimer != null) {
    clearTimeout(stuckInstalledTimer);
    stuckInstalledTimer = null;
  }
  if (status.state === "installed") {
    stuckInstalledTimer = setTimeout(async () => {
      const { ok: conflict } = await safeInvoke("check_port_conflict", {
        port: status.port,
      });
      if (conflict) {
        setWarning("port-conflict", {
          title:
            `Port ${status.port} is held by ${conflict.command} (pid ${conflict.pid}).`,
          body: "I can't bind my HTTP port because another process is " +
            "already listening on it. Quit that process (or restart " +
            "your computer if you're not sure what it is) and try " +
            "again. The daemon will pick the port up automatically " +
            "once it's free.",
          danger: true,
        });
      }
    }, STUCK_INSTALLED_MS);
  } else {
    clearWarning("port-conflict");
  }
}

// --------------------------------------------------------------------------
// Public entry — wire to the existing log + status streams
// --------------------------------------------------------------------------

let wired = false;

export function wireWarnings(initialStatus) {
  if (wired) return;
  wired = true;
  // Existing manager.js wiring already drives these listeners; we
  // add second listeners since `listen` is fan-out (each callback
  // independently subscribes). Avoids touching manager.js's render
  // loop.
  listen("daemon-log-line", (evt) => handleLogLine(evt.payload));
  listen("daemon-status-changed", (evt) => handleStatus(evt.payload));
  if (initialStatus) handleStatus(initialStatus);
}
