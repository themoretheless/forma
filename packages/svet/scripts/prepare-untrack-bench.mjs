import { createHash } from 'node:crypto';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const output = resolve(
  root,
  'node_modules/.cache/svet-untrack-core-bench',
  `snapshot-${Date.now()}`,
);
await mkdir(output, { recursive: true });
await writeFile(resolve(output, 'package.json'), '{"type":"module","private":true}\n');
const files = (await readdir(resolve(root, 'src/engine')))
  .filter(file => file.endsWith('.ts'))
  .sort();
const replace = (text, from, to) => {
  if (!text.includes(from))
    throw Error('Untrack implementation changed; update the benchmark transformation explicitly');
  return text.replace(from, to);
};
function baseline(text) {
  if (!text.includes('allocateReactiveNode'))
    throw Error('Managed node-allocation correctness fix is required');
  text = replace(
    text,
    '  /** Live JS frames across generations; reset must not clear this count. */\n  observationDepth: number;\n',
    '',
  );
  text = replace(
    text,
    'function enterObservationContext(ctx: JsReactiveContext): void {\n  if (ctx.observationDepth++ === 0) activeObservationContexts.push(ctx);\n}\nfunction leaveObservationContext(ctx: JsReactiveContext): void {\n  if (--ctx.observationDepth === 0) activeObservationContexts.pop();\n}\n',
    '',
  );
  text = replace(text, '      observationDepth: 0,\n', '');
  text = replace(text, 'enterObservationContext(ctx);', 'activeObservationContexts.push(ctx);');
  text = replace(text, 'leaveObservationContext(ctx);', 'activeObservationContexts.pop();');
  text = replace(text, 'enterObservationContext(x);', 'activeObservationContexts.push(x);');
  text = replace(text, 'leaveObservationContext(x);', 'activeObservationContexts.pop();');
  text = replace(
    text,
    '  // Retired generations can still have JS frames on the stack; their finally\n  // blocks, not reset, release observationDepth and the shared context entry.\n',
    '',
  );
  text = text.replace(
    '// Each synchronously observed core appears once, independent of nesting depth.',
    '// Benchmark baseline: one context entry per observer invocation.',
  );
  if (/observationDepth|enterObservationContext|leaveObservationContext/.test(text))
    throw Error('Incomplete baseline transformation');
  return text;
}
const metadata = {
  createdAt: new Date().toISOString(),
  node: process.version,
  typescript: ts.version,
  variants: {},
  scope:
    'Both variants retain current native cache and state-view fixes; only context registration differs.',
};
for (const variant of ['baseline', 'candidate']) {
  const directory = resolve(output, variant);
  await mkdir(directory, { recursive: true });
  const hashes = {};
  for (const file of files) {
    let text = await readFile(resolve(root, 'src/engine', file), 'utf8');
    if (file === 'reactivity.ts' && variant === 'baseline') text = baseline(text);
    const result = ts.transpileModule(text, {
      fileName: file,
      compilerOptions: {
        target: ts.ScriptTarget.ES2022,
        module: ts.ModuleKind.ESNext,
        rewriteRelativeImportExtensions: true,
      },
    });
    hashes[file] = {
      source: createHash('sha256').update(text).digest('hex'),
      javascript: createHash('sha256').update(result.outputText).digest('hex'),
    };
    await writeFile(resolve(directory, file.replace(/\.ts$/, '.js')), result.outputText);
  }
  metadata.variants[variant] = hashes;
}
await writeFile(resolve(output, 'metadata.json'), JSON.stringify(metadata, null, 2) + '\n');
console.log(JSON.stringify({ output }, null, 2));
