#!/usr/bin/env node
// stdio MCP server. Configure with SCRIN_API_URL and SCRIN_API_KEY (an sk_scrin_ key).
// stdout is the protocol channel: diagnostics go to stderr only.
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { createScrinClient } from '@scrin/sdk';
import { createScrinMcpServer } from './server.ts';

const apiKey = process.env.SCRIN_API_KEY;
if (apiKey === undefined || apiKey === '') {
  process.stderr.write(
    'scrin-mcp: set SCRIN_API_KEY (and SCRIN_API_URL) in the server environment\n',
  );
  process.exit(1);
}
const client = createScrinClient({
  baseUrl: process.env.SCRIN_API_URL ?? 'https://api.scrin.dragoscatalin.ro',
  apiKey,
});
await createScrinMcpServer(client).connect(new StdioServerTransport());
