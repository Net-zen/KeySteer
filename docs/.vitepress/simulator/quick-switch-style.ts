import type { ConfigDocument } from '../config-studio/document.ts'

/** Shared color resolution for the picker and the live quick-switch preview. */
export function quickSwitchColors(settings: ConfigDocument, appearance: string) {
  const ui = settings.ui ?? {}
  const color = (value: unknown, fallback: string): string => typeof value === 'string' ? value
    : (value as Record<string, string> | undefined)?.[appearance] ?? fallback
  return {
    background_color: color(ui.background_color, appearance === 'dark' ? '#182444FF' : '#EEF2FFFF'),
    text_color: color(ui.text_color, appearance === 'dark' ? '#E8EEFFFF' : '#1A3479FF'),
    border_color: color(ui.border_color, '#8B9CDAFF'),
  }
}
