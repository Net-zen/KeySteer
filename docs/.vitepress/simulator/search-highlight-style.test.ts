import assert from 'node:assert/strict'
import test from 'node:test'
import { stringify } from 'smol-toml'
import { deleteConfigPath, parseConfigDocument, setConfigPath } from '../config-studio/document.ts'
import { fieldLocation } from '../config-studio/navigation.ts'
import { fields } from '../config-studio/fields.ts'
import { searchHighlightStyle } from './search-highlight-style.ts'
import { selectedCardStyle } from './window-card-style.ts'

test('current-label defaults match Window and are not exported until edited', () => {
  const document = parseConfigDocument('[ui_hint]').document
  for (const appearance of ['light', 'dark']) {
    const result = searchHighlightStyle(document, appearance)
    assert.deepEqual({ background: result.background, border: result.border, borderWidth: result.borderWidth }, selectedCardStyle({}, appearance))
  }
  assert.equal(parseConfigDocument(stringify(document)).document.ui_hint.search_highlight, undefined)
  setConfigPath(document, 'ui_hint.search_highlight.background_color', { light: '#12345680', dark: '#ABCDEF80' })
  const exported = parseConfigDocument(stringify(document)).document
  assert.equal(exported.ui_hint.search_highlight.border_width, undefined)
  assert.equal(searchHighlightStyle(exported, 'dark').background, '#ABCDEF80')
  deleteConfigPath(document, 'ui_hint.search_highlight.background_color')
  assert.equal(searchHighlightStyle(document, 'light').background, '#E8F6F0FF')
})

test('current-label fields support themes, transparency and zero border width', () => {
  const document = parseConfigDocument('[ui_hint.search_highlight]\nbackground_color = "#11223380"\ntext_color = { light = "#123456FF", dark = "#ABCDEFff" }\nborder_color = "#68BCA380"\nborder_width = 0.0').document
  assert.deepEqual(searchHighlightStyle(parseConfigDocument(stringify(document)).document, 'dark'), {
    background: '#11223380', text: '#ABCDEFff', border: '#68BCA380', borderWidth: 0,
  })
  const controls = [...fields.ui_hint.colors, ...fields.ui_hint.layout].filter(field => field.path.startsWith('ui_hint.search_highlight.'))
  assert.equal(controls.length, 4)
  for (const field of controls) assert.deepEqual(fieldLocation(field.path), { page: 'ui_hint', tab: 'appearance' })
  assert.equal(searchHighlightStyle({ theme: { light: { text: '#101010FF' } } }, 'light').text, '#101010FF')
  for (const value of ['background_color = "bad"', 'text_color = { light = "#000000FF" }', 'border_width = -1', 'border_width = 21', 'border_width = nan', 'unknown = 1']) {
    assert.throws(() => parseConfigDocument('[ui_hint.search_highlight]\n' + value), /search_highlight/)
  }
})
