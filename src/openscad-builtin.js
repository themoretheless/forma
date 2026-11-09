import { createOpenScadProject } from './openscad-project.js';
import { controlsLibrary } from './controls-library.js';
import { stripPathPrefix } from './shared/text.js';
const sources = import.meta.glob('../openscad-ui/ui/*.ui', {
  query: '?raw',
  import: 'default',
  eager: true,
});
const rust = import.meta.glob('../openscad-ui/{Cargo.toml,src/*.rs}', {
  query: '?raw',
  import: 'default',
  eager: true,
});
export const openScadProject = createOpenScadProject(controlsLibrary, {
  ...stripPathPrefix(sources, '../openscad-ui/'),
  ...stripPathPrefix(rust, '../openscad-ui/'),
});
