#!/usr/bin/env -S deno run -A
/**
 * Rebuild the entire embedding cache.
 *
 * Deletes all cached embeddings and re-embeds every memory file,
 * applying chunking to long memories. Useful after schema changes,
 * chunking parameter changes, or cache corruption recovery.
 *
 * Usage:
 *   ENTITY_CORE_DATA_DIR=./data deno run -A scripts/rebuild-embedding-cache.ts [--dry-run]
 */

import { EmbeddingCache, getEmbedder } from "../src/embeddings/mod.ts";
import { FileStore } from "../src/storage/mod.ts";
import type { Granularity } from "../src/types.ts";

const DATA_DIR = Deno.env.get("ENTITY_CORE_DATA_DIR") || "./data";
const DRY_RUN = Deno.args.includes("--dry-run");
const VERBOSE = Deno.args.includes("--verbose");

const GRANULARITIES: Granularity[] = [
  "daily",
  "weekly",
  "monthly",
  "yearly",
  "significant",
];

async function main() {
  console.error(`[Rebuild] Data directory: ${DATA_DIR}`);
  if (DRY_RUN) {
    console.error("[Rebuild] Dry run mode — no changes will be made");
  }

  const store = new FileStore(DATA_DIR);
  const cache = new EmbeddingCache(DATA_DIR);
  await cache.initialize();

  const embedder = getEmbedder();
  await embedder.initialize();

  if (!embedder.isReady()) {
    console.error("[Rebuild] Failed to load embedding model. Cannot proceed.");
    Deno.exit(1);
  }

  const beforeStats = cache.getStats();
  console.error(
    `[Rebuild] Current cache: ${beforeStats.totalCached} memories, ${beforeStats.totalChunks} chunks`,
  );

  if (!DRY_RUN) {
    // Clear the cache
    const db = (cache as unknown as { db: { exec(sql: string): void } }).db;
    db.exec("DELETE FROM vec_memory_embeddings");
    db.exec("DELETE FROM memory_embeddings");
    console.error("[Rebuild] Cache cleared");
  }

  let embedded = 0;
  const skipped = 0;
  let failed = 0;

  for (const granularity of GRANULARITIES) {
    const memories = await store.listMemories(granularity);
    console.error(
      `[Rebuild] Processing ${memories.length} ${granularity} memories...`,
    );

    for (const memory of memories) {
      if (DRY_RUN) {
        embedded++;
        if (VERBOSE) {
          console.error(
            `  Would embed ${memory.id} (${memory.content.length} chars)`,
          );
        }
        continue;
      }

      try {
        const result = await cache.getOrCompute(memory, embedder);
        if (result) {
          embedded++;
          if (VERBOSE) {
            console.error(
              `  Embedded ${memory.id} (${memory.content.length} chars)`,
            );
          }
        } else {
          failed++;
          console.error(`  Failed to embed ${memory.id}`);
        }
      } catch (error) {
        failed++;
        console.error(
          `  Error embedding ${memory.id}: ${
            error instanceof Error ? error.message : error
          }`,
        );
      }
    }
  }

  const afterStats = DRY_RUN ? beforeStats : cache.getStats();

  console.error(`\n[Rebuild] Complete:`);
  console.error(`  Embedded: ${embedded}`);
  console.error(`  Skipped:  ${skipped}`);
  console.error(`  Failed:   ${failed}`);
  console.error(
    `  Cache now: ${afterStats.totalCached} memories, ${afterStats.totalChunks} chunks`,
  );

  cache.close();
}

main().catch((error) => {
  console.error("[Rebuild] Fatal error:", error);
  Deno.exit(1);
});
