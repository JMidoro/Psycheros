# Changelog

All notable changes to the Psycheros harness daemon are documented here. The
format follows [Keep a Changelog](https://keepachangelog.com/), and this package
follows [Semantic Versioning](https://semver.org/).

## [0.3.0] - 2026-05-14

### Changed

- **Durable scheduler replaces `Deno.cron` everywhere.** Every scheduled or
  event-triggered task — daily memory summarization, identity snapshots, MCP
  identity-change pushes, every flavour of Pulse trigger — now routes through a
  shared `@psycheros/scheduler` workspace package backed by two SQLite tables
  (`schedules` + `job_runs`). Cron fires missed while the daemon was down are
  caught up on next boot per each schedule's catch-up policy; in-flight runs at
  crash are reclaimed instead of orphaned; identity-write pushes survive process
  death via a durable queue; long-running handlers (LLM streams, multi-step
  summarization) keep their leases auto-renewed.
- **Pulse run statistics are derived from `job_runs`, not stored on `pulses`.**
  The `pulses` table no longer carries `success_count`, `error_count`,
  `last_run_at`, or `last_status` — these are computed on demand via
  `DBClient.getPulseStats()`. Existing data is preserved through a one-time
  migration on first boot.
- **`Deno.cron` flag retired.** The `--unstable-cron` flag is no longer required
  in `deno.json` tasks, the Dockerfile, the `.env.example`, or the
  `PSYCHEROS_MCP_ARGS` default. Existing overrides that still pass it are
  harmless but unused.

### Removed

- `src/server/cron-tracker.ts` and the legacy `cron_job_runs` / `pulse_runs`
  tables. The first-boot schema migration folds every legacy row into `job_runs`
  and drops both tables.

### Migration

This release performs a one-shot SQLite migration on first boot:

- `cron_job_runs` rows fold into `job_runs` as their respective handlers
  (`memory.summarize-daily`, `identity.snapshot`), then the table is dropped.
- `pulse_runs` rows fold into `job_runs` as `pulse.execute`, with pulse context
  preserved in `payload`. Any row left in `running` state from a previous
  process is marked `dead` with a reclaim explanation. The legacy table is
  dropped.
- The `pulses` table is rebuilt in place to remove the four denormalized
  run-stat columns; every other column and every row is preserved.

Migration is idempotent — safe to run on a DB that's already been migrated.

## [0.2.0] - 2026-05-13

### Added

- Version chip in the chat header (lower-right). Clicks through to the GitHub
  release page for the running version; staging builds render the chip
  non-interactive with a `· staging` flavor and the full sha in the tooltip.
- `/health` now returns identity + version JSON (`name`, `version`,
  `version_base`, `version_suffix`, `is_staging`, `entity_core_version`,
  `started_at`). Container `HEALTHCHECK` still only reads `r.ok`.
- Admin "Versions" section in the diagnostics dashboard, showing psycheros,
  entity-core, and sqlite-vec versions side by side. Copy-as-markdown export
  includes the same block.
- Service worker cache key now stamps the running version
  (`psycheros-offline-<safe-version>`), evicting stale offline assets on every
  upgrade instead of forever pinning the v2 cache.
- Container image carries `org.opencontainers.image.version` LABEL matching the
  running version (visible in `docker inspect` and the GHCR sidebar).

### Fixed

- Startup banner version no longer shows hardcoded `0.1.0` regardless of the
  actual release. `src/version.ts` is now the source of truth for the running
  version, sourced from `deno.json` via a JSON import.

## [0.1.2] - 2026-05-13

### Fixed

- `getMessagesPaginated`: scroll-back no longer jumps to the oldest message when
  loading earlier history.

## [0.1.1] - 2026-05-13

### Fixed

- First-run setup for `ZAI_API_KEY`-only deployments. The seeded default LLM
  profile previously pointed at OpenRouter under a "Custom Endpoint" label, so
  the Z.ai key failed auth on first message. The seeded profile now resolves
  correctly to Z.ai (provider `zai`, base URL
  `https://api.z.ai/api/coding/paas/v4/chat/completions`, model `glm-4.7`). No
  data migration; existing volumes (`psycheros-data`, `entity-core-data`) and
  saved LLM profiles carry over unchanged.

### Changed

- `README.md` Essential environment table: `PSYCHEROS_MCP_ENABLED` documented
  default corrected to `true` (matches `.env.example` and runtime).

## [0.1.0] - 2026-05-13

### Added

- Initial public release.
- Persistent AI entity served through a web chat UI on port 3000.
- Streaming LLM, tool execution, RAG.
- Hierarchical memory (daily → weekly → monthly → yearly summaries).
- Knowledge graph (people, places, relationships) backed by SQLite + sqlite-vec.
- Lorebook, data vault, autonomous Pulse triggers.
- Discord gateway, image generation, image captioning.
- Entity identity and memory served by the sibling `entity-core` MCP server,
  spawned as a subprocess when `PSYCHEROS_MCP_ENABLED=true`.

[0.1.2]: https://github.com/PsycherosAI/Psycheros/releases/tag/psycheros-v0.1.2
[0.1.1]: https://github.com/PsycherosAI/Psycheros/releases/tag/psycheros-v0.1.1
[0.1.0]: https://github.com/PsycherosAI/Psycheros/releases/tag/psycheros-v0.1.0
