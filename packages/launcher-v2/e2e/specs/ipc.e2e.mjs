// IPC E2E — verifies the JS → Rust command bridge actually round-trips
// through the live webview. This is the highest-value class of test
// the WebDriver harness enables, because nothing else in the test
// pyramid (Rust unit tests, integration tests, smoke binary) exercises
// the real IPC serialization layer with the real Tauri runtime.
//
// We hit read-only commands only — get_daemon_mode, get_update_channel,
// daemon_status — so the test never mutates state outside the hermetic
// PSYCHEROS_LAUNCHER_DATA_DIR. The launcher binary runs in a tempdir
// that's cleaned up on teardown, so each invoke is also hermetic from
// state perspective.

import { after, before, describe, it } from "mocha";
import { strict as assert } from "node:assert";
import {
  killLauncher,
  spawnLauncher,
  waitForWebDriver,
} from "../lib/launcher.mjs";
import { connect } from "../lib/connect.mjs";

describe("ipc: read-only commands round-trip through the live webview", function () {
  this.timeout(60000);

  let launcher;
  let browser;

  before(async () => {
    launcher = spawnLauncher();
    await waitForWebDriver();
    browser = await connect();
    // Wait for window.__TAURI__ to be present before invoking through it.
    await browser.waitUntil(
      () => browser.execute(() => typeof window.__TAURI__ === "object"),
      { timeout: 10000, timeoutMsg: "window.__TAURI__ never appeared" },
    );
  });

  after(async () => {
    if (browser) await browser.deleteSession().catch(() => {});
    await killLauncher(launcher);
  });

  // browser.executeAsync hands the test a `done` callback the page-side
  // code calls once the invoke promise resolves. Wrapper keeps the call
  // sites readable.
  async function invoke(name, args = null) {
    return browser.executeAsync((cmd, payload, done) => {
      window.__TAURI__.core.invoke(cmd, payload ?? undefined)
        .then((result) => done({ ok: result, err: null }))
        .catch((err) => done({ ok: null, err: String(err) }));
    }, name, args);
  }

  it("get_daemon_mode returns either 'autostart' or 'manual'", async () => {
    const res = await invoke("get_daemon_mode");
    assert.equal(res.err, null, `invoke errored: ${res.err}`);
    assert.ok(
      res.ok === "autostart" || res.ok === "manual",
      `expected 'autostart' or 'manual', got ${JSON.stringify(res.ok)}`,
    );
  });

  it("get_update_channel returns either 'stable' or 'beta'", async () => {
    const res = await invoke("get_update_channel");
    assert.equal(res.err, null, `invoke errored: ${res.err}`);
    assert.ok(
      res.ok === "stable" || res.ok === "beta",
      `expected 'stable' or 'beta', got ${JSON.stringify(res.ok)}`,
    );
  });

  it("daemon_status returns not-installed on a fresh data dir", async () => {
    // Fresh PSYCHEROS_LAUNCHER_DATA_DIR with HOME redirect + a config
    // pointing the probe at an unused high port — the only legal state
    // is `not-installed`. State enum serializes kebab-case over IPC
    // (serde rename_all = "kebab-case"). The `supervisor_loaded` flag
    // is deliberately NOT asserted: it reads from the OS's launchctl
    // session which is global to the user and can't be sandboxed —
    // a dev with the real daemon installed would see `true` there
    // even with HOME redirected.
    const res = await invoke("daemon_status");
    assert.equal(res.err, null, `invoke errored: ${res.err}`);
    assert.equal(
      res.ok?.state,
      "not-installed",
      `expected state=not-installed on a fresh dir, got ${JSON.stringify(res.ok)}`,
    );
    assert.equal(res.ok?.supervisor_installed, false);
  });

  it("get_update_history returns an empty array on a fresh data dir", async () => {
    const res = await invoke("get_update_history");
    assert.equal(res.err, null, `invoke errored: ${res.err}`);
    assert.ok(Array.isArray(res.ok), `expected an array, got ${typeof res.ok}`);
    assert.equal(res.ok.length, 0, `expected empty array on fresh dir`);
  });

  it("invoking an unknown command rejects (not silent no-op)", async () => {
    // If invoke ever silently no-ops for typos, the entire frontend's
    // error handling becomes a lie. Verify the negative.
    const res = await invoke("definitely_not_a_real_command_xyz");
    assert.notEqual(res.err, null, "expected unknown command to reject");
  });
});
