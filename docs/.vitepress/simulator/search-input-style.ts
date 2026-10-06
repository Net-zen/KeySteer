import type { ConfigDocument } from '../config-studio/document.ts'

/** Native search panel colors, shared by controls and the state preview. */
export function searchPanelColors(document: ConfigDocument, appearance: string, block = 'search_input_ui') {
  const color = (value: unknown, fallback: string): string => typeof value === 'string' ? value
    : (value as Record<string, string> | undefined)?.[appearance] ?? fallback
  const dark = appearance === 'dark'
  const palette = document.theme?.[appearance] ?? {}
  const ui = document.ui_hint?.[block] ?? {}
  const point = document.ui_hint?.search_point ?? {}
  const surface = color(palette.surface, dark ? '#0A1338FF' : '#EEF2FFFF')
  const surfaceAlpha = /^#[\da-f]{8}$/i.test(surface) ? parseInt(surface.slice(7), 16) : 255
  const background = color(ui.background_color, surface.slice(0, 7) + Math.round(surfaceAlpha * .95).toString(16).padStart(2, '0'))
  const border = color(ui.border_color, color(palette.accent, dark ? '#6E82D6FF' : '#465FBCFF'))
  return {
    background_color: background,
    text_color: color(ui.text_color, color(palette.text, dark ? '#E8EEFFFF' : '#17327AFF')),
    border_color: border,
    point_background_color: color(point.input_background_color, dark ? '#284D44FF' : '#E8F6F0FF'),
    point_border_color: color(point.input_border_color, border),
  }
}
