import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import { stringify } from 'smol-toml'
import { cloneConfigDocument, deleteConfigPath, parseConfigDocument, resolveConfigDocument, setConfigPath } from '../config-studio/document.ts'
import { selectedCardStyle } from './window-card-style.ts'

test('selected-card preview uses built-in defaults without exporting unmodified overrides', async () => {
  const defaults = parseConfigDocument(await readFile(new URL('../../../keysteer.default.toml', import.meta.url), 'utf8')).document
  const exported = parseConfigDocument(stringify(defaults)).document
  for (const field of ['selected_background_color', 'selected_border_color', 'selected_border_width']) {
    assert.equal(Object.hasOwn(exported.window.card, field), false)
  }
  for (const appearance of ['light', 'dark']) {
    const expected = {
      background: appearance === 'dark' ? '#284D44FF' : '#E8F6F0FF',
      border: appearance === 'dark' ? '#85CDB8FF' : '#60B49CFF', borderWidth: 1.5,
    }
    assert.deepEqual(selectedCardStyle({}, appearance), expected)
    assert.deepEqual(selectedCardStyle(exported.window.card, appearance), expected)
  }
})

test('editing one selected-card field exports only that override and reset removes it', async () => {
  const defaults = parseConfigDocument(await readFile(new URL('../../../keysteer.default.toml', import.meta.url), 'utf8')).document
  const edited = cloneConfigDocument(defaults)
  setConfigPath(edited, 'window.card.selected_background_color', { light: '#EAF4FFFF', dark: '#284D44FF' })
  const exported = parseConfigDocument(stringify(edited)).document
  assert.deepEqual({ ...exported.window.card.selected_background_color }, { light: '#EAF4FFFF', dark: '#284D44FF' })
  assert.equal(Object.hasOwn(exported.window.card, 'selected_border_color'), false)
  assert.equal(Object.hasOwn(exported.window.card, 'selected_border_width'), false)
  deleteConfigPath(edited, 'window.card.selected_background_color')
  assert.equal(Object.hasOwn(parseConfigDocument(stringify(edited)).document.window.card, 'selected_background_color'), false)
  assert.deepEqual(selectedCardStyle(edited.window.card, 'light'), selectedCardStyle({}, 'light'))
})

test('selected-card edits retain theme colors, transparency and a hidden border through export', async () => {
  const defaults = parseConfigDocument(await readFile(new URL('../../../keysteer.default.toml', import.meta.url), 'utf8')).document
  const imported = parseConfigDocument(`[window.card]
selected_background_color = { light = "#E0F2E9FF", dark = "#285245E0" }
selected_border_color = "#68BCA380"
selected_border_width = 0.0`).document
  const exported = parseConfigDocument(stringify(imported)).document
  const effective = resolveConfigDocument(defaults, exported)
  for (const appearance of ['light', 'dark']) {
    assert.deepEqual(selectedCardStyle(effective.window.card, appearance), {
      background: appearance === 'dark' ? '#285245E0' : '#E0F2E9FF',
      border: '#68BCA380', borderWidth: 0,
    })
  }
  for (const invalid of ['selected_background_color = "bad"', 'selected_border_color = "#00FF00"',
    'selected_border_width = -1', 'selected_border_width = 21', 'selected_border_width = nan']) {
    assert.throws(() => parseConfigDocument(`[window.card]\n${invalid}`), /card/)
  }
})
