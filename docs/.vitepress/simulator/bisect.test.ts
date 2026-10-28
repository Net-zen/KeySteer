import assert from 'node:assert/strict'
import test from 'node:test'
import { applyBisectAction, bisectColors, bisectAxes, bisectRegion, createBisectState } from './bisect.ts'
import { parseConfigDocument, resolveConfigDocument } from '../config-studio/document.ts'
import { resolveLayeredPhysicalBinding, resolvedChordKeys } from './bindings.ts'
import { readFileSync } from 'node:fs'
import { stringify } from 'smol-toml'

test('Bisect narrows one axis, ignores exhausted axes, and undoes the last accepted cut', () => {
  const settings = { min_size_width: 240, min_size_height: 150, max_depth: 20 }
  const state = createBisectState(settings), pointer = { x: 50, y: 50 }
  for (const action of ['bisect_right', 'bisect_left', 'bisect_left', 'bisect_down']) {
    assert.equal(applyBisectAction(state, action, settings, pointer), 'handled')
  }
  assert.deepEqual(state.path, ['right', 'left', 'down'])
  assert.deepEqual(bisectRegion(state.path), { x: 50, y: 50, width: 25, height: 50 })
  assert.deepEqual(bisectAxes(state, settings), [false, true])
  assert.equal(applyBisectAction(state, 'bisect_up', settings, pointer), 'finish')
  assert.deepEqual(pointer, { x: 62.5, y: 62.5 })
  assert.equal(state.finished, true)
  assert.equal(applyBisectAction(state, 'previous_level', settings, pointer), 'handled')
  assert.equal(state.finished, false)
  assert.equal(state.terminal, false)
  assert.deepEqual(bisectRegion(state.path), { x: 50, y: 50, width: 25, height: 50 })
})

test('Bisect follows configuration, finishes once, resets and cancels at the root', () => {
  const settings = { cursor_follow_selection: false, max_depth: 2 }
  const state = createBisectState(settings, 'grid'), pointer = { x: 12, y: 34 }
  applyBisectAction(state, 'bisect_left', settings, pointer)
  assert.deepEqual(pointer, { x: 12, y: 34 })
  applyBisectAction(state, 'follow', settings, pointer)
  assert.deepEqual(pointer, { x: 25, y: 50 })
  assert.equal(applyBisectAction(state, 'bisect_down', settings, pointer), 'finish')
  assert.equal(applyBisectAction(state, 'confirm_selection', settings, pointer), 'handled')
  applyBisectAction(state, 'first_level', settings, pointer)
  assert.deepEqual(state, createBisectState(settings, 'grid'))
  assert.equal(applyBisectAction(state, 'previous_level', settings, pointer), 'cancel')
})

test('Bisect settings roundtrip with sparse controls and full Primary+F priority', () => {
  const defaults = parseConfigDocument(readFileSync(new URL('../../../keysteer.default.toml', import.meta.url), 'utf8')).document
  const document = parseConfigDocument('[bisect.bindings]\nh = "none"\narrow_left = "bisect_left"\n[bisect.ui]\nline_width = 0').document
  const effective = resolveConfigDocument(defaults, parseConfigDocument(stringify(document)).document)
  assert.equal(effective.bisect.bindings.h, 'none')
  assert.equal(effective.bisect.bindings.arrow_left, 'bisect_left')
  assert.equal(effective.bisect.bindings.j, 'bisect_down')
  assert.equal(effective.bisect.bindings.enter, 'confirm_selection')
  assert.equal(effective.bisect.temporary_mode_keys.length, 0)
  assert.equal(effective.bisect.ui.line_width, 0)
  for (const isMac of [false, true]) {
    const keys = resolvedChordKeys(defaults, 'primary+f', isMac)
    assert.equal(resolveLayeredPhysicalBinding(defaults, 'normal', keys, 'f', isMac)?.value, 'bisect')
    assert.equal(resolveLayeredPhysicalBinding(defaults, 'window', keys, 'f', isMac)?.value, 'recursive_grid')
  }
  for (const source of ['[bisect]\nkeys = "hjkl"', '[bisect]\nmax_depth = 0', '[bisect.ui]\nfont_size = 20', '[bisect.ui]\nline_color = "bad"', '[bisect.lifecycle]\nafter_click = "left_click"', '[bisect.lifecycle]\nafter_finish = "finish"']) {
    assert.throws(() => parseConfigDocument(source))
  }
})

test('Bisect color controls and preview share themed native defaults and overrides', () => {
  assert.deepEqual(bisectColors({}, 'light'), { line_color: '#465FBCFF', highlight_color: '#0B23774D' })
  assert.deepEqual(bisectColors({ theme: { dark: { accent: '#11223344', accent_alt: '#44556680' } } }, 'dark'), { line_color: '#11223344', highlight_color: '#44556626' })
  assert.equal(bisectColors({ bisect: { ui: { highlight_color: { light: '#12345678', dark: '#98765432' } } } }, 'dark').highlight_color, '#98765432')
})
