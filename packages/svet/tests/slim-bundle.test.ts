// @vitest-environment node
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { build } from 'vite';

// Markers of the three things the slim entry exists to keep out of an application bundle.
const EMBEDDED_WASM = 'AGFzbQEAAAAB'; // base64 of the `\0asm` header that opens `WASM_BASE64`
const GPU_MODULE = 'GPU tensor belongs to a different reactive context';
const FORMA_PARSER = 'Duplicate Forma range contract annotation';

const dist = resolve(import.meta.dirname, '../dist/package');
let workDir: string;

/** Bundles a consumer the way an application would (`npm test` builds `dist/package` first). */
async function bundle(entry: 'index.js' | 'slim.js'): Promise<string> {
  const engine = pathToFileURL(join(dist, entry)).href;
  const file = join(workDir, `${entry}.consumer.mjs`);
  await writeFile(
    file,
    `
    import { instantiateFormaAst, signal, h, mount } from ${JSON.stringify(engine)};
    const ast = ${JSON.stringify({ componentName: 'Card' })};
    export function start(root) {
      const count = signal(0);
      mount(() => h('button', { onClick: () => count.set(count() + 1) }, () => String(count())), root);
      return instantiateFormaAst(ast);
    }
  `,
  );
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: { write: false, minify: false, lib: { entry: file, formats: ['es'] } },
  });
  const outputs = Array.isArray(result) ? result : [result];
  return outputs
    .flatMap(item => ('output' in item ? item.output : []))
    .map(chunk => ('code' in chunk ? chunk.code : ''))
    .join('\n');
}

describe('bundle contents', () => {
  beforeAll(async () => {
    workDir = await mkdtemp(join(tmpdir(), 'svet-slim-bundle-'));
  });
  afterAll(async () => {
    await rm(workDir, { recursive: true, force: true });
  });

  it('the self-contained entry carries the embedded core and the 3D Forma tags', async () => {
    const code = await bundle('index.js');
    expect(code).toContain(EMBEDDED_WASM);
    expect(code).toContain(GPU_MODULE);
  });

  it('the slim entry leaves out the embedded core, the GPU module and the .ui parser', async () => {
    const code = await bundle('slim.js');
    expect(code).not.toContain(EMBEDDED_WASM);
    expect(code).not.toContain(GPU_MODULE);
    expect(code).not.toContain(FORMA_PARSER);
    // …while the runtime itself is there.
    expect(code).toContain('Svet WASM core is not loaded');
  });
});
