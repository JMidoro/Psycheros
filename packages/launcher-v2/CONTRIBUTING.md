# Contributing to launcher-v2

Dev setup, build commands, agent context. For end-user install + usage, see
[`README.md`](README.md). For operational recovery (daemon won't start, MCP
down, etc.), see [`docs/runbook.md`](docs/runbook.md).

## Prerequisites

- **Rust** 1.77+
- **Deno** 2.x (matches `.deno-version` at the workspace root)
- **Tauri 2 CLI** — `npx --yes @tauri-apps/cli@^2.0 …` works without global
  install
- **macOS only** for now — Linux + Windows supervisors are stubs (see
  [`docs/supervisors.md`](docs/supervisors.md))

The launcher-v2 package is **not** in the Deno workspace — it's Rust + plain
HTML/JS/CSS. The root `deno.json` workspace list deliberately omits it.

## Dev loop

```bash
cd packages/launcher-v2

# One-time: stage the sidecar Deno binary + generate icons.
./scripts/setup.sh

# Hot-reload dev. Tauri opens a window pointed at the local
# webview; Rust changes trigger a rebuild, HTML/JS/CSS changes
# hot-reload in the webview.
npx --yes @tauri-apps/cli@^2.0 dev
```

In dev mode the launcher uses the system Deno from your PATH (not the staged
one), and reads source from your local checkout (via `PSYCHEROS_SRC_DIR`) rather
than cloning from GitHub.

## Building a distributable

```bash
cd packages/launcher-v2
npx --yes @tauri-apps/cli@^2.0 build --target aarch64-apple-darwin
```

Produces `.dmg` + `.app.tar.gz` under
`src-tauri/target/aarch64-apple-darwin/release/bundle/`. The CI release job
(`.github/workflows/release.yml`) runs this against the staging repo's tags and
uploads to GitHub Releases. Manual builds are useful for smoke-testing before
tagging.

## Gates

Run these before opening a PR. CI checks the same set.

```bash
cd packages/launcher-v2/src-tauri
cargo check
cargo fmt --check
cargo clippy -- -D warnings
cargo test --lib

# JS sanity check (no build step — plain ES modules):
cd ..
for f in frontend/js/*.js; do node --check "$f"; done
```

## End-to-end testing

UI ↔ IPC behavior — the only class of regression the cargo gates and the
`--smoke` binary can't catch — is covered by a wdio + mocha suite under
[`e2e/`](e2e/) that drives the real Tauri webview via `tauri-plugin-webdriver`.
macOS-only today (matches the supervisor support story).

```bash
# Build with the opt-in webdriver feature (off by default — release
# builds never ship the WebDriver server).
cd src-tauri && cargo build --features webdriver

# Then run the suite.
cd ../e2e && npm install && npm test
```

Each spec spawns its own launcher process in a hermetic temp dir
(`PSYCHEROS_LAUNCHER_DATA_DIR`), waits for the plugin's WebDriver server on
`127.0.0.1:4445`, runs assertions against the live webview, and tears down. See
[`e2e/README.md`](e2e/README.md) for the spec template + troubleshooting notes.

CI runs the E2E suite as part of the `launcher-v2` job in
`.github/workflows/check.yml` — `cargo build --features webdriver` → `npm ci` →
`npm test`.

## Agent context

The load-bearing wirings, traps that bite, and architectural commitments live in
[`CLAUDE.md`](CLAUDE.md) — read it first when modifying anything. Deep
references (architecture, supervisors, source provisioning, frontend
conventions, release pipeline) are in [`docs/`](docs/).

The v1.0 roadmap that drove the most recent body of work is preserved at
[`docs/v1-roadmap.md`](docs/v1-roadmap.md) for historical context — what
shipped, the architectural decisions locked in, the ownership matrix between
launcher and psycheros.

## First-person convention

Every user-facing string, prompt, and code comment is written in the entity's
first person: "I am…", "I should…", "my memory". This is project-wide, not
launcher-specific — [`PHILOSOPHY.md`](../../PHILOSOPHY.md) at the repo root
carries the rationale. Preserve it in new code.

Internal-only Rust code (struct fields, function names, error types) doesn't
need first person — only what reaches the user. The launcher's UI surface is
utility surface, not entity surface, so the convention applies mainly to copy in
`frontend/`.
