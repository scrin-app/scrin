import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import { ScrinApiError, unwrap, type ScrinClient } from '@scrin/sdk';
import { z } from 'zod';

export const SERVER_NAME = 'scrin';
export const SERVER_VERSION = '0.1.0';

const id = (what: string) => z.string().min(4).max(64).describe(`${what} id`);

function ok(data: unknown): CallToolResult {
  return {
    content: [{ type: 'text', text: JSON.stringify(data, null, 2) }],
    structuredContent: { result: data },
  };
}

function fail(e: unknown): CallToolResult {
  const message =
    e instanceof ScrinApiError
      ? `scrin API error ${e.status} ${e.code}: ${e.message}`
      : `error: ${e instanceof Error ? e.message : String(e)}`;
  return { isError: true, content: [{ type: 'text', text: message }] };
}

async function guard(fn: () => Promise<unknown>): Promise<CallToolResult> {
  try {
    return ok(await fn());
  } catch (e) {
    return fail(e);
  }
}

/** MCP tools over the scrin accounts API. Every call goes through the typed SDK. */
export function createScrinMcpServer(client: ScrinClient): McpServer {
  const server = new McpServer({ name: SERVER_NAME, version: SERVER_VERSION });

  server.registerTool(
    'list_devices',
    {
      title: 'List devices',
      description:
        'List devices registered in the organisation, optionally filtered by group, tag or search text.',
      inputSchema: {
        group: id('Device group').optional(),
        tag: z.string().max(40).optional(),
        search: z.string().max(100).optional().describe('Name substring or exact 9-digit scrin ID'),
        limit: z.number().int().min(1).max(200).default(50),
      },
      annotations: { readOnlyHint: true, openWorldHint: false },
    },
    ({ group, tag, search, limit }) =>
      guard(() =>
        unwrap(
          client.GET('/v1/devices', {
            params: {
              query: {
                limit,
                offset: 0,
                ...(group === undefined ? {} : { group }),
                ...(tag === undefined ? {} : { tag }),
                ...(search === undefined ? {} : { q: search }),
              },
            },
          }),
        ),
      ),
  );

  server.registerTool(
    'get_device',
    {
      title: 'Get device',
      description: 'Get one device by its id (dev_…).',
      inputSchema: { deviceId: id('Device') },
      annotations: { readOnlyHint: true, openWorldHint: false },
    },
    ({ deviceId }) =>
      guard(() => unwrap(client.GET('/v1/devices/{id}', { params: { path: { id: deviceId } } }))),
  );

  server.registerTool(
    'list_sessions',
    {
      title: 'List sessions',
      description: 'List signed session logs (newest first), optionally for one device.',
      inputSchema: {
        deviceId: id('Device').optional(),
        limit: z.number().int().min(1).max(200).default(50),
      },
      annotations: { readOnlyHint: true, openWorldHint: false },
    },
    ({ deviceId, limit }) =>
      guard(() =>
        unwrap(
          client.GET('/v1/sessions', {
            params: { query: { limit, ...(deviceId === undefined ? {} : { device: deviceId }) } },
          }),
        ),
      ),
  );

  server.registerTool(
    'verify_audit',
    {
      title: 'Verify audit chain',
      description:
        'Recompute the organisation audit hash chain. Returns valid=false and the first broken record if any row was altered or removed.',
      inputSchema: {},
      annotations: { readOnlyHint: true, openWorldHint: false },
    },
    () => guard(() => unwrap(client.GET('/v1/audit/verify'))),
  );

  server.registerTool(
    'request_jit_access',
    {
      title: 'Request JIT access',
      description:
        'Request time-boxed access to a device. Creates a pending grant that a different admin must approve.',
      inputSchema: {
        deviceId: id('Device'),
        reason: z.string().min(3).max(500),
        durationMinutes: z.number().int().min(5).max(1440).default(60),
      },
      annotations: {
        readOnlyHint: false,
        destructiveHint: false,
        idempotentHint: false,
        openWorldHint: false,
      },
    },
    ({ deviceId, reason, durationMinutes }) =>
      guard(() => unwrap(client.POST('/v1/jit', { body: { deviceId, reason, durationMinutes } }))),
  );

  server.registerTool(
    'approve_jit',
    {
      title: 'Approve JIT access',
      description:
        'Approve a pending JIT grant (requires jit:approve; nobody can approve their own request). This grants real access to a machine — confirm with the user first.',
      inputSchema: { grantId: id('JIT grant'), note: z.string().max(500).optional() },
      annotations: {
        readOnlyHint: false,
        destructiveHint: true,
        idempotentHint: false,
        openWorldHint: false,
      },
    },
    ({ grantId, note }) =>
      guard(() =>
        unwrap(
          client.POST('/v1/jit/{id}/approve', {
            params: { path: { id: grantId } },
            body: note === undefined ? {} : { note },
          }),
        ),
      ),
  );

  return server;
}
