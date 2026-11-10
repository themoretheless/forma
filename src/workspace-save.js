// The status shown after a workspace write. The project is already in browser storage by then,
// so a failed write still keeps the edit; the text must not claim it reached the project files.
export function workspaceSaveStatus(outcome, projectName) {
  if (outcome?.ok) return { status: 'saved', text: `Сохранено (${projectName})` };
  const reason = typeof outcome?.status === 'number' ? `: HTTP ${outcome.status}` : '';
  return { status: 'saved', text: `Сохранено в браузере (запись в проект не удалась${reason})` };
}
