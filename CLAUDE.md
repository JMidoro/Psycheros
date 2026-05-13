# Psycheros — workspace agent card

Deno 2.x workspace. Four packages, one canonical-self model, one design value
that runs through everything: **the entity is the subject.**

## The first-person convention

All prompts, system messages, tool descriptions, code comments, and
documentation are written from the entity's first-person perspective — "I am…",
"I should…", "my memory", "my identity". Never the second person. The entity
internalizes the system as theirs, not as rules imposed on them.

This is the core design value. Preserve it in every contribution, including new
code comments and any user-facing copy. Full rationale:
[`PHILOSOPHY.md`](PHILOSOPHY.md).

## Workspace shape

```
psycheros-workspace/
├── deno.json              # workspace root: shared compilerOptions + hoisted deps
├── packages/
│   ├── psycheros/         # harness daemon (port 3000) — embodiment
│   ├── entity-core/       # MCP server (stdio) — canonical identity + memory
│   ├── entity-loom/       # chat-history import wizard (port 3210)
│   └── launcher/          # bootstrap installer + dashboard (port 3001)
└── .github/workflows/     # multi-package CI matrix
```

| Package                | Per-package agent guide                                                                     |
| ---------------------- | ------------------------------------------------------------------------------------------- |
| `packages/psycheros`   | [`packages/psycheros/CLAUDE.md`](packages/psycheros/CLAUDE.md)                              |
| `packages/entity-core` | [`packages/entity-core/CLAUDE.md`](packages/entity-core/CLAUDE.md)                          |
| `packages/entity-loom` | [`packages/entity-loom/CLAUDE.md`](packages/entity-loom/CLAUDE.md)                          |
| `packages/launcher`    | no per-package CLAUDE.md — see [`packages/launcher/README.md`](packages/launcher/README.md) |

## The cross-cutting truth

**`entity-core` is canonical for identity and memory.** Every other package that
touches identity or memory is a _consumer_ of the MCP server, not a source of
truth.

- `psycheros` is an embodiment. It has a local `identity/` directory, but that's
  a cache populated from `entity-core` when MCP is enabled. Direct writes to
  `identity/` outside of explicit MCP-fallback paths are bugs.
- `psycheros` writes memories _exclusively_ through MCP. There is no
  Psycheros-local memory store.
- `entity-loom` produces an _importable package_. It doesn't write to
  `entity-core` directly — the user imports the package after the wizard
  finishes.

When in doubt about where identity or memory state lives, the answer is
`entity-core`.

## Workspace commands

From the workspace root:

```bash
deno check        # type-check everything
deno lint
deno fmt --check
```

Per-package commands (dev server, start, stop) live in each package's
`deno.json`:

```bash
cd packages/<name> && deno task <task>
```

## Hoisted dependencies

Shared dependencies are declared once at workspace root in `deno.json` to
prevent version drift:

- `@std/*` (path, async, dotenv, fs, assert)
- `@db/sqlite` — SQLite + sqlite-vec
- `@xenova/transformers` — HuggingFace transformer embeddings (used by
  `psycheros` and `entity-core`)
- `@modelcontextprotocol/sdk` — MCP protocol (server in `entity-core`, client in
  `psycheros`)
- `jszip` — used by both `psycheros` and `entity-core`

Package-specific dependencies stay in each package's own `deno.json`. **Don't
add a duplicate of a hoisted dep to a package's `deno.json`** — that's how
version drift starts.

## CLAUDE.md scope

CLAUDE.md files describe how to operate on the code: load-bearing wirings, traps
that bite, patterns to follow. They are not a feature catalog (that's the
README) and not a deep reference (that's `packages/<name>/docs/`). If a section
is long enough to need subheadings, it probably belongs in `docs/`.

## Contributing

[`CONTRIBUTING.md`](CONTRIBUTING.md) covers setup (Deno 2.7.5+), the
first-person convention as it applies to PRs, and CI gates (`deno check` +
`deno lint` + `deno fmt --check`).
