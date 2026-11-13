// What the inspector shows for a running native window: the live tree the Rust runtime reports,
// matched to the source nodes the window was started from.

/**
 * @param {{ nodes?: Array<{ control: number | null, parent: number | null, kind: string }>,
 *   controls?: Array<{ index: number, bounds: number[], disabled?: boolean }>,
 *   selected?: number | null }} data report of the native window
 * @param {Array<{ type: string, props: Record<string, unknown> }>} controls source nodes in the
 *   order the runtime numbers its controls
 * @param {boolean} current the source the window was started from is still the source on screen
 */
export function nativeInspectionModel(data, controls, current) {
  const rows = (data.nodes ?? []).map(node => {
    const control = data.controls?.find(c => c.index === node.control),
      source = controls[node.control];
    let label = source ? source.type + ' ' + (source.props.key ?? '') : node.kind;
    if (control)
      label +=
        ' · ' +
        control.bounds.map(v => Math.round(v)).join(', ') +
        (control.disabled ? ' · disabled' : '');
    return {
      label,
      indent: node.parent === null ? 0 : node.control === null ? 12 : 24,
      pressed: node.control === data.selected,
      disabled: !current || !source,
      source,
    };
  });
  const selected = controls[data.selected];
  return {
    status: current
      ? 'Данные из работающего Rust runtime. F9 и щелчок в окне выбирают элемент.'
      : 'Исходник изменился или окно запущено раньше этой сессии: переход к коду отключён.',
    rows,
    selected: current && selected ? selected : null,
  };
}
