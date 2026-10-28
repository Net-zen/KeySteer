import type { ConfigDocument } from '../config-studio/document.ts'

export type Cut = 'left' | 'down' | 'up' | 'right'
export interface BisectState {
  path: Cut[]
  terminal: boolean
  finished: boolean
  follow: boolean
  returnMode: string
}
export function createBisectState(settings: ConfigDocument = {}, returnMode = 'normal'): BisectState {
  return { path: [], terminal: false, finished: false, follow: settings.cursor_follow_selection !== false, returnMode }
}

/** Percent coordinates, on the same 960 x 600 logical canvas as the preview. */
export function bisectRegion(path: readonly Cut[]) {
  let x = 0, y = 0, width = 100, height = 100
  for (const cut of path) {
    if (cut === 'left' || cut === 'right') {
      width /= 2
      if (cut === 'right') x += width
    } else {
      height /= 2
      if (cut === 'down') y += height
    }
  }
  return { x, y, width, height }
}

export function bisectAxes(state: BisectState, settings: ConfigDocument): [boolean, boolean] {
  if (state.path.length >= Number(settings.max_depth ?? 20)) return [false, false]
  const region = bisectRegion(state.path)
  return [region.width * 9.6 / 2 >= Number(settings.min_size_width ?? 2), region.height * 6 / 2 >= Number(settings.min_size_height ?? 2)]
}

export function applyBisectAction(state: BisectState, action: string, settings: ConfigDocument, pointer: { x: number; y: number }): 'ignored' | 'handled' | 'finish' | 'cancel' {
  const warp = () => { const region = bisectRegion(state.path); pointer.x = region.x + region.width / 2; pointer.y = region.y + region.height / 2 }
  if (action === 'cancel_selection') return 'cancel'
  if (action === 'previous_level') {
    if (!state.path.length) return 'cancel'
    state.path.pop(); state.terminal = false; state.finished = false
    return 'handled'
  }
  if (action === 'first_level' || action === 'restart_mode') {
    Object.assign(state, createBisectState(settings, state.returnMode))
    return 'handled'
  }
  if (action === 'follow') { state.follow = !state.follow; if (state.follow) warp(); return 'handled' }
  if (action === 'confirm_selection' || action === 'finish') {
    if (action === 'confirm_selection') warp()
    if (state.finished) return 'handled'
    state.finished = true
    return 'finish'
  }
  const cut = action.slice('bisect_'.length) as Cut
  if (!action.startsWith('bisect_') || !['left', 'down', 'up', 'right'].includes(cut)) return 'ignored'
  if (state.finished || state.terminal) return 'handled'
  const axes = bisectAxes(state, settings)
  if (!axes[cut === 'left' || cut === 'right' ? 0 : 1]) return 'handled'
  state.path.push(cut)
  state.terminal = !bisectAxes(state, settings).some(Boolean)
  if (state.follow || state.terminal) warp()
  if (state.terminal) { state.finished = true; return 'finish' }
  return 'handled'
}

/** Resolve the same accent and translucent alternate accent as native grids. */
export function bisectColors(document: ConfigDocument, appearance: 'light' | 'dark') {
  const theme = document.theme?.[appearance] ?? {}
  const ui = document.bisect?.ui ?? {}
  const resolve = (value: any, fallback: string): string => typeof value === 'string' ? value : value?.[appearance] ?? fallback
  const accent = theme.accent ?? (appearance === 'dark' ? '#6E82D6FF' : '#465FBCFF')
  const alternate = theme.accent_alt ?? (appearance === 'dark' ? '#8FA2F0FF' : '#0B2377FF')
  const highlight = alternate.slice(0, 7) + Math.round(parseInt(alternate.slice(7, 9), 16) * .3).toString(16).padStart(2, '0').toUpperCase()
  return { line_color: resolve(ui.line_color, accent), highlight_color: resolve(ui.highlight_color, highlight) }
}
