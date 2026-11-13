// What the panel under the editor shows: diagnostics, the event journal or the preview state.
// Pure: the editor passes what it knows and gets counts, markup and link targets back.
import { escapeHtml } from './shared/text.js';

const JOURNAL_LIMIT = 200;

/** Adds an entry to the journal, dropping the oldest once it holds `JOURNAL_LIMIT`. */
export function appendLog(logs, text, time) {
  logs.push({ time, text });
  if (logs.length > JOURNAL_LIMIT) logs.shift();
}

/** An error hides the warnings: they describe markup that did not compile. */
export function problemCount(error, warnings) {
  return error ? 1 : warnings.length;
}

/** The markup of the panel body for the chosen tab. */
export function outputHtml({ tab, state, logs, error, warnings }) {
  if (tab === 'state') return `<pre>${escapeHtml(JSON.stringify(state, null, 2))}</pre>`;
  if (tab === 'events')
    return (
      logs
        .map(entry => `<div class="log"><time>${entry.time}</time>${escapeHtml(entry.text)}</div>`)
        .join('') || '<p class="muted">События появятся при взаимодействии с предпросмотром.</p>'
    );
  if (error) return `<p class="error">● ${escapeHtml(error)}</p>`;
  if (warnings.length)
    return warnings.map(w => `<p class="warn">⚠ ${escapeHtml(w.message)}</p>`).join('');
  return '<p class="ok">✓ Разметка проверена. Ошибок нет.</p>';
}

/**
 * The places in source the shown error can jump to. There are none unless the problems tab is
 * open and `diagnostic` describes this very error; a place in a file the project does not hold
 * is skipped.
 */
export function diagnosticTargets({ tab, error, diagnostic, files }) {
  if (tab !== 'problems' || !diagnostic || diagnostic.message !== error) return [];
  const targets = [];
  for (const [label, source] of [
    ['К ошибке', diagnostic.source],
    ['К определению узла', diagnostic.related],
  ]) {
    if (!source || !Object.hasOwn(files, source.file) || typeof files[source.file] !== 'string')
      continue;
    const line = files[source.file].slice(0, source.from).split('\n').length;
    targets.push({
      label: `${label} · ${source.file}:${line}`,
      file: source.file,
      from: source.from,
      to: source.to,
    });
  }
  return targets;
}
