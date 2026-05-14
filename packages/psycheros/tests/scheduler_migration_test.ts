/**
 * Scheduler migration integration test.
 *
 * Creates a SQLite database with the *legacy* schema (cron_job_runs,
 * pulse_runs, and pulses-with-denormalized-stat-columns), inserts
 * representative rows, then runs the new `initializeSchema` and verifies
 * the migration folded everything into `job_runs` cleanly with no shims
 * left behind.
 *
 * This is the highest-risk piece of the surgery: wrong migration = lost
 * pulse history or corrupted pulses table.
 */

import { assert, assertEquals } from "@std/assert";
import { Database } from "@db/sqlite";
import { initializeSchema } from "../src/db/schema.ts";

function makeLegacyDb(): Database {
  const db = new Database(":memory:");
  // Minimal stand-ins for upstream tables the pulses table references.
  db.exec(`
    CREATE TABLE conversations (
      id TEXT PRIMARY KEY,
      title TEXT,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );

    CREATE TABLE cron_job_runs (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      job_id TEXT NOT NULL,
      started_at TEXT NOT NULL,
      completed_at TEXT NOT NULL,
      duration_ms INTEGER NOT NULL,
      status TEXT NOT NULL CHECK (status IN ('success', 'error')),
      result TEXT,
      error TEXT
    );

    CREATE TABLE pulses (
      id TEXT PRIMARY KEY,
      name TEXT NOT NULL,
      description TEXT,
      prompt_text TEXT NOT NULL,
      chat_mode TEXT NOT NULL DEFAULT 'visible',
      conversation_id TEXT,
      enabled INTEGER NOT NULL DEFAULT 1,
      trigger_type TEXT NOT NULL DEFAULT 'cron',
      cron_expression TEXT,
      interval_seconds INTEGER,
      random_interval_min INTEGER,
      random_interval_max INTEGER,
      run_at TEXT,
      inactivity_threshold_seconds INTEGER,
      chain_pulse_ids TEXT,
      max_chain_depth INTEGER NOT NULL DEFAULT 3,
      source TEXT NOT NULL DEFAULT 'user',
      auto_delete INTEGER NOT NULL DEFAULT 0,
      webhook_token TEXT,
      filesystem_watch_path TEXT,
      success_count INTEGER NOT NULL DEFAULT 0,
      error_count INTEGER NOT NULL DEFAULT 0,
      last_run_at TEXT,
      last_status TEXT,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );

    CREATE TABLE pulse_runs (
      id TEXT PRIMARY KEY,
      pulse_id TEXT NOT NULL,
      conversation_id TEXT,
      trigger_source TEXT NOT NULL,
      started_at TEXT NOT NULL,
      completed_at TEXT,
      duration_ms INTEGER,
      status TEXT NOT NULL CHECK (status IN ('running', 'success', 'error', 'skipped')),
      result_summary TEXT,
      error_message TEXT,
      tool_calls_count INTEGER DEFAULT 0,
      output_content TEXT,
      chain_depth INTEGER NOT NULL DEFAULT 0,
      chain_parent_run_id TEXT,
      created_at TEXT NOT NULL
    );
  `);
  return db;
}

Deno.test("migration: empty legacy tables → empty new tables, schema rebuilt", () => {
  const db = makeLegacyDb();

  initializeSchema(db);

  // Legacy tables are gone.
  const cronRunsExists = db.prepare(
    "SELECT 1 FROM sqlite_master WHERE type='table' AND name='cron_job_runs'",
  ).get();
  const pulseRunsExists = db.prepare(
    "SELECT 1 FROM sqlite_master WHERE type='table' AND name='pulse_runs'",
  ).get();
  assertEquals(cronRunsExists, undefined);
  assertEquals(pulseRunsExists, undefined);

  // New tables exist.
  const schedulesExists = db.prepare(
    "SELECT 1 FROM sqlite_master WHERE type='table' AND name='schedules'",
  ).get();
  const jobRunsExists = db.prepare(
    "SELECT 1 FROM sqlite_master WHERE type='table' AND name='job_runs'",
  ).get();
  assert(schedulesExists);
  assert(jobRunsExists);

  // pulses has been rebuilt without the dying columns.
  const cols = db.prepare("PRAGMA table_info('pulses')")
    .all<{ name: string }>()
    .map((c) => c.name);
  assert(!cols.includes("success_count"));
  assert(!cols.includes("error_count"));
  assert(!cols.includes("last_run_at"));
  assert(!cols.includes("last_status"));
  // But carries over the other columns.
  assert(cols.includes("trigger_type"));
  assert(cols.includes("source"));
  assert(cols.includes("auto_delete"));

  db.close();
});

Deno.test("migration: cron_job_runs rows fold into job_runs with handler mapping", () => {
  const db = makeLegacyDb();

  db.exec(
    `INSERT INTO cron_job_runs
       (job_id, started_at, completed_at, duration_ms, status, result, error)
     VALUES (?, ?, ?, ?, ?, ?, ?)`,
    [
      "memory-daily",
      "2026-05-12T05:00:00.000Z",
      "2026-05-12T05:00:42.000Z",
      42000,
      "success",
      "Summarized 1 day(s)",
      null,
    ],
  );
  db.exec(
    `INSERT INTO cron_job_runs
       (job_id, started_at, completed_at, duration_ms, status, result, error)
     VALUES (?, ?, ?, ?, ?, ?, ?)`,
    [
      "identity-snapshot",
      "2026-05-12T03:00:00.000Z",
      "2026-05-12T03:00:01.500Z",
      1500,
      "error",
      null,
      "MCP not connected",
    ],
  );

  initializeSchema(db);

  const rows = db.prepare(
    `SELECT handler, status, result_summary, error_message
       FROM job_runs ORDER BY handler`,
  ).all<{
    handler: string;
    status: string;
    result_summary: string | null;
    error_message: string | null;
  }>();

  assertEquals(rows.length, 2);
  // Sorted alphabetically: identity.snapshot comes first.
  assertEquals(rows[0].handler, "identity.snapshot");
  assertEquals(rows[0].status, "error");
  assertEquals(rows[0].error_message, "MCP not connected");
  assertEquals(rows[1].handler, "memory.summarize-daily");
  assertEquals(rows[1].status, "success");
  assertEquals(rows[1].result_summary, "Summarized 1 day(s)");

  db.close();
});

Deno.test("migration: pulse_runs rows fold into job_runs as pulse.execute", () => {
  const db = makeLegacyDb();

  const now = new Date().toISOString();
  db.exec(
    `INSERT INTO pulses
       (id, name, prompt_text, chain_pulse_ids, created_at, updated_at)
     VALUES ('p1', 'Test Pulse', 'do the thing', '[]', ?, ?)`,
    [now, now],
  );
  db.exec(
    `INSERT INTO pulse_runs
       (id, pulse_id, conversation_id, trigger_source, started_at,
        completed_at, duration_ms, status, result_summary,
        tool_calls_count, chain_depth, created_at)
     VALUES ('run1', 'p1', 'conv-a', 'cron', ?, ?, 12000, 'success',
             'did it', 3, 0, ?)`,
    ["2026-05-12T10:00:00.000Z", "2026-05-12T10:00:12.000Z", now],
  );

  initializeSchema(db);

  const row = db.prepare(
    `SELECT id, handler, schedule_id, payload_json, status,
            result_summary, duration_ms
       FROM job_runs WHERE id = 'run1'`,
  ).get<{
    id: string;
    handler: string;
    schedule_id: string | null;
    payload_json: string;
    status: string;
    result_summary: string;
    duration_ms: number;
  }>();
  assert(row);
  assertEquals(row.handler, "pulse.execute");
  // schedule_id is NULL on migrated rows — the live schedule is created
  // by the pulse engine at boot, not by the migration. The pulse link
  // travels in payload.pulseId, which is what the UI queries.
  assertEquals(row.schedule_id, null);
  assertEquals(row.status, "success");
  assertEquals(row.duration_ms, 12000);
  assertEquals(row.result_summary, "did it");

  const payload = JSON.parse(row.payload_json) as Record<string, unknown>;
  assertEquals(payload.pulseId, "p1");
  assertEquals(payload.triggerSource, "cron");
  assertEquals(payload.conversationId, "conv-a");
  assertEquals(payload.toolCallsCount, 3);

  db.close();
});

Deno.test("migration: in-flight 'running' pulse_runs become 'dead' with explanation", () => {
  const db = makeLegacyDb();

  const now = new Date().toISOString();
  db.exec(
    `INSERT INTO pulses
       (id, name, prompt_text, chain_pulse_ids, created_at, updated_at)
     VALUES ('p1', 'Test Pulse', 'do the thing', '[]', ?, ?)`,
    [now, now],
  );
  db.exec(
    `INSERT INTO pulse_runs
       (id, pulse_id, trigger_source, started_at, status, chain_depth, created_at)
     VALUES ('stuck-run', 'p1', 'cron', ?, 'running', 0, ?)`,
    ["2026-05-12T10:00:00.000Z", now],
  );

  initializeSchema(db);

  const row = db.prepare(
    `SELECT status, error_message, completed_at
       FROM job_runs WHERE id = 'stuck-run'`,
  ).get<{
    status: string;
    error_message: string;
    completed_at: string | null;
  }>();
  assert(row);
  assertEquals(row.status, "dead");
  assert(row.error_message?.includes("Reclaimed"));
  assert(row.completed_at !== null);

  db.close();
});

Deno.test("migration: pulse_run with chain parent preserves chain_parent_run_id in payload", () => {
  const db = makeLegacyDb();

  const now = new Date().toISOString();
  db.exec(
    `INSERT INTO pulses
       (id, name, prompt_text, chain_pulse_ids, created_at, updated_at)
     VALUES ('p1', 'Parent', '...', '[]', ?, ?)`,
    [now, now],
  );
  db.exec(
    `INSERT INTO pulses
       (id, name, prompt_text, chain_pulse_ids, created_at, updated_at)
     VALUES ('p2', 'Child', '...', '[]', ?, ?)`,
    [now, now],
  );
  db.exec(
    `INSERT INTO pulse_runs
       (id, pulse_id, trigger_source, started_at, completed_at,
        status, chain_depth, chain_parent_run_id, created_at)
     VALUES ('child-run', 'p2', 'chain', ?, ?, 'success', 1, 'parent-run', ?)`,
    ["2026-05-12T10:00:00.000Z", "2026-05-12T10:00:05.000Z", now],
  );

  initializeSchema(db);

  const row = db.prepare(
    "SELECT payload_json FROM job_runs WHERE id = 'child-run'",
  ).get<{ payload_json: string }>();
  assert(row);
  const payload = JSON.parse(row.payload_json) as Record<string, unknown>;
  assertEquals(payload.chainDepth, 1);
  assertEquals(payload.chainParentRunId, "parent-run");

  db.close();
});

Deno.test("migration: pulses table preserves all other data after rebuild", () => {
  const db = makeLegacyDb();

  const now = new Date().toISOString();
  db.exec(
    `INSERT INTO pulses
       (id, name, description, prompt_text, chat_mode, enabled,
        trigger_type, cron_expression, chain_pulse_ids, max_chain_depth,
        source, auto_delete, webhook_token,
        success_count, error_count, last_run_at, last_status,
        created_at, updated_at)
     VALUES ('p1', 'Daily Reflection', 'Reflect on the day',
             'What stood out today?', 'visible', 1,
             'cron', '0 22 * * *', '["p2"]', 5,
             'user', 0, 'webhook-token-abc',
             10, 2, ?, 'success', ?, ?)`,
    [now, now, now],
  );

  initializeSchema(db);

  const row = db.prepare("SELECT * FROM pulses WHERE id = 'p1'")
    .get<Record<string, unknown>>();
  assert(row);
  assertEquals(row.name, "Daily Reflection");
  assertEquals(row.description, "Reflect on the day");
  assertEquals(row.prompt_text, "What stood out today?");
  assertEquals(row.cron_expression, "0 22 * * *");
  assertEquals(row.max_chain_depth, 5);
  assertEquals(row.chain_pulse_ids, '["p2"]');
  assertEquals(row.source, "user");
  assertEquals(row.webhook_token, "webhook-token-abc");
  // The dying columns are gone — undefined when projected via PRAGMA.
  assertEquals(row.success_count, undefined);
  assertEquals(row.last_run_at, undefined);

  db.close();
});

Deno.test("migration: indexes are recreated on the rebuilt pulses table", () => {
  const db = makeLegacyDb();
  initializeSchema(db);

  const indexes = db.prepare(
    `SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='pulses'`,
  ).all<{ name: string }>().map((r) => r.name);

  // The three indexes we explicitly create.
  assert(indexes.includes("idx_pulses_enabled"));
  assert(indexes.includes("idx_pulses_trigger_type"));
  assert(indexes.includes("idx_pulses_conversation"));

  db.close();
});

Deno.test("migration: running it twice is idempotent (no-op on already-migrated DB)", () => {
  const db = makeLegacyDb();

  const now = new Date().toISOString();
  db.exec(
    `INSERT INTO pulses
       (id, name, prompt_text, chain_pulse_ids, created_at, updated_at)
     VALUES ('p1', 'Test', '...', '[]', ?, ?)`,
    [now, now],
  );
  db.exec(
    `INSERT INTO pulse_runs
       (id, pulse_id, trigger_source, started_at, completed_at,
        status, chain_depth, created_at)
     VALUES ('run-x', 'p1', 'cron', ?, ?, 'success', 0, ?)`,
    ["2026-05-12T10:00:00.000Z", "2026-05-12T10:00:05.000Z", now],
  );

  initializeSchema(db);
  const countAfterFirst = db.prepare(
    "SELECT COUNT(*) AS c FROM job_runs",
  ).get<{ c: number }>()?.c ?? 0;
  assertEquals(countAfterFirst, 1);

  // Second run: legacy tables are already gone, so the migration block
  // should detect that and do nothing.
  initializeSchema(db);
  const countAfterSecond = db.prepare(
    "SELECT COUNT(*) AS c FROM job_runs",
  ).get<{ c: number }>()?.c ?? 0;
  assertEquals(countAfterSecond, 1);

  db.close();
});
