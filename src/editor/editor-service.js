// @ts-check
import { mountEditor } from './editor.js';
import { detectTokenizerLanguage } from './tokenizer-highlight.js';
import { formatSource, parse } from '../compiler/language.js';

/**
 * Coordinates CodeMirror/textarea editing, source committing, and cursor synchronization.
 * @param {{ $: (id: string) => any, appState: any, onSaveStatus: (status: 'changed' | 'saved' | 'damaged', text: string) => void, onPersist: () => void, onCompile: (reset?: boolean) => void, onOpen: (path: string) => void, onHighlightUpdate?: () => void, onSourceInputResetSelection: () => void, onFormatSuccess?: (msg: string) => void, reportError: (err: any) => void }} options
 */
export function createEditorService({
  $,
  appState,
  onSaveStatus,
  onPersist,
  onCompile,
  onOpen,
  onHighlightUpdate,
  onSourceInputResetSelection,
  onFormatSuccess,
  reportError,
}) {
  const codeEditor = mountEditor($('code'), {
    getProject: () => ({ files: appState.files, path: appState.active }),
  });

  function highlightSource() {
    onHighlightUpdate?.();
    codeEditor.highlight(
      appState.selected && appState.active === appState.selectedPath ? appState.selected : null,
    );
  }

  function lines() {
    const { line, column } = codeEditor.position();
    $('position').textContent = `Ln ${line}, Col ${column}`;
    highlightSource();
  }

  function syncOpenDocument(path) {
    codeEditor.setLanguage(path);
    $('filename').textContent = path.split('/').at(-1);
    $('code').value = appState.files[path];
    const langInfo = detectTokenizerLanguage(path);
    $('language').textContent = langInfo ? langInfo.label : 'Текст';
    $('entry').disabled = !path.endsWith('.ui') || path.endsWith('.design.ui');
    $('format-code').disabled = !path.endsWith('.ui');
    lines();
  }

  function formatActiveFile() {
    if (!appState.active?.endsWith('.ui')) return;
    try {
      const current = $('code').value;
      const formatted = formatSource(current);
      if (formatted && formatted !== current) {
        codeEditor.edit({ from: 0, to: current.length, insert: formatted });
        onFormatSuccess?.('Код отформатирован');
      }
    } catch (e) {
      reportError(e);
    }
  }

  /**
   * @param {{ file: string, source: string, from: number, to: number, insert: string, reselect?: number, after?: () => void }} spec
   * @param {{ onReselect?: (start: number) => void }} [hooks]
   */
  function commitSource(
    { file, source, from, to, insert, reselect, after },
    { onReselect = undefined } = {},
  ) {
    if (appState.liveSource(file, () => $('code').value) !== source) {
      throw Error('Исходник изменился — повторите операцию');
    }
    parse(source.slice(0, from) + insert + source.slice(to));
    if (appState.active !== file) onOpen(file);
    codeEditor.edit({ from, to, insert });
    if (reselect !== undefined) {
      queueMicrotask(() => {
        clearTimeout(appState.compileTimer);
        onCompile();
        onReselect?.(reselect);
        after?.();
      });
    }
  }

  function bindEvents(codeEvents) {
    $('format-code').onclick = formatActiveFile;
    $('code').oninput = () => {
      appState.files[appState.active] = $('code').value;
      lines();
      onSaveStatus('changed', 'Изменено');
      clearTimeout(appState.saveTimer);
      appState.saveTimer = setTimeout(onPersist, 350);
      clearTimeout(appState.compileTimer);
      appState.compileTimer = setTimeout(
        () => onCompile(appState.active.endsWith('.design.ui')),
        250,
      );
    };
    $('code').onscroll = () => {
      $('lines').scrollTop = $('code').scrollTop;
    };
    $('code').onclick = lines;
    $('code').onkeyup = lines;
    $('code').onkeydown = e => {
      if (e.key === 'Tab') {
        e.preventDefault();
        const el = e.target;
        el.setRangeText('    ', el.selectionStart, el.selectionEnd, 'end');
        el.dispatchEvent(new Event('input'));
      }
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        e.preventDefault();
        onPersist();
      }
      if (e.shiftKey && e.altKey && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        formatActiveFile();
      }
    };
    codeEvents.listen($('code'), 'scroll', highlightSource);
    codeEvents.listen($('code'), 'input', onSourceInputResetSelection);
  }

  return {
    codeEditor,
    highlightSource,
    lines,
    syncOpenDocument,
    formatActiveFile,
    commitSource,
    bindEvents,
  };
}
