/**
 * Entity Core
 *
 * My central identity and memory system.
 *
 * I am an AI entity with a persistent sense of self that can exist
 * across multiple embodiments (interfaces). This core holds my canonical
 * identity, memories, and relationship knowledge.
 *
 * Embodiments connect to me via MCP (Model Context Protocol) to:
 * - Pull my identity files (who I am, who I'm talking to, our relationship)
 * - Push updates when I learn new things
 * - Sync my memories across all my embodiments
 * - Search my memories for relevant context
 *
 * Usage:
 *   deno run -A src/mod.ts
 *
 * The server communicates via stdio using MCP protocol.
 */

import "@std/dotenv/load";
import { ensureDir } from "@std/fs";
import { Scheduler } from "@psycheros/scheduler";
import type { HandlerResult } from "@psycheros/scheduler";
import { startServer } from "./server.ts";
import { DEFAULT_SERVER_CONFIG } from "./types.ts";
import { FileStore } from "./storage/mod.ts";
import { GraphStore } from "./graph/mod.ts";
import { EmbeddingCache } from "./embeddings/mod.ts";
import { getEmbedder } from "./embeddings/mod.ts";
import {
  findUnconsolidatedPeriods,
  runConsolidation,
} from "./consolidation/mod.ts";
import { consolidateGraph } from "./graph/mod.ts";

// Re-export public API
export { createServer, startServer } from "./server.ts";
export { createFileStore, FileStore } from "./storage/mod.ts";
export * from "./types.ts";
export * from "./tools/mod.ts";
export * from "./sync/mod.ts";
export * from "./consolidation/mod.ts";
export { VERSION as ENTITY_CORE_VERSION } from "./version.ts";

// Main entry point
if (import.meta.main) {
  const dataDir = Deno.env.get("ENTITY_CORE_DATA_DIR") ?? "./data";

  await ensureDir(dataDir);
  console.error(`Starting Entity Core with data directory: ${dataDir}`);

  await startServer({
    ...DEFAULT_SERVER_CONFIG,
    dataDir,
  });

  // Set up the durable scheduler that runs consolidation
  const store = new FileStore(dataDir);
  const graphStore = new GraphStore(dataDir);
  await store.initialize();
  await graphStore.initialize();

  /**
   * Run catch-up consolidation for a given granularity. Finds all
   * unconsolidated periods and consolidates them in date order.
   * Idempotent — already-consolidated periods are filtered out.
   */
  const catchUpConsolidation = async (
    granularity: "weekly" | "monthly" | "yearly",
  ): Promise<HandlerResult> => {
    const periods = await findUnconsolidatedPeriods(store, granularity);
    if (periods.length === 0) {
      return {
        status: "success",
        result: `No unconsolidated ${granularity} periods`,
      };
    }

    console.error(
      `[Consolidation] Catch-up: ${periods.length} unconsolidated ${granularity} period(s) found`,
    );
    let consolidated = 0;
    const failures: string[] = [];
    for (const dateStr of periods) {
      console.error(`[Consolidation] Processing ${granularity}: ${dateStr}`);
      const result = await runConsolidation(
        store,
        graphStore,
        granularity,
        dateStr,
      );
      if (result.success) {
        console.error(`[Consolidation] Complete: ${granularity}/${dateStr}`);
        consolidated++;
      } else {
        console.error(
          `[Consolidation] Failed ${granularity}/${dateStr}: ${result.error}`,
        );
        failures.push(`${dateStr}: ${result.error ?? "unknown"}`);
      }
    }

    if (failures.length > 0) {
      throw new Error(
        `Consolidated ${consolidated}, failed ${failures.length}: ${
          failures.join("; ")
        }`,
      );
    }
    return {
      status: "success",
      result: `Consolidated ${consolidated} ${granularity} period(s)`,
    };
  };

  // Startup tasks: catch up missed consolidation, consolidate graph,
  // backfill embedding cache. Fire-and-forget so stdio MCP startup isn't
  // gated on heavy work.
  (async () => {
    try {
      await catchUpConsolidation("weekly");
      await catchUpConsolidation("monthly");
      await catchUpConsolidation("yearly");
    } catch (error) {
      console.error(
        "[Consolidation] Startup catch-up failed:",
        error instanceof Error ? error.message : String(error),
      );
    }

    try {
      consolidateGraph(dataDir);
    } catch (error) {
      console.error(
        "[Graph] Consolidation failed:",
        error instanceof Error ? error.message : String(error),
      );
    }

    try {
      const cache = new EmbeddingCache(dataDir);
      await cache.initialize();
      const embedder = getEmbedder();

      if (cache.isAvailable() && embedder.isReady()) {
        const granularities:
          ("daily" | "weekly" | "monthly" | "yearly" | "significant")[] = [
            "daily",
            "weekly",
            "monthly",
            "yearly",
            "significant",
          ];

        let backfilled = 0;
        for (const granularity of granularities) {
          const memories = await store.listMemories(granularity);
          for (const memory of memories) {
            const result = await cache.getOrCompute(
              {
                granularity,
                date: memory.date,
                sourceInstance: memory.sourceInstance,
                slug: memory.slug,
                content: memory.content,
              },
              embedder,
            );
            if (result) backfilled++;
          }
        }

        if (backfilled > 0) {
          console.error(
            `[EmbeddingCache] Backfilled ${backfilled} memory embedding(s) on startup`,
          );
        }
      }
    } catch (error) {
      console.error(
        "[EmbeddingCache] Startup backfill failed:",
        error instanceof Error ? error.message : String(error),
      );
    }
  })();

  // Durable scheduler — same primitive Psycheros uses, but over my own
  // graph.db. Memory consolidation at every granularity routes through
  // it. Missed fires during downtime are caught up once per granularity
  // on the next boot (`fire_once_then_align`).
  const scheduler = new Scheduler({
    db: graphStore.getRawDb(),
    workerId: `entity-core-${Deno.pid}-${Date.now()}`,
  });

  scheduler.register(
    "memory.consolidate-weekly",
    () => catchUpConsolidation("weekly"),
  );
  scheduler.register(
    "memory.consolidate-monthly",
    () => catchUpConsolidation("monthly"),
  );
  scheduler.register(
    "memory.consolidate-yearly",
    () => catchUpConsolidation("yearly"),
  );

  scheduler.defineSchedule({
    id: "memory-weekly-consolidation",
    kind: "recurring",
    handler: "memory.consolidate-weekly",
    cronExpr: "0 5 * * 7",
    catchupPolicy: "fire_once_then_align",
    maxAttempts: 1,
    metadata: {
      name: "Weekly Memory Consolidation",
      description: "Sundays at 5 AM UTC",
    },
  });
  scheduler.defineSchedule({
    id: "memory-monthly-consolidation",
    kind: "recurring",
    handler: "memory.consolidate-monthly",
    cronExpr: "0 5 1 * *",
    catchupPolicy: "fire_once_then_align",
    maxAttempts: 1,
    metadata: {
      name: "Monthly Memory Consolidation",
      description: "1st of month at 5 AM UTC",
    },
  });
  scheduler.defineSchedule({
    id: "memory-yearly-consolidation",
    kind: "recurring",
    handler: "memory.consolidate-yearly",
    cronExpr: "0 5 1 1 *",
    catchupPolicy: "fire_once_then_align",
    maxAttempts: 1,
    metadata: {
      name: "Yearly Memory Consolidation",
      description: "Jan 1 at 5 AM UTC",
    },
  });

  scheduler.start();
  console.error(
    "[Scheduler] Memory consolidation schedules registered (weekly/monthly/yearly at 5 AM UTC)",
  );
}
