import type { ConfigDocument } from '../config-studio/document.ts'
import { selectedCardStyle } from './window-card-style.ts'

/** Same selection colors as native Window cards and UI Hint labels. */
export function searchHighlightStyle(document: ConfigDocument, appearance: string) {
  const highlight = document.ui_hint?.search_highlight ?? {}
  const selected = selectedCardStyle({
    selected_background_color: highlight.background_color,
    selected_border_color: highlight.border_color,
    selected_border_width: highlight.border_width,
  }, appearance)
  const configured = highlight.text_color
  const fallback = document.theme?.[appearance]?.text ?? (appearance === 'dark' ? '#E8EEFFFF' : '#17327AFF')
  return {
    ...selected,
    text: typeof configured === 'string' ? configured : configured?.[appearance] ?? fallback,
  }
}
