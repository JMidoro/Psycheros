/**
 * Pulse chain integrity tests.
 *
 * Exercises `DBClient.detectPulseChainCycle()` — the load-bearing piece
 * that prevents a pulse from re-entering an in-progress chain (which
 * would otherwise produce an unbounded recursive chain of LLM calls).
 *
 * The function walks job_runs backwards via the `chainParentRunId` link
 * stored in `payload.chainParentRunId`, looking for either:
 *   - The candidate pulse already appearing in the ancestor chain
 *     (re-entry attempt)
 *   - A self-referential link in job_runs (corrupted graph)
 */

import { assertEquals } from "@std/assert";
import { Database } from "@db/sqlite";
import { initializeSchema } from "../src/db/schema.ts";
import { DBClient } from "../src/db/client.ts";

function makePopulatedDb(): {
  db: Database;
  client: DBClient;
  cleanup: () => void;
} {
  // Temp file-backed DB so DBClient can open it normally.
  const path = Deno.makeTempFileSync({
    prefix: "psycheros-chain-test-",
    suffix: ".db",
  });
  const raw = new Database(path);
  initializeSchema(raw);
  raw.close();
  const client = new DBClient(path);
  const cleanup = () => {
    client.close();
    try {
      Deno.removeSync(path);
    } catch { /* already gone */ }
  };
  return { db: client.getRawDb(), client, cleanup };
}

function insertChainRun(
  db: Database,
  opts: {
    id: string;
    pulseId: string;
    parentRunId: string | null;
    chainDepth: number;
  },
): void {
  const now = new Date().toISOString();
  const payload = JSON.stringify({
    pulseId: opts.pulseId,
    triggerSource: "chain",
    chainDepth: opts.chainDepth,
    chainParentRunId: opts.parentRunId,
  });
  db.exec(
    `INSERT INTO job_runs
       (id, schedule_id, handler, payload_json, status, attempt,
        max_attempts, scheduled_for, started_at, completed_at,
        result_summary, created_at)
     VALUES (?, NULL, 'pulse.execute', ?, 'success', 1, 1, ?, ?, ?, '', ?)`,
    [opts.id, payload, now, now, now, now],
  );
}

Deno.test("detectPulseChainCycle: no parent → no cycle", () => {
  const { client, cleanup } = makePopulatedDb();
  assertEquals(client.detectPulseChainCycle("any-pulse", null), false);
  cleanup();
});

Deno.test("detectPulseChainCycle: linear chain A→B→C, no cycle", () => {
  const { db, client, cleanup } = makePopulatedDb();
  insertChainRun(db, {
    id: "run-A",
    pulseId: "A",
    parentRunId: null,
    chainDepth: 0,
  });
  insertChainRun(db, {
    id: "run-B",
    pulseId: "B",
    parentRunId: "run-A",
    chainDepth: 1,
  });
  insertChainRun(db, {
    id: "run-C",
    pulseId: "C",
    parentRunId: "run-B",
    chainDepth: 2,
  });

  // Asking "would adding pulse D under run-C cause a cycle?" — no.
  assertEquals(client.detectPulseChainCycle("D", "run-C"), false);
  cleanup();
});

Deno.test("detectPulseChainCycle: A→B→A re-entry detected", () => {
  const { db, client, cleanup } = makePopulatedDb();
  insertChainRun(db, {
    id: "run-A",
    pulseId: "A",
    parentRunId: null,
    chainDepth: 0,
  });
  insertChainRun(db, {
    id: "run-B",
    pulseId: "B",
    parentRunId: "run-A",
    chainDepth: 1,
  });

  // Asking "would adding pulse A under run-B cause a cycle?" — yes,
  // A is already in the ancestor chain.
  assertEquals(client.detectPulseChainCycle("A", "run-B"), true);
  cleanup();
});

Deno.test("detectPulseChainCycle: A→B→C→A re-entry detected three levels up", () => {
  const { db, client, cleanup } = makePopulatedDb();
  insertChainRun(db, {
    id: "run-A",
    pulseId: "A",
    parentRunId: null,
    chainDepth: 0,
  });
  insertChainRun(db, {
    id: "run-B",
    pulseId: "B",
    parentRunId: "run-A",
    chainDepth: 1,
  });
  insertChainRun(db, {
    id: "run-C",
    pulseId: "C",
    parentRunId: "run-B",
    chainDepth: 2,
  });

  assertEquals(client.detectPulseChainCycle("A", "run-C"), true);
  cleanup();
});

Deno.test("detectPulseChainCycle: self-referential parent → cycle detected (corrupted graph)", () => {
  const { db, client, cleanup } = makePopulatedDb();
  // run-X claims itself as parent — should be caught by visited-set guard.
  insertChainRun(db, {
    id: "run-X",
    pulseId: "X",
    parentRunId: "run-X",
    chainDepth: 5,
  });

  assertEquals(client.detectPulseChainCycle("Y", "run-X"), true);
  cleanup();
});

Deno.test("detectPulseChainCycle: missing parent run terminates walk safely", () => {
  const { client, cleanup } = makePopulatedDb();
  // Parent run doesn't exist — the walk breaks and returns false.
  assertEquals(client.detectPulseChainCycle("A", "nonexistent-run"), false);
  cleanup();
});
