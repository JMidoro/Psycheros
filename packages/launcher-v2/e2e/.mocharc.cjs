// Mocha config — wired so every spec gets a long timeout (Tauri startup
// + WebDriver port-binding can take a few seconds on a cold cache) and
// runs sequentially (the launcher binary is single-instance — one
// session per test would race for port 4445 if parallelism were on).
module.exports = {
  extension: ["mjs"],
  spec: ["specs/**/*.e2e.mjs"],
  timeout: 30000,
  slow: 5000,
  reporter: "spec",
  parallel: false,
};
