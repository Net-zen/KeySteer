export const searchEditActions = new Set([
  'accept', 'cancel', 'paste', 'copy', 'cut', 'select_all',
  'left', 'right', 'home', 'end', 'select_left', 'select_right',
  'select_home', 'select_end', 'backspace', 'delete',
])

/** Read only the current key = action schema; imports are normalized beforehand. */
export function readSearchEditBindings(value: unknown): Record<string, string> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('ui_hint.search_edit_keys must be a key = action table')
  if (!Object.values(value).every(action => typeof action === 'string' && searchEditActions.has(action))) throw new Error('ui_hint.search_edit_keys requires known editing actions')
  return { ...value } as Record<string, string>
}
