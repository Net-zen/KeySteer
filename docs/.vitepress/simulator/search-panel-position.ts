import type { ConfigDocument } from '../config-studio/document.ts'
import { centeredRect, clampPosition, percentagePositionRatios, percentageRegion, rectCenter, positionFromPoints, type PositionRect } from './window-card-position.ts'

export type SearchPanelBlock = 'search_input_ui' | 'search_info_ui'
export type PanelRect = PositionRect

const anchors: Record<string, { x: number; y: number }> = {
  top_left: { x: 0, y: 0 }, top_center: { x: .5, y: 0 }, top_right: { x: 1, y: 0 },
  center: { x: .5, y: .5 },
  bottom_left: { x: 0, y: 1 }, bottom_center: { x: .5, y: 1 }, bottom_right: { x: 1, y: 1 },
}

export function searchPanelSettings(document: ConfigDocument, block: SearchPanelBlock) {
  const info = block === 'search_info_ui'
  return {
    position_mode: info ? 'search_input' : 'screen',
    position: info ? ['44%', '50%', '56%', '50%'] : ['95%', '50%', '5%', '50%'],
    width: info ? 520 : 280, font_size: 14, padding_x: -1, padding_y: info ? 16 : 6,
    ...document.ui_hint?.[block],
  }
}

/** Reading an old named anchor must not rewrite it until the user edits it. */
export function panelPositionPercentages(position: unknown): string[] {
  if (Array.isArray(position)) { percentagePositionRatios(position); return [...position] }
  const point = anchors[String(position)]
  if (!point) throw new Error('Unknown search panel position')
  return positionFromPoints(point, point)
}

function anchorPoint(area: PanelRect, position: unknown) {
  return rectCenter(percentageRegion(area, percentagePositionRatios(panelPositionPercentages(position))))
}

/** Mirrors CompiledSearchPanel::place; dimensions and coordinates use the same scale. */
export function placeSearchPanel(ui: ReturnType<typeof searchPanelSettings>, screen: PanelRect, window: PanelRect | null, input: PanelRect | null, height: number, scale = 1): PanelRect {
  const area = ui.position_mode === 'window' && window ? window : screen
  const width = Math.min(ui.width * scale, area.width)
  const followed = ui.position_mode === 'search_input' ? input : null
  let panel: PanelRect
  if (followed) {
    const anchor = anchorPoint({ x: 0, y: 0, width: 1, height: 1 }, ui.position)
    const inputCenter = rectCenter(followed), vertical = anchor.y - .5
    const center = { x: inputCenter.x + (anchor.x - .5) * followed.width,
      y: inputCenter.y + (vertical <= 0 ? -1 : 1) * (.5 + Math.abs(vertical)) * (followed.height + height) }
    panel = { x: center.x - width / 2, y: center.y - height / 2, width, height }
  } else if (typeof ui.position === 'string') {
    const anchor = anchorPoint(area, ui.position), point = anchors[ui.position]
    panel = { x: anchor.x - width * point.x, y: anchor.y - height * point.y, width, height }
  } else {
    panel = centeredRect(area, anchorPoint(area, ui.position), width, height)
  }
  if (input) panel = clampPosition(panel, screen)
  if (!followed && Array.isArray(ui.position)) panel = { ...panel, width: Math.min(panel.width, area.width), height: Math.min(panel.height, area.height) }
  return panel
}

export function searchPanelLayout(document: ConfigDocument, screen: PanelRect, window: PanelRect | null, scale = 1) {
  const inputUi = searchPanelSettings(document, 'search_input_ui')
  const infoUi = searchPanelSettings(document, 'search_info_ui')
  const paddingY = (ui: typeof inputUi) => ui.padding_y >= 0 ? ui.padding_y : Math.round(ui.font_size * .2)
  const inputHeight = (inputUi.font_size * 1.8 + paddingY(inputUi) * 2) * scale
  const infoHeight = (infoUi.font_size * (1.8 * 4 + .5 * 2 + .8) + paddingY(infoUi) * 2) * scale
  const input = placeSearchPanel(inputUi, screen, window, null, inputHeight, scale)
  return { input, info: placeSearchPanel(infoUi, screen, window, input, infoHeight, scale) }
}
