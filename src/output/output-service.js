// @ts-check
import { appendLog, diagnosticTargets, outputHtml, problemCount } from './output-panel.js';
import { studioEventTypes } from '../shared/studio-events.js';

/**
 * Manages diagnostics, event log, debugger state tabs, and source jumps.
 * @param {{ $: (id: string) => any, root?: Document | Element, appState: any, studioEvents: any, onOpenLocation: (target: any) => void, documentObj?: Document }} options
 */
export function createOutputService({
  $,
  root = document,
  appState,
  studioEvents,
  onOpenLocation,
  documentObj = document,
}) {
  function output() {
    const problemTotal = problemCount(appState.error, appState.lintWarnings);
    $('problem-count').textContent = String(problemTotal);
    studioEvents.emit(studioEventTypes.problems, { count: problemTotal });
    $('event-count').textContent = String(appState.logs.length);
    $('output').innerHTML = outputHtml({
      tab: appState.tab,
      state: appState.state,
      logs: appState.logs,
      error: appState.error,
      warnings: appState.lintWarnings,
    });
    for (const target of diagnosticTargets({
      tab: appState.tab,
      error: appState.error,
      diagnostic: appState.sourceDiagnostic,
      files: appState.files,
    })) {
      const button = documentObj.createElement('button');
      button.textContent = target.label;
      button.onclick = () => onOpenLocation(target);
      $('output').append(button);
    }
  }

  function syncTabs() {
    /** @type {NodeListOf<HTMLElement>} */ (root.querySelectorAll('[data-tab]')).forEach(b =>
      b.classList.toggle('chosen', b.dataset.tab === appState.tab),
    );
  }

  function reportError(err) {
    appState.error = typeof err === 'string' ? err : String(err?.message ?? err);
    appState.sourceDiagnostic = err?.diagnostic ?? null;
    output();
  }

  function log(text) {
    appendLog(appState.logs, text, new Date().toLocaleTimeString());
    output();
  }

  function bindTabs() {
    /** @type {NodeListOf<HTMLElement>} */ (root.querySelectorAll('[data-tab]')).forEach(b => {
      b.onclick = () => {
        appState.tab = b.dataset.tab;
        syncTabs();
        output();
      };
    });
  }

  return { output, syncTabs, reportError, log, bindTabs };
}
