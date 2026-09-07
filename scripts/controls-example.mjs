// Emit one concrete scene/template pair for the same native Runtime used by Studio.
import { readFileSync, readdirSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compileComponents } from '../src/components.js';
import { catalogProject } from '../vector-ui/controls/catalog.js';
import init, { text_metrics } from '../public/vector-pkg/forma.js';
const root = fileURLToPath(new URL('../', import.meta.url));
const [theme = 'light', size = 'regular', output = '.forma/controls'] = process.argv.slice(2);
const files = {};
for (const folder of ['components', 'assets']) for (const name of readdirSync(resolve(root, 'vector-ui/controls', folder))) {
  files[`${folder}/${name}`] = readFileSync(resolve(root, 'vector-ui/controls', folder, name), 'utf8');
}
await init({ module_or_path: readFileSync(resolve(root, 'public/vector-pkg/forma_bg.wasm')) });
const project = catalogProject(files, theme, size);
const result = compileComponents(project, 'ui/FormaControls.ui', {}, {
  measureText: (text, fontSize) => text_metrics(text, fontSize),
});
const directory = resolve(root, output);
mkdirSync(directory, { recursive: true });
for (const [name, data] of Object.entries({
  'scene.ui': result.source, 'template.ui': result.template,
  'project.json': JSON.stringify(project, null, 2) + '\n',
})) writeFileSync(resolve(directory, name), data);
console.log(`Forma ${theme}/${size}: ${result.instanceTree.nodes.filter(node => !['Frame', 'Scroll'].includes(node.type)).length} controls → ${directory}`);
console.log(`Native: cargo run --offline --release --manifest-path vector-ui/Cargo.toml --features native -- ${output}/scene.ui ${output}/template.ui`);
