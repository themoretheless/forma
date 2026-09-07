import { sizes, themes, materializeTheme } from './tokens.js';
import { extraSections, extraSpecimens } from './catalog-extra.js';

const originalSections = [
  { id: 'buttons', number: '01', title: 'Кнопки', detail: 'Одна структура. Четыре назначения.', height: 184 },
  { id: 'icons', number: '02', title: 'Иконки и подписи', detail: 'Icon + Label внутри ContentPresenter.', height: 184 },
  { id: 'states', number: '03', title: 'Состояния', detail: 'Наведите, нажмите или перейдите клавишей Tab.', height: 184 },
  { id: 'badges', number: '04', title: 'Метки и аватары', detail: 'Передают информацию, не забирают фокус.', height: 184 },
  { id: 'progress', number: '05', title: 'Прогресс и разделитель', detail: 'Пассивная композиция из простых фигур.', height: 184 },
  { id: 'messages', number: '06', title: 'Сообщения', detail: 'Отдельные иконка, заголовок и описание.', height: 184 },
];

export const sections = [extraSections[0], ...originalSections, ...extraSections.slice(1)].map((section,i)=>({...section,number:String(i+1).padStart(2,'0')}));

export function specimens(section, size = 'regular', state = {}) {
  if (!sizes[size]) throw new Error(`Unknown control size: ${size}`);
  const geometry = sizes[size], button = (type, key, x, y, text, props = {}) => ({
    type, key, x, y, text, ...geometry, width: 146, action: key, ...props,
  });
  const passive = (type, key, x, y, props = {}) => ({ type, key, x, y, ...props });
  switch (section) {
    case 'buttons': return [
      button('PrimaryButton', 'save', 0, 12, 'Сохранить'),
      button('SecondaryButton', 'cancel', 158, 12, 'Отмена'),
      button('GhostButton', 'details', 0, 76, 'Подробнее'),
      button('DangerButton', 'delete', 158, 76, 'Удалить'),
    ];
    case 'icons': return [
      button('IconButton', 'searchIcon', 0, 8, 'Поиск', { width: geometry.height, icon: 'assets/search.svg' }),
      button('IconButton', 'addIcon', 60, 8, 'Добавить', { width: geometry.height, icon: 'assets/plus.svg' }),
      button('IconButton', 'closeIcon', 120, 8, 'Закрыть', { width: geometry.height, icon: 'assets/close.svg' }),
      button('LeadingIconButton', 'search', 0, 68, 'Найти документы', { width: 304, icon: 'assets/search.svg' }),
      button('PrimaryIconButton', 'create', 0, 128, 'Новый проект', { width: 304, icon: 'assets/plus.svg' }),
    ];
    case 'states': return [
      button('PrimaryButton', 'enabled', 0, 12, 'Доступно'),
      button('PrimaryButton', 'disabled', 158, 12, 'Недоступно', { disabled: true }),
      button('SecondaryButton', 'focus', 0, 76, 'Фокус по Tab'),
      button('GhostButton', 'disabledGhost', 158, 76, 'Недоступно', { disabled: true }),
    ];
    case 'badges': return [
      passive('Badge', 'neutralBadge', 0, 12, { text: 'Черновик', width: 96, tone: 'neutral' }),
      passive('Badge', 'successBadge', 104, 12, { text: 'Готово', width: 96, tone: 'success' }),
      passive('Badge', 'warningBadge', 208, 12, { text: 'Ожидание', width: 96, tone: 'warning' }),
      passive('Badge', 'dangerBadge', 0, 56, { text: 'Ошибка', width: 96, tone: 'danger' }),
      passive('Badge', 'accentBadge', 104, 56, { text: 'Новое', width: 96, tone: 'accent' }),
      passive('Avatar', 'anna', 0, 118, { text: 'АН' }),
      passive('Avatar', 'max', 54, 118, { text: 'МК' }),
      passive('Avatar', 'team', 108, 118, { text: '+3' }),
    ];
    case 'progress': return [
      passive('ProgressBar', 'progress25', 0, 24, { width: 304, value: { expr: '25%' } }),
      passive('ProgressBar', 'progress60', 0, 66, { width: 304, value: { expr: '60%' } }),
      passive('ProgressBar', 'progress100', 0, 108, { width: 304, value: { expr: '100%' } }),
      passive('Separator', 'separator', 0, 160, { width: 304 }),
    ];
    case 'messages': return [
      passive('Alert', 'savedNotice', 0, 0, { width: 304, height: 80, text: 'Изменения сохранены', description: 'Можно продолжать работу', tone: 'success' }),
      passive('Alert', 'warningNotice', 0, 100, { width: 304, height: 80, text: 'Проверьте подключение', description: 'Изменения остались локально', tone: 'warning' }),
    ];
    default: return extraSpecimens(section,size,state);
  }
}

function value(v) {
  if (v?.expr) return v.expr;
  if (typeof v === 'string') return `'${v.replace(/\\/g, '\\\\').replace(/'/g, "\\'").replace(/\n/g, '\\n')}'`;
  return String(v);
}
function instance(item, offsetX = 0, offsetY = 0) {
  const { type, action, x = 0, y = 0, ...props } = item;
  return `${type} { x: ${x + offsetX}; y: ${y + offsetY}; ${Object.entries(props).map(([k, v]) => `${k}: ${value(v)};`).join(' ')} ${action ? `clicked -> actions.${action}();` : ''} }`;
}
export function sectionSource(id, theme = 'light', size = 'regular', state = {}) {
  if (!themes[theme]) throw new Error(`Unknown Forma theme: ${theme}`);
  const section = sections.find(s => s.id === id);
  if (!section) throw new Error(`Unknown section: ${id}`);
  const source = `component Samples { Frame { width: 320; height: ${section.height}; padding: 0; gap: 0; background: ${themes[theme].surface}; clip: true; ${specimens(id, size, state).map(item => instance(item, 8)).join('\n')} } }`;
  return materializeTheme({'sample.ui':source},theme)['sample.ui'];
}
export function catalogSource(theme = 'light', size = 'regular', state = {}) {
  if (!themes[theme]) throw new Error(`Unknown Forma theme: ${theme}`);
  const nodes = sections.flatMap((section, index) => {
    const x = 24 + index % 2 * 352, y = 24 + Math.floor(index / 2) * 304;
    return [instance({ type: 'Surface', key: `heading${index}`, text: `${section.number} · ${section.title}`, width: 320, height: 28, fontSize: 17, borderWidth: 0, background: { expr: '#00000000' } }, x, y),
      ...specimens(section.id, size, state).map(item => instance(item, x + 8, y + 42))];
  });
  const source = `component FormaControls { Frame { width: 720; height: 780; padding: 0; gap: 0; background: ${themes[theme].canvas}; clip: true; Scroll { ${nodes.join('\n')} } } }`;
  return materializeTheme({'catalog.ui':source},theme)['catalog.ui'];
}
export function catalogProject(libraryFiles, theme = 'light', size = 'regular', options = {}) {
  return {
    'ui/FormaControls.ui': catalogSource(theme, size, options.state),
    ...materializeTheme(libraryFiles, theme, options),
    'README.md': '# Forma controls\n\nВыберите «Вектор · Rust/WASM» и ui/FormaControls.ui → «Показать UI». Все части находятся в components/. Клики выводятся в журнал; обработчики приложения ещё не подключены.\n',
  };
}
