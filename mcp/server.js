import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { readFile } from 'node:fs/promises';
import { z } from 'zod';
import { MCP_TOOLS } from '../protocol/mcp-tools.js';

const server = new McpServer({ name: 'forma-studio', version: '0.1.0' });
const pathSchema = z.string().min(1),
  scalarSchema = z.union([z.string(), z.number(), z.boolean(), z.null()]);

function fieldToZod(field) {
  let schema;
  switch (field.type) {
    case 'path':
      schema = pathSchema;
      break;
    case 'string':
      schema = z.string();
      break;
    case 'boolean':
      schema = z.boolean();
      break;
    case 'int':
      schema = z.number().int();
      break;
    case 'nonnegativeInt':
      schema = z.number().int().nonnegative();
      break;
    case 'positiveInt':
      schema = z.number().int().positive();
      break;
    case 'enum':
      schema = z.enum(field.values);
      break;
    case 'scalar':
      schema = scalarSchema;
      break;
    case 'stringRecord':
      schema = z.record(z.string(), z.string());
      break;
    case 'scalarRecord':
      schema = z.record(z.string(), scalarSchema);
      break;
    default:
      throw Error(`Unsupported MCP field type: ${field.type}`);
  }
  if (field.nullable) schema = schema.nullable();
  if (field.optional) schema = schema.optional();
  return schema;
}

function buildInputSchema(descriptor) {
  return Object.fromEntries(
    Object.entries(descriptor).map(([key, field]) => [key, fieldToZod(field)]),
  );
}

async function request(command, args) {
  const descriptor = JSON.parse(
    await readFile(new URL('../.forma/bridge.json', import.meta.url), 'utf8'),
  );
  if (descriptor.url !== 'http://127.0.0.1:5173/__forma_mcp') throw Error('Invalid bridge address');
  const response = await fetch(descriptor.url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${descriptor.token}` },
    body: JSON.stringify({ command, args }),
    signal: AbortSignal.timeout(10000),
  });
  const result = await response.json();
  if (!response.ok || result.error) throw Error(result.error || 'Bridge failed');
  return result.result;
}

for (const tool of MCP_TOOLS) {
  server.registerTool(
    tool.name,
    {
      description: tool.description,
      inputSchema: buildInputSchema(tool.schema),
      annotations: {
        readOnlyHint: tool.readOnly,
        destructiveHint: Boolean(tool.destructive),
        openWorldHint: false,
      },
    },
    async args => {
      try {
        return {
          content: [
            { type: 'text', text: JSON.stringify(await request(tool.name, args), null, 2) },
          ],
        };
      } catch (e) {
        return { isError: true, content: [{ type: 'text', text: e.message }] };
      }
    },
  );
}

server.registerResource(
  'language',
  'forma://language',
  { description: 'Implemented language and IDE limitations' },
  async uri => ({
    contents: [
      {
        uri: uri.href,
        text: await readFile(new URL('../README.md', import.meta.url), 'utf8'),
        mimeType: 'text/markdown',
      },
    ],
  }),
);
await server.connect(new StdioServerTransport());
