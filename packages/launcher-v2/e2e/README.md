# launcher-v2 — end-to-end tests

These specs drive the real Tauri webview via
[`tauri-plugin-webdriver`](https://crates.io/crates/tauri-plugin-webdriver),
exercising the UI ↔ IPC boundary that nothing else in the test pyramid
covers. They run on macOS today (the only platform with a full
supervisor impl); the plugin itself is cross-platform if/when
Linux/Windows supervisors ship.

## Prerequisites

- Node 20+ and npm
- A debug build of the launcher with the `webdriver` cargo feature:

  ```
  cd ../src-tauri
  cargo build --features webdriver
  ```

  Release builds intentionally exclude the WebDriver server — the
  feature flag is opt-in.

## Running

```
npm install
npm test
```

Each spec spawns its own launcher process in a hermetic temp dir
(`PSYCHEROS_LAUNCHER_DATA_DIR`), waits for the WebDriver server on
`127.0.0.1:4445`, runs assertions, and tears down.

## Writing tests

Pattern (see `specs/smoke.e2e.mjs`):

```js
import { describe, it, before, after } from "mocha";
import { strict as assert } from "node:assert";
import { spawnLauncher, waitForWebDriver, killLauncher } from "../lib/launcher.mjs";
import { connect } from "../lib/connect.mjs";

describe("my feature", () => {
  let launcher, browser;
  before(async () => {
    launcher = spawnLauncher();
    await waitForWebDriver();
    browser = await connect();
  });
  after(async () => {
    if (browser) await browser.deleteSession().catch(() => {});
    await killLauncher(launcher);
  });

  it("does the thing", async () => {
    const el = await browser.$("#card-manager");
    await el.waitForExist({ timeout: 5000 });
    assert.ok(await el.isDisplayed());
  });
});
```

The launcher's frontend uses stable IDs on its top-level cards
(`#card-wizard`, `#card-manager`, `#card-diagnostics`, etc.) — favor
these over class selectors, which churn with style edits.

## Troubleshooting

- **`WebDriver server on 127.0.0.1:4445 not ready`** — usually means
  the binary wasn't built with `--features webdriver`. Re-run the
  cargo build step.
- **Tests hang** — check for orphan launcher processes:
  `ps aux | grep psycheros-launcher` and kill them. The `after` hook
  SIGTERMs the child but a crashed spec can skip it.
- **`Error: connect ECONNREFUSED 127.0.0.1:4445`** — the launcher
  process exited before `waitForWebDriver` saw the port. Surface its
  stderr by re-running with the launcher's output visible (the helper
  already streams it to the test stdout).
