export type SelectionGeometryField = 'padding' | 'pointer_offset'
export type Pair = [number, number]
export interface GeometryViewport { x: number; y: number; width: number; height: number }

export const textRegion = { width: 180, height: 36 }
export const geometryAspect = 420 / 260

export function clampSelectionPair(field: SelectionGeometryField, pair: number[]): Pair {
  const min = field === 'padding' ? 0 : -32768, max = field === 'padding' ? 100 : 32767
  return pair.map(value => Math.max(min, Math.min(max, Math.round(value)))) as Pair
}

/** Both offsets use the original region, never the expanded outline. */
export function selectionPairAtPoint(field: SelectionGeometryField, point: Pair): Pair {
  return clampSelectionPair(field, [point[0] - textRegion.width, point[1] - textRegion.height])
}

/** Keep imported offsets visible without changing their values. Freeze while dragging. */
export function selectionGeometryViewport(padding: Pair, pointer: Pair): GeometryViewport {
  const left = Math.min(-50, -padding[0] - 24, textRegion.width + pointer[0] - 24)
  const top = Math.min(-60, -padding[1] - 24, textRegion.height + pointer[1] - 24)
  const right = Math.max(textRegion.width + 65, textRegion.width + padding[0] + 24, textRegion.width + pointer[0] + 40)
  const bottom = Math.max(textRegion.height + 65, textRegion.height + padding[1] + 24, textRegion.height + pointer[1] + 40)
  const width = Math.max(right - left, (bottom - top) * geometryAspect)
  const height = width / geometryAspect
  return { x: (left + right - width) / 2, y: (top + bottom - height) / 2, width, height }
}

export function selectionPointInViewport(fraction: Pair, viewport: GeometryViewport): Pair {
  return [viewport.x + fraction[0] * viewport.width, viewport.y + fraction[1] * viewport.height]
}

export function selectionSliderBounds(field: SelectionGeometryField, pair: Pair): Pair {
  if (field === 'padding') return [0, 100]
  const range = Math.max(100, Math.ceil(Math.max(...pair.map(Math.abs)) / 50) * 50)
  return [Math.max(-32768, -range), Math.min(32767, range)]
}
