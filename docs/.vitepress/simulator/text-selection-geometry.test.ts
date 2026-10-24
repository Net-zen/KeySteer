import assert from 'node:assert/strict'
import test from 'node:test'
import { parseConfigDocument, resolveConfigDocument, setConfigPath } from '../config-studio/document.ts'
import { clampSelectionPair, selectionGeometryViewport, selectionPairAtPoint, selectionPointInViewport, selectionSliderBounds, textRegion } from '../config-studio/text-selection-geometry.ts'

test('visual edits keep symmetric padding separate from the original pointer anchor', () => {
  const point: [number, number] = [textRegion.width + 18.4, textRegion.height + 7.6]
  assert.deepEqual(selectionPairAtPoint('padding', point), [18, 8])
  assert.deepEqual(selectionPairAtPoint('pointer_offset', point), [18, 8])
  assert.deepEqual(selectionPairAtPoint('padding', [0, 0]), [0, 0])
  assert.deepEqual(selectionPairAtPoint('pointer_offset', [0, 0]), [-180, -36])
  assert.deepEqual(clampSelectionPair('padding', [100.9, -1]), [100, 0])
  assert.deepEqual(clampSelectionPair('pointer_offset', [-32769, 32768]), [-32768, 32767])
})

test('responsive preview maps dragging back to config units and fits extreme imported values', () => {
  for (const pointer of [[12, 12], [-80, -70], [-32768, 32767]] as [number, number][]) {
    const view = selectionGeometryViewport([100, 100], pointer)
    const point: [number, number] = [textRegion.width + pointer[0], textRegion.height + pointer[1]]
    const fraction: [number, number] = [(point[0] - view.x) / view.width, (point[1] - view.y) / view.height]
    assert.ok(fraction.every(value => value > 0 && value < 1))
    assert.deepEqual(selectionPairAtPoint('pointer_offset', selectionPointInViewport(fraction, view)), pointer)
    assert.ok(view.x <= -100 && view.y <= -100)
    const [min, max] = selectionSliderBounds('pointer_offset', pointer)
    assert.ok(pointer.every(value => value >= min && value <= max))
    assert.ok(min >= -32768 && max <= 32767)
  }
  assert.deepEqual(selectionSliderBounds('padding', [2, 2]), [0, 100])
  assert.deepEqual(selectionSliderBounds('pointer_offset', [12, 12]), [-100, 100])
})

test('visual changes export pairs under ui and preserve the other setting', async () => {
  const { stringify } = await import('smol-toml')
  const defaults = parseConfigDocument('[text_select.ui]\npadding = [2, 2]\npointer_offset = [12, 12]').document
  const document = parseConfigDocument('').document
  setConfigPath(document, 'text_select.ui.padding', selectionPairAtPoint('padding', [210, 46]))
  let effective = resolveConfigDocument(defaults, parseConfigDocument(stringify(document)).document)
  assert.deepEqual(effective.text_select.ui.padding, [30, 10])
  assert.deepEqual(effective.text_select.ui.pointer_offset, [12, 12])
  setConfigPath(document, 'text_select.ui.pointer_offset', selectionPairAtPoint('pointer_offset', [165, 62]))
  effective = resolveConfigDocument(defaults, parseConfigDocument(stringify(document)).document)
  assert.deepEqual(effective.text_select.ui.padding, [30, 10])
  assert.deepEqual(effective.text_select.ui.pointer_offset, [-15, 26])
})
