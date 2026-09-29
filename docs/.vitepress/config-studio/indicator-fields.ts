import type { StyleField } from './fields.ts'

export const indicatorModes = ['normal', 'text_input', 'grid', 'recursive_grid', 'ui_hint', 'window', 'window_quick', 'window_editor', 'window_restore', 'window_tab']
export function indicatorFields(mode?: string): StyleField[] {
  const root = mode ? `mode_indicator.modes.${mode}` : 'mode_indicator'
  return [
    ...(mode ? [
      { path: `${root}.enabled`, label: '显示模式标识符', kind: 'boolean' as const },
      { path: `${root}.text`, label: '标识符文字', kind: 'text' as const },
    ] : []),
    { path: `${root}.ui.indicator_offset`, label: '标识符偏移 [X, Y]', kind: 'offset' },
    { path: `${root}.ui.font_size`, label: '字号', kind: 'number', min: 1, step: 1 },
    { path: `${root}.ui.font_family`, label: '字体', kind: 'text' },
    ...['background_color', 'text_color', 'border_color'].map((key, index) => ({ path: `${root}.ui.${key}`, label: ['背景色', '文字色', '边框色'][index], kind: 'color' as const })),
    ...['border_radius', 'padding_x', 'padding_y', 'border_width'].map((key, index) => ({ path: `${root}.ui.${key}`, label: ['圆角', '水平内边距', '垂直内边距', '边框宽度'][index], kind: 'number' as const, min: key === 'border_width' ? 0 : -1, step: 1 })),
  ]
}

/** Cursor settings are global; importing per-mode overrides still preserves them. */
export const cursorFields: StyleField[] = [
  { path: 'mode_indicator.cursor.enabled', label: '显示光标圆环', kind: 'boolean' },
  { path: 'mode_indicator.cursor.radius', label: '圆环半径', kind: 'number', min: 1, max: 256, step: 1 },
  { path: 'mode_indicator.cursor.stroke_width', label: '圆环线宽', kind: 'number', min: 0, max: 256, step: 1 },
  { path: 'mode_indicator.cursor.left_pressed_color', label: '左键按下颜色', kind: 'color' },
  { path: 'mode_indicator.cursor.middle_pressed_color', label: '中键按下颜色', kind: 'color' },
  { path: 'mode_indicator.cursor.right_pressed_color', label: '右键按下颜色', kind: 'color' },
]
