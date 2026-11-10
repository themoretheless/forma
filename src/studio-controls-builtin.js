import { createStudioControlsProject } from './studio-controls-project.js';
import { controlsLibrary } from './controls-library.js';
import { stripPathPrefix } from './shared/text.js';
const sources = import.meta.glob('../studio-controls/{components,ui}/*.ui', {
  query: '?raw',
  import: 'default',
  eager: true,
});
const notes = import.meta.glob('../studio-controls/README.md', {
  query: '?raw',
  import: 'default',
  eager: true,
});
export const studioControlsProject = createStudioControlsProject(controlsLibrary, {
  ...stripPathPrefix(sources, '../studio-controls/'),
  ...stripPathPrefix(notes, '../studio-controls/'),
});
