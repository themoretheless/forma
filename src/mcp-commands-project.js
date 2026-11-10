import { validateProject } from './browser-storage.js';
import { buildNativeRustExport } from './native-export.js';

export function createProjectCommands(studio, helpers) {
  const { snapshot, requireFile, applyFiles } = helpers;
  return {
    ide_status() {
      return snapshot();
    },
    project_read() {
      return { ...studio.files };
    },
    project_export_native(args) {
      return buildNativeRustExport(studio.files, studio.entry, args);
    },
    file_read(args) {
      requireFile(args.path);
      return { path: args.path, content: studio.files[args.path] };
    },
    file_open(args) {
      requireFile(args.path);
      studio.open(args.path);
      return snapshot();
    },
    editor_fold(args) {
      studio.codeEditor.fold(args);
      return snapshot();
    },
    file_write(args) {
      const { files } = studio;
      if (typeof args.path !== 'string' || !args.path) throw Error('Invalid project path');
      validateProject({ [args.path]: args.content });
      if (Object.hasOwn(files, args.path) && args.expectedContent !== files[args.path])
        throw Error('File changed or expectedContent missing. Read it first.');
      files[args.path] = args.content;
      applyFiles();
      return snapshot();
    },
    project_replace(args) {
      const { files } = studio;
      if (
        JSON.stringify(Object.entries(files).sort()) !==
        JSON.stringify(Object.entries(args.expectedFiles).sort())
      )
        throw Error('Project changed. Read it first.');
      validateProject(args.files);
      studio.resetProjectEditing();
      const next = Object.fromEntries(Object.entries(args.files));
      studio.files = next;
      studio.active = Object.keys(next)[0];
      studio.entry = Object.keys(next).find(p => p.endsWith('.ui') && !p.endsWith('.design.ui'));
      studio.scenario = 0;
      applyFiles();
      return snapshot();
    },
  };
}
