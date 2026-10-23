export function positionFromPoints(start: { x: number; y: number }, end: { x: number; y: number }): string[] {
  const percent = (value: number) => Math.round(Math.max(0, Math.min(1, value)) * 1000) / 10
  const left = Math.min(percent(start.x), percent(end.x)), right = Math.max(percent(start.x), percent(end.x))
  const top = Math.min(percent(start.y), percent(end.y)), bottom = Math.max(percent(start.y), percent(end.y))
  return [top, Math.round((100 - right) * 10) / 10, Math.round((100 - bottom) * 10) / 10, left].map(value => `${value}%`)
}

export function cardPositionRatios(value: unknown = ['50%', '50%', '50%', '50%']): number[] {
  return percentagePositionRatios(value, 'window.card.position')
}

export function percentagePositionRatios(value: unknown, path = 'position'): number[] {
  if (!Array.isArray(value) || value.length !== 4) throw new Error(`${path} requires four percentages`)
  const ratios = value.map(part => {
    if (typeof part !== 'string' || !/^\s*(?:\d+(?:\.\d*)?|\.\d+)%\s*$/.test(part)) throw new Error(`${path} requires percentages`)
    const ratio = Number(part.trim().slice(0, -1)) / 100
    if (!Number.isFinite(ratio) || ratio < 0 || ratio > 1) throw new Error(`${path} must be 0%..100%`)
    return ratio
  })
  if (ratios[0] + ratios[2] > 1 + 1e-12 || ratios[1] + ratios[3] > 1 + 1e-12) throw new Error(`${path} opposite sides exceed 100%`)
  return ratios
}

export interface PositionRect { x: number; y: number; width: number; height: number }

export function relativePoint(area: PositionRect, anchor: { x: number; y: number }) {
  return { x: area.x + area.width * anchor.x, y: area.y + area.height * anchor.y }
}

export function rectCenter(rect: PositionRect) {
  return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }
}

/** Shared card/panel geometry; mirrors api::style's numeric placement helpers. */
export function percentageRegion(area: PositionRect, [top, right, bottom, left]: number[]): PositionRect {
  return { ...relativePoint(area, { x: left, y: top }),
    width: area.width * Math.max(0, 1 - left - right), height: area.height * Math.max(0, 1 - top - bottom) }
}

export function clampPosition(rect: PositionRect, area: PositionRect): PositionRect {
  return { ...rect,
    x: Math.max(area.x, Math.min(Math.max(area.x, area.x + area.width - rect.width), rect.x)),
    y: Math.max(area.y, Math.min(Math.max(area.y, area.y + area.height - rect.height), rect.y)) }
}

export function centeredRect(area: PositionRect, center: { x: number; y: number }, width: number, height: number): PositionRect {
  return clampPosition({ x: center.x - width / 2, y: center.y - height / 2, width, height }, area)
}

// Values are relative to the simulator's current screen, never a previous screen.
export function packedCardCenters(count: number, width: number, height: number, insets: number[]): Array<{ x: number; y: number }> {
  if (!count) return []
  const region = percentageRegion({ x: 0, y: 0, width: 100, height: 100 }, insets)
  const gap = 1
  const availableRows = Math.max(1, Math.floor((100 + gap) / (height + gap)))
  const minimum = Math.ceil(count / availableRows)
  width = Math.min(width, Math.max(1, (100 - gap * (minimum - 1)) / minimum))
  const capacity = Math.max(1, Math.floor((100 + gap) / (width + gap)))
  const regionWidth = region.width
  const regionHeight = region.height
  let columns = minimum
  if (regionWidth > .001) columns = Math.max(minimum, Math.min(count, Math.max(1, Math.floor((regionWidth + gap) / (width + gap)))))
  else {
    let best = Infinity
    for (let n = minimum; n <= Math.max(minimum, Math.min(capacity, count)); n++) {
      const cost = n * (width + gap) + Math.ceil(count / n) * (height + gap)
      if (cost < best) { best = cost; columns = n }
    }
  }
  const totalWidth = columns * (width + gap) - gap
  const totalHeight = Math.ceil(count / columns) * (height + gap) - gap
  const origin = clampPosition({ x: region.x - (regionWidth > .001 ? 0 : totalWidth / 2),
    y: region.y + regionHeight / 2 - totalHeight / 2, width: totalWidth, height: totalHeight }, { x: 0, y: 0, width: 100, height: 100 })
  return Array.from({ length: count }, (_, i) => ({ x: origin.x + (i % columns) * (width + gap) + width / 2, y: origin.y + Math.floor(i / columns) * (height + gap) + height / 2 }))
}
