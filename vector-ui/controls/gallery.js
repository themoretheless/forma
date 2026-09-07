import './gallery.css';
import { loadVectorRuntime, createVectorPreview } from '../../src/vector-preview.js';
import { compileComponents } from '../../src/components.js';
import { parse } from '../../src/language.js';
import { themes, materializeTheme } from './tokens.js';
import { sections, sectionSource, catalogProject } from './catalog.js';
import { createCatalogSession } from './catalog-session.js';

const rawFiles = import.meta.glob(['./components/*.ui', './assets/*.svg'], { query: '?raw', import: 'default', eager: true });
const libraryFiles = Object.fromEntries(Object.entries(rawFiles).map(([path, source]) => [path.slice(2), source]));
const $ = id => document.getElementById(id);
const preference = matchMedia('(prefers-reduced-motion: reduce)');
let theme = 'light', size = 'regular', totalEvents = 0;
const previews = new Map();
let session = null;
$('motion').checked = preference.matches;
$('component-count').textContent = Object.keys(libraryFiles).filter(p=>p.startsWith('components/')).length;
$('icon-count').textContent = Object.keys(libraryFiles).filter(p=>p.startsWith('assets/')).length;

for (const section of sections) {
  const link = document.createElement('a');
  link.href = `#${section.id}`;
  link.innerHTML = `<span>${section.number}</span>${section.title}`;
  $('section-nav').append(link);
  const card = document.createElement('section');
  card.className = 'specimen'; card.id = section.id;
  card.innerHTML = `<div class="specimen-heading"><span class="mono">${section.number}</span><span class="specimen-kind">${section.kind ?? (['badges','progress','messages'].includes(section.id) ? 'ИНФОРМАЦИЯ' : 'ВЗАИМОДЕЙСТВИЕ')}</span></div><h2>${section.title}</h2><p>${section.detail}</p><div class="stage-wrap"><div class="stage" id="stage-${section.id}"></div></div><details><summary>Из каких частей</summary><pre></pre></details>`;
  const parts = {
    buttons: 'Surface → Button\n  ├ PrimaryButton\n  ├ SecondaryButton\n  ├ GhostButton\n  └ DangerButton',
    icons: 'IconButton → Icon\nLeadingIconButton → IconLabel\n  ├ Icon → SVG\n  └ Label → Text',
    states: 'Button + PointerArea\nhover / pressed / focus / disabled\nTab · Shift+Tab · Space · Enter',
    badges: 'Surface → Badge\nSurface → Avatar\nБез PointerArea: без Tab и click',
    progress: 'Surface → ProgressBar\n  └ Frame → Rectangle\nSurface → Separator',
    messages: 'Surface → Alert → MessageContent\n  ├ Icon\n  ├ Label · заголовок\n  └ Label · описание',
  };
  card.querySelector('pre').textContent = section.parts ?? parts[section.id];
  $('catalog').append(card);
}

function renderAll(onlySection) {
  const palette = themes[theme];
  document.body.dataset.theme = theme;
  for (const [key, value] of Object.entries(palette)) document.documentElement.style.setProperty(`--${key}`, value);
  $('light').setAttribute('aria-pressed', String(theme === 'light'));
  $('dark').setAttribute('aria-pressed', String(theme === 'dark'));
  const files = materializeTheme(libraryFiles, theme, { reducedMotion: $('motion').checked });
  for (const section of sections) {
    const preview = previews.get(section.id);
    if (!preview || (onlySection && section.id !== onlySection)) continue;
    const source = sectionSource(section.id, theme, size, session?.state()), path = `ui/${section.id}.ui`;
    const compiled = compileComponents({ ...files, [path]: source }, path, {}, { measureText: preview.measureText });
    if (!preview.render({ container: $(`stage-${section.id}`), ...compiled, nodes: parse(source).nodes, designMode: false, reducedMotion: $('motion').checked })) {
      throw Error($('backend').textContent || `Не удалось отрисовать ${section.title}`);
    }
  }
  delete document.body.dataset.failed;
  $('backend').textContent = 'Rust / WASM · готово';
}

function renderSafely() {
  try { renderAll(); }
  catch (error) { $('backend').textContent = error.message; document.body.dataset.failed = 'true'; }
}

try {
  const runtime = await loadVectorRuntime();
  session = createCatalogSession(runtime, (section, focus) => {
    try { renderAll(section); if(focus) previews.get(section)?.focusKey(focus); }
    catch(error) { $('backend').textContent=error.message; document.body.dataset.failed='true'; }
  });
  for (const section of sections) previews.set(section.id, createVectorPreview({
    runtime,
    onControlPointer: (phase, input) => session.pointer(phase, input),
    onControlKey: (event, node) => session.key(event, node),
    onAction(action, node) {
      totalEvents++;
      $('event').textContent = `${node.props.key} → ${action}()`;
      $('event-count').textContent = `${totalEvents} ${new Intl.PluralRules('ru').select(totalEvents)==='one'?'событие':new Intl.PluralRules('ru').select(totalEvents)==='few'?'события':'событий'}`;
      session.dispatch(action);
    },
    onError(error) { $('backend').textContent = error.message; document.body.dataset.failed = 'true'; },
  }));
  renderAll();
  $('backend').textContent = 'Rust / WASM · готово';
} catch (error) {
  $('backend').textContent = error.message;
  document.body.dataset.failed = 'true';
}

for (const name of ['light', 'dark']) $(name).addEventListener('click', () => { theme = name; renderSafely(); });
$('size').addEventListener('change', event => { size = event.target.value; renderSafely(); });
$('motion').addEventListener('change', renderSafely);
preference.addEventListener('change', event => { $('motion').checked = event.matches; renderSafely(); });
$('download').addEventListener('click', () => {
  const fields=Object.fromEntries([...previews.values()].flatMap(preview=>preview.snapshot().controls.filter(c=>c.editable).map(c=>[c.key,c.value])));
  const project = catalogProject(libraryFiles, theme, size, { reducedMotion: $('motion').checked, state: {...session?.state(),fields} });
  const url = URL.createObjectURL(new Blob([JSON.stringify(project, null, 2)], { type: 'application/json' }));
  const anchor = document.createElement('a');
  anchor.href = url; anchor.download = `forma-controls-${theme}.json`; anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
});
window.addEventListener('pagehide', event => {
  if (!event.persisted) { for (const preview of previews.values()) preview.destroy(); session?.destroy(); }
});
window.addEventListener('pageshow', event => { if (event.persisted) renderSafely(); });
