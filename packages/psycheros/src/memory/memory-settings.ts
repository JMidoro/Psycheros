/**
 * Memory Settings Persistence
 *
 * Manages loading and saving custom daily memory-writing instructions.
 * Settings are stored in `.psycheros/memory-settings.json`.
 *
 * These instructions are injected into the daily summarization prompt so the
 * entity can follow user-defined preferences when writing daily memories.
 * Written from the entity's first-person perspective.
 */

import { join } from "@std/path";

// =============================================================================
// Types
// =============================================================================

/**
 * Memory settings — custom instructions for daily summarization.
 */
export interface MemorySettings {
  /** Custom instructions injected into the daily memory summarization prompt */
  dailyInstructions: string;
}

// =============================================================================
// Defaults
// =============================================================================

/**
 * Default memory settings — no custom instructions.
 */
export function getDefaultMemorySettings(): MemorySettings {
  return {
    dailyInstructions: "",
  };
}

// =============================================================================
// Load / Save
// =============================================================================

/**
 * Load memory settings from `.psycheros/memory-settings.json`.
 * Falls back to defaults when the file doesn't exist.
 */
export async function loadMemorySettings(
  dataRoot: string,
): Promise<MemorySettings> {
  const defaults = getDefaultMemorySettings();
  const settingsPath = join(dataRoot, ".psycheros", "memory-settings.json");

  try {
    const text = await Deno.readTextFile(settingsPath);
    const saved = JSON.parse(text) as Partial<MemorySettings>;
    return { ...defaults, ...saved };
  } catch {
    return defaults;
  }
}

/**
 * Save memory settings to `.psycheros/memory-settings.json`.
 */
export async function saveMemorySettings(
  dataRoot: string,
  settings: MemorySettings,
): Promise<void> {
  const settingsDir = join(dataRoot, ".psycheros");
  const settingsPath = join(settingsDir, "memory-settings.json");

  await Deno.mkdir(settingsDir, { recursive: true });
  await Deno.writeTextFile(
    settingsPath,
    JSON.stringify(settings, null, 2) + "\n",
  );
}
