/**
 * MCP Sync Tool
 *
 * Allows the entity to manually trigger a sync with entity-core.
 * This pulls the latest identity files and pushes any pending changes.
 */

import type { ToolResult } from "../types.ts";
import type { Tool, ToolContext } from "./types.ts";

/**
 * The sync_mcp tool allows the entity to sync with entity-core.
 */
export const syncMcpTool: Tool = {
  definition: {
    type: "function",
    function: {
      name: "sync_mcp",
      description:
        "Sync with entity-core to get my latest identity files and push any pending changes. " +
        "I use this when I want to ensure I have the most up-to-date identity information, " +
        "or after making changes that should be synced to the central entity-core server.",
      parameters: {
        type: "object",
        properties: {
          reason: {
            type: "string",
            description: "Optional reason for syncing (for logging)",
          },
        },
        required: [],
      },
    },
  },

  execute: async (
    args: Record<string, unknown>,
    ctx: ToolContext,
  ): Promise<ToolResult> => {
    const mcpClient = ctx.config.mcpClient;

    if (!mcpClient) {
      return {
        toolCallId: ctx.toolCallId,
        content:
          "MCP is not enabled. Set PSYCHEROS_MCP_ENABLED=true to use entity-core sync.",
        isError: true,
      };
    }

    const reason = args.reason as string | undefined;
    const logPrefix = reason ? `[sync_mcp: ${reason}]` : "[sync_mcp]";

    try {
      // Check if connected
      if (!mcpClient.isConnected()) {
        return {
          toolCallId: ctx.toolCallId,
          content:
            `${logPrefix} MCP client is not connected to entity-core. Changes remain queued for later sync.`,
          isError: false,
        };
      }

      // Pull canonical identity from entity-core. Pending local writes
      // are pushed automatically by the scheduler's
      // `mcp.push-identity-change` jobs — I just report how many are
      // still in flight.
      const identity = await mcpClient.pull();
      const pending = mcpClient.getPendingCount();

      const parts: string[] = [`${logPrefix} Sync completed.`];

      if (identity) {
        const selfCount = identity.self.length;
        const userCount = identity.user.length;
        const relationshipCount = identity.relationship.length;
        parts.push(
          `Pulled ${selfCount} self, ${userCount} user, ${relationshipCount} relationship files.`,
        );
      }

      if (pending.identity > 0) {
        parts.push(
          `${pending.identity} identity write(s) pending — will sync within ~5s.`,
        );
      } else {
        parts.push("No pending local writes.");
      }

      return {
        toolCallId: ctx.toolCallId,
        content: parts.join(" "),
        isError: false,
      };
    } catch (error) {
      const errorMessage = error instanceof Error
        ? error.message
        : String(error);
      return {
        toolCallId: ctx.toolCallId,
        content: `${logPrefix} Sync failed: ${errorMessage}`,
        isError: true,
      };
    }
  },
};
