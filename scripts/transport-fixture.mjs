// Writes the transport files consumed by `cargo run --example template_load_perf`.
// The fixture matches scripts/compiler-session-bench.mjs so JS and native phases
// describe the same scene; outputs live under the ignored .forma/ directory.
import {mkdir, readFile, writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {resolve} from 'node:path';
import {createComponentCompiler} from '../src/components.js';

const out = resolve(process.argv[2] ?? '.forma/perf/template-load');
const read = name => readFile(new URL(`../vector-ui/examples/${name}`, import.meta.url), 'utf8');

const components = {
  'components/Button.ui': await read('Button.slots.ui'),
  'components/ImageButton.ui': await read('ImageButton.component.ui'),
  'assets/search.svg': await read('search.svg'),
};
const compile = createComponentCompiler();
const rows = [];
for (const count of [1, 32, 256]) {
  const controls = Array.from({length: count}, (_, i) => `ImageButton { text: 'Item ${i}'; width: 180; height: 50; }`).join(' ');
  const files = {...components, 'ui/Demo.ui': `component Demo { Frame { width: 900; height: 600; ${controls} } }`};
  const {source, template} = compile(files, 'ui/Demo.ui');
  await mkdir(out, {recursive: true});
  const path = resolve(out, `forma-transport-${count}.txt`);
  await writeFile(path, `${source}\n@@@SPLIT@@@\n${template}`);
  rows.push({
    count,
    sourceBytes: source.length,
    templateBytes: template.length,
    transportSha256: createHash('sha256').update(source + template).digest('hex'),
  });
}
console.log(JSON.stringify(rows, null, 2));
