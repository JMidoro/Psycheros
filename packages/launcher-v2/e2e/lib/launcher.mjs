// Binary lifecycle helpers — spawn the launcher (built with the
// `webdriver` cargo feature) in a hermetic temp data dir, wait for its
// WebDriver server to bind to 127.0.0.1:4445, then kill on teardown.

import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { connect as tcpConnect } from "node:net";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const REPO_ROOT = resolve(HERE, "../../");
export const WEBDRIVER_HOST = "127.0.0.1";
export const WEBDRIVER_PORT = 4445;

// Where cargo builds the binary. Both debug and release builds land
// under target/<profile>/, and the binary name is set by the [[bin]]
// stanza in src-tauri/Cargo.toml.
function defaultBinaryPath() {
  // Override via PSYCHEROS_LAUNCHER_BIN if the caller built somewhere
  // unusual — useful for CI builds that may use a release profile or
  // a target-triple-specific subdir.
  if (process.env.PSYCHEROS_LAUNCHER_BIN) {
    return process.env.PSYCHEROS_LAUNCHER_BIN;
  }
  return resolve(REPO_ROOT, "src-tauri/target/debug/psycheros-launcher");
}

/**
 * Spawn the launcher binary in a hermetic temp data dir.
 *
 * Returns `{ child, dataDir }`. The caller should `await waitForWebDriver()`
 * before connecting wdio, and call `killLauncher` in their `after` hook.
 */
export function spawnLauncher({ env = {} } = {}) {
  const dataDir = mkdtempSync(join(tmpdir(), "psycheros-e2e-"));
  // Pre-seed config.json with an unused high port so the daemon probe
  // doesn't hit whatever real service might be holding 3000 on the host
  // (a dev's actual Psycheros install, another project's Node server,
  // etc.). The launcher reads cfg.port from this file via
  // `daemon::status::probe` → `config::load`. 38567 is arbitrary and
  // chosen well away from the dynamic-port range.
  const probePort = 38567;
  writeFileSync(
    join(dataDir, "config.json"),
    JSON.stringify({ port: probePort }, null, 2),
  );
  const binary = defaultBinaryPath();
  const child = spawn(binary, [], {
    env: {
      ...process.env,
      // Hermetic data dir — same env override pattern the smoke binary
      // and integration tests use. See src-tauri/src/paths.rs.
      PSYCHEROS_LAUNCHER_DATA_DIR: dataDir,
      // HOME redirect so the launchd supervisor's plist-path probe
      // (`dirs::home_dir()` → `~/Library/LaunchAgents/<label>.plist`)
      // lands in the tempdir rather than reading whatever the developer
      // has actually installed on their machine. `dirs::home_dir()`
      // honors HOME on macOS, unlike `dirs::data_dir()` which uses
      // NSSearchPath. Without this, `daemon_status` returns the host
      // machine's real state instead of the fresh-install state we
      // want the suite to assert against.
      HOME: dataDir,
      // Don't inherit a stray user-set custom WebDriver port.
      TAURI_WEBDRIVER_PORT: String(WEBDRIVER_PORT),
      ...env,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  // Surface launcher stderr to the test logs — invaluable when a test
  // fails because the binary panicked.
  child.stderr.on("data", (chunk) => {
    process.stderr.write(`[launcher] ${chunk}`);
  });
  child.stdout.on("data", (chunk) => {
    process.stdout.write(`[launcher] ${chunk}`);
  });

  return { child, dataDir };
}

/**
 * Poll the WebDriver port until it accepts a connection (or timeout).
 * Tauri startup + plugin init can take ~1-3s on macOS; we give it a
 * generous ceiling so CI cold runs don't false-negative.
 */
export async function waitForWebDriver({ timeoutMs = 20000 } = {}) {
  const deadline = Date.now() + timeoutMs;
  let lastErr;
  while (Date.now() < deadline) {
    try {
      await tcpPing(WEBDRIVER_HOST, WEBDRIVER_PORT);
      return;
    } catch (err) {
      lastErr = err;
      await sleep(250);
    }
  }
  throw new Error(
    `WebDriver server on ${WEBDRIVER_HOST}:${WEBDRIVER_PORT} not ready after ${timeoutMs}ms (last error: ${lastErr?.message ?? lastErr})`,
  );
}

function tcpPing(host, port) {
  return new Promise((res, rej) => {
    const sock = tcpConnect(port, host);
    sock.once("connect", () => {
      sock.end();
      res();
    });
    sock.once("error", rej);
  });
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

/**
 * Kill the launcher process and remove its temp data dir.
 *
 * Awaits the child's exit before returning, so the next test's
 * spawnLauncher won't race the dying process for port 4445. Without
 * this, mocha's serial describe order produces flaky ECONNRESET on
 * the new wdio session because TCP teardown lags the kill signal.
 */
export async function killLauncher({ child, dataDir }) {
  if (child && child.exitCode === null && !child.killed) {
    const exited = new Promise((resolve) => {
      child.once("exit", resolve);
    });
    child.kill("SIGTERM");
    // SIGTERM should be enough; SIGKILL as a 2s fallback covers a
    // hung event loop.
    const timer = setTimeout(() => {
      if (child.exitCode === null) child.kill("SIGKILL");
    }, 2000);
    await exited;
    clearTimeout(timer);
  }
  if (dataDir) {
    try {
      rmSync(dataDir, { recursive: true, force: true });
    } catch {
      // Best effort — the OS will clean tmpdir eventually.
    }
  }
}
