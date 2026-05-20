// Thin wrapper around webdriverio's `remote` — pre-fills the host/port
// the tauri-plugin-webdriver server binds to so individual tests don't
// need to know the connection details.

import { remote } from "webdriverio";
import { WEBDRIVER_HOST, WEBDRIVER_PORT } from "./launcher.mjs";

/**
 * Open a new WebDriver session against the running launcher. The
 * plugin accepts (but ignores) capabilities — pass anything.
 */
export async function connect() {
  return remote({
    hostname: WEBDRIVER_HOST,
    port: WEBDRIVER_PORT,
    logLevel: "warn",
    capabilities: {},
  });
}
