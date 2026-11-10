import {
  designStatesSummary,
  stateCreateEdit,
  stateDeleteEdit,
  statePropertyEdit,
  stateRenameEdit,
} from './design-states.js';
import { copyElement, insertElement, moveAmongSiblings, removeElement } from './element-edit.js';
import { insertableControls } from './control-catalog.js';

export function createDesignCommands(studio, helpers) {
  const { $ } = studio;
  const { snapshot, requireFile, findNode } = helpers;

  function spliceComponent(command, args) {
    // All four splice the entry file the compiled tree came from, against its live text. The
    // splice reports the offset the new, moved or duplicated node takes — for a delete, the
    // parent that keeps the selection — and that is what the preview is asked to select:
    // without it the agent would have to re-read `component_tree` to find its own work.
    const { entry, error } = studio;
    const source = studio.liveSource(entry);
    if (error || source !== args.expectedContent)
      throw Error('Stale or invalid source. Read the entry file first.');
    findNode(args.start);
    const change =
      command === 'component_insert'
        ? insertElement(source, args.start, args.markup, false, args.side ?? 'auto')
        : command === 'component_move'
          ? moveAmongSiblings(source, args.start, args.target, args.side)
          : // A duplicate is the node's own text spliced back after itself, so the key uniquifying
            // the paste path does covers a copied `key:` too. The copy keeps the source's
            // coordinates: the bridge also runs on the HTML preview, where no scene measured an
            // offset to take, and `component_property` can move it from there.
            command === 'component_delete'
            ? removeElement(source, args.start)
            : insertElement(source, args.start, copyElement(source, args.start), false, 'after');
    // A move onto the slot the node already holds is a no-op the primitive reports as null.
    // The caller still gets the file text back, because that is the content its next write
    // has to quote as `expectedContent`.
    if (!change) return { start: args.start, content: source, changed: false };
    studio.commitSource({ file: entry, source, ...change, reselect: change.start });
    return {
      start: change.start,
      content: source.slice(0, change.from) + change.insert + source.slice(change.to),
      changed: true,
    };
  }

  return {
    designer_tree() {
      return studio.controlTree.snapshot();
    },
    designer_tree_view(args) {
      studio.controlTree.setView(args);
      return studio.controlTree.snapshot();
    },
    designer_tree_select(args) {
      studio.controlTree.selectId(args.id);
      return studio.controlTree.snapshot();
    },
    designer_tree_fold(args) {
      studio.controlTree.fold(args.id, args.collapsed);
      return studio.controlTree.snapshot();
    },
    preview_renderer(args) {
      studio.rendererPicker.value = args.renderer;
      studio.rendererPicker.onchange();
      return snapshot();
    },
    vector_status() {
      return studio.vectorPreview?.snapshot() ?? { ready: false };
    },
    preview_open(args) {
      requireFile(args.path);
      studio.entry = args.path;
      studio.scenario = 0;
      studio.compile(true);
      return snapshot();
    },
    preview_scenario(args) {
      const { compiled } = studio;
      if (args.name === null) {
        if (!compiled?.states?.length) throw Error('The component has no design states');
        $('scenario').value = -1;
        $('scenario').onchange({ target: $('scenario') });
        return snapshot();
      }
      const index = (compiled?.states ?? []).findIndex(s => s.name === args.name);
      if (index < 0) throw Error('Design state not found');
      studio.scenario = index;
      $('scenario').value = index;
      $('scenario').onchange({ target: $('scenario') });
      return snapshot();
    },
    // Same target as the states panel: the file that panel authors, never an arbitrary design file
    // the entry does not use. An agent that wants another one edits it through file_write.
    design_states_read() {
      return designStatesSummary(studio.designStateFile().source);
    },
    design_state_create(args) {
      return studio.designStateEdit(
        source => stateCreateEdit(source, args.name),
        args.expectedContent,
      );
    },
    design_state_rename(args) {
      return studio.designStateEdit(
        source => stateRenameEdit(source, args.name, args.newName),
        args.expectedContent,
      );
    },
    design_state_delete(args) {
      return studio.designStateEdit(
        source => stateDeleteEdit(source, args.name),
        args.expectedContent,
      );
    },
    // The state block is named, not indexed, so a rename between the read and this call cannot land
    // a property in the wrong block; `null` is the base block, which is what the panel's own row is.
    design_state_property(args) {
      return studio.designStateEdit(
        source =>
          statePropertyEdit(
            source,
            args.state ?? null,
            args.type,
            args.key,
            args.property,
            args.value,
          ),
        args.expectedContent,
      );
    },
    preview_mode(args) {
      if (studio.mode !== args.mode) $('run').click();
      return snapshot();
    },
    preview_viewport(args) {
      $(args.device).click();
      $('preview').classList.toggle('light', args.theme === 'light');
      return snapshot();
    },
    component_tree() {
      return { entry: studio.entry, nodes: studio.compiled?.nodes ?? [], error: studio.error };
    },
    component_spacing() {
      return studio.spacingOverlay.read();
    },
    component_select(args) {
      if (studio.error) throw Error('Fix diagnostics before selecting compiled source');
      studio.select(findNode(args.start));
      return snapshot();
    },
    component_property(args) {
      const { files, entry, error } = studio;
      if (error || files[entry] !== args.expectedContent)
        throw Error('Stale or invalid source. Read entry file first.');
      const n = findNode(args.start);
      if (!Object.hasOwn(n.props, args.property) || typeof n.props[args.property] === 'object')
        throw Error('Only existing literal properties are supported');
      if (typeof n.props[args.property] !== typeof args.value)
        throw Error('Property type mismatch');
      studio.select(n);
      const input = studio.propertyInput(args.property);
      if (!input) throw Error('Свойство недоступно для правки: включите исходный вид');
      input.value = String(args.value);
      input.onchange();
      return snapshot();
    },
    // The palette's own list, so an agent inserts exactly what a designer can insert.
    component_controls() {
      return insertableControls(studio.files);
    },
    component_insert(args) {
      return spliceComponent('component_insert', args);
    },
    component_move(args) {
      return spliceComponent('component_move', args);
    },
    component_delete(args) {
      return spliceComponent('component_delete', args);
    },
    component_duplicate(args) {
      return spliceComponent('component_duplicate', args);
    },
  };
}
