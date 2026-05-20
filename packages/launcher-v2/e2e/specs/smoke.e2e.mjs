// Smoke E2E — proves the harness works end-to-end:
//   1. The launcher binary (built with --features webdriver) launches.
//   2. The plugin's WebDriver server binds 127.0.0.1:4445.
//   3. wdio connects and attaches to the live webview.
//   4. The frontend HTML loaded.
//   5. The bootstrap path resolved (either wizard or manager card visible).
//   6. The Tauri IPC bridge is exposed via window.__TAURI__ — the load-
//      bearing `withGlobalTauri: true` in tauri.conf.json is correct.
//
// On a fresh PSYCHEROS_LAUNCHER_DATA_DIR (which is what the helper sets
// up) the launcher should land in the wizard card; if anything's wrong
// with that gating, we'd see the manager. Either is acceptable for
// smoke — the assertion is "one of them is visible," not "exactly the
// wizard." The Cmd+, view-toggle test asserts the more specific
// invariants.

import { after, before, describe, it } from "mocha";
import { strict as assert } from "node:assert";
import {
  killLauncher,
  spawnLauncher,
  waitForWebDriver,
} from "../lib/launcher.mjs";
import { connect } from "../lib/connect.mjs";

describe("smoke: launcher boots and renders a card", function () {
  // Generous: cold cargo cache + first launch on a CI runner can be slow.
  this.timeout(60000);

  let launcher;
  let browser;

  before(async () => {
    launcher = spawnLauncher();
    await waitForWebDriver();
    browser = await connect();
  });

  after(async () => {
    if (browser) {
      await browser.deleteSession().catch(() => {});
    }
    await killLauncher(launcher);
  });

  it("loads index.html with the expected title", async () => {
    // waitUntil rather than a one-shot getTitle — on cold CI runners
    // the WebDriver session attaches before the HTML head finishes
    // parsing, so `document.title` is briefly empty. Locally the
    // warm caches make this race invisible; CI exposes it.
    await browser.waitUntil(
      async () => (await browser.getTitle()) === "Psycheros",
      {
        timeout: 5000,
        interval: 100,
        timeoutMsg: "document.title never became 'Psycheros' within 5s",
      },
    );
  });

  it("reveals either the wizard or the manager card within 5s", async () => {
    await browser.waitUntil(
      async () => {
        const wizard = await browser.$("#card-wizard");
        const manager = await browser.$("#card-manager");
        const wizardVisible = await wizard.isDisplayed().catch(() => false);
        const managerVisible = await manager.isDisplayed().catch(() => false);
        return wizardVisible || managerVisible;
      },
      {
        timeout: 5000,
        interval: 100,
        timeoutMsg:
          "Neither #card-wizard nor #card-manager became visible after 5s",
      },
    );
  });

  it("exposes the Tauri IPC bridge on window.__TAURI__", async () => {
    // withGlobalTauri: true in tauri.conf.json is load-bearing — every
    // frontend IPC call goes through this object. If the config drifts,
    // this assertion catches it before the user does.
    const tauriExists = await browser.execute(
      () =>
        typeof window.__TAURI__ === "object" && window.__TAURI__ !== null,
    );
    assert.equal(tauriExists, true);
  });
});
