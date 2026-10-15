import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import { stringify } from 'smol-toml'
import { parseConfigDocument, resolveConfigDocument } from '../config-studio/document.ts'
import { panelPositionPercentages, searchPanelLayout, searchPanelSettings } from './search-panel-position.ts'
import { positionFromPoints } from './window-card-position.ts'
import layoutCases from '../../../tests/fixtures/search-panel-layout.json' with { type: 'json' }

const screen = { x: 0, y: 0, width: 1000, height: 800 }
const window = { x: 100, y: 100, width: 600, height: 500 }
const near = (actual: number, expected: number) => assert.ok(Math.abs(actual - expected) < 1e-9, `${actual} != ${expected}`)

test('native and browser share search panel layout cases', () => {
  const rect = ([x, y, width, height]: number[]) => ({ x, y, width, height })
  for (const entry of layoutCases) {
    const document = parseConfigDocument(entry.config).document
    const layout = searchPanelLayout(document, rect(entry.screen), entry.window ? rect(entry.window) : null, entry.scale)
    for (const block of ['input', 'info'] as const) {
      const expected = rect(entry[block])
      for (const field of ['x', 'y', 'width', 'height'] as const) {
        assert.ok(Math.abs(layout[block][field] - expected[field]) < 1e-9, `${entry.name}: ${block}.${field} = ${layout[block][field]}, expected ${expected[field]}`)
      }
    }
  }
})

test('shipped search panels and sparse previews share percentage-only defaults', async () => {
  const defaults = parseConfigDocument(await readFile(new URL('../../../keysteer.default.toml', import.meta.url), 'utf8')).document
  const effective = resolveConfigDocument(defaults, {})
  assert.deepEqual(searchPanelLayout(effective, screen, window), searchPanelLayout({}, screen, window))
  const layout = searchPanelLayout(effective, screen, window)
  near(layout.input.x + layout.input.width / 2, 500)
  near(layout.input.y + layout.input.height / 2, 760)
  near(layout.input.y - layout.info.y - layout.info.height, 11.712)
  for (const block of ['search_input_ui', 'search_info_ui'] as const) {
    assert.equal('x_offset' in effective.ui_hint[block], false)
    assert.equal('y_offset' in effective.ui_hint[block], false)
    assert.deepEqual(effective.ui_hint[block].position, searchPanelSettings({}, block).position)
  }
})

test('screen and window references use their own regions and missing windows fall back to screen', () => {
  const document = { ui_hint: { search_input_ui: { position_mode: 'window', position: ['50%', '50%', '50%', '50%'] } } }
  const original = JSON.stringify(document)
  const inWindow = searchPanelLayout(document, screen, window).input
  near(inWindow.x + inWindow.width / 2, 400)
  near(inWindow.y + inWindow.height / 2, 350)
  const fallback = searchPanelLayout(document, screen, null).input
  near(fallback.x + fallback.width / 2, 500)
  near(fallback.y + fallback.height / 2, 400)
  assert.equal(JSON.stringify(document), original)
})

test('information follows its input above or below and screen positioning remains independent', () => {
  const document = { ui_hint: { search_input_ui: { position: ['50%', '50%', '50%', '50%'] }, search_info_ui: {} as Record<string, unknown> } }
  const above = searchPanelLayout(document, screen, window)
  near(above.input.y - above.info.y - above.info.height, 11.712)
  document.ui_hint.search_info_ui.position = ['100%', '50%', '0%', '50%']
  const below = searchPanelLayout(document, screen, window)
  near(below.info.y - below.input.y - below.input.height, 97.6)
  document.ui_hint.search_info_ui = { position_mode: 'screen', position: ['0%', '50%', '100%', '50%'] }
  const independent = searchPanelLayout(document, screen, window)
  near(independent.info.y, 0)
})

test('input-center placement controls direction and gap using only percentages in 0–100', () => {
  for (const top of [0, 25, 44, 49, 50, 51, 56, 75, 100]) {
    const document = { ui_hint: { search_input_ui: { position: ['50%', '50%', '50%', '50%'] },
      search_info_ui: { position: [`${top}%`, '50%', `${100 - top}%`, '50%'] } } }
    const { input, info } = searchPanelLayout(document, screen, window)
    const gap = top <= 50 ? input.y - info.y - info.height : info.y - input.y - input.height
    near(gap, Math.abs(top / 100 - .5) * (input.height + info.height))
    near(info.x + info.width / 2, input.x + input.width / 2)
  }
  for (const top of ['-1%', '101%']) assert.throws(() => parseConfigDocument(`[ui_hint.search_info_ui]\nposition = ['${top}', '50%', '0%', '50%']`), /position/)
})

test('dragged percentages export and reimport without offsets or changes to the other panel', () => {
  const document = { ui_hint: { search_input_ui: { position: positionFromPoints({ x: .2, y: .3 }, { x: .8, y: .5 }) } } }
  const reimported = parseConfigDocument(stringify(document)).document
  assert.deepEqual(reimported.ui_hint.search_input_ui.position, ['30%', '20%', '50%', '20%'])
  assert.equal(reimported.ui_hint.search_info_ui, undefined)
  const layout = searchPanelLayout(reimported, screen, window)
  near(layout.input.x + layout.input.width / 2, 500)
  near(layout.input.y + layout.input.height / 2, 320)
  const clamped = searchPanelLayout({ ui_hint: { search_info_ui: { width: 2000 } } }, screen, window).info
  assert.equal(clamped.x, 0)
  assert.equal(clamped.width, screen.width)
})

test('both panels accept and discard legacy offsets without changing layout or export', () => {
  for (const block of ['search_input_ui', 'search_info_ui']) {
    for (const field of ['x_offset', 'y_offset']) {
      for (const value of ['0', '-9000', '9000', '"ignored"']) {
        const parsed = parseConfigDocument(`[ui_hint.${block}]\n${field} = ${value}`).document
        assert.equal(field in parsed.ui_hint[block], false)
        assert.deepEqual(searchPanelLayout(parsed, screen, window), searchPanelLayout({}, screen, window))
        assert.doesNotMatch(stringify(parsed), /[xy]_offset/)
      }
    }
  }
})

test('both panels reject invalid percentages and invalid reference modes', () => {
  for (const block of ['search_input_ui', 'search_info_ui']) {
    assert.throws(() => parseConfigDocument(`[ui_hint.${block}]\nposition = ['60%', '0%', '50%', '0%']`), /position/)
    assert.throws(() => parseConfigDocument(`[ui_hint.${block}]\nposition_mode = 'mouse'`), /position_mode/)
  }
  assert.throws(() => parseConfigDocument('[ui_hint.search_input_ui]\nposition_mode = "search_input"'), /position_mode/)
  assert.deepEqual(panelPositionPercentages('bottom_center'), ['100%', '50%', '0%', '50%'])
})
