import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { stringify } from 'smol-toml'
import { parseConfigDocument } from '../config-studio/document.ts'
import { KeyMappingRecorder, keyMappingCaption, keyMappingTarget, parseKeyMapping } from './key-mapping.ts'
import { resolveBinding } from './bindings.ts'

test('direct mappings load from the shipped configuration and round-trip through TOML', () => {
  const document = parseConfigDocument(readFileSync('keysteer.default.toml', 'utf8')).document
  assert.equal(keyMappingTarget(document.normal.bindings.t, document), 'home')
  document.normal.bindings.t = parseKeyMapping(' End ', document)!.binding
  document.normal.bindings.y = parseKeyMapping('Ctrl + C', document)!.binding
  const restored = parseConfigDocument(stringify(document)).document
  assert.equal(restored.normal.bindings.t, 'end')
  assert.equal(restored.normal.bindings.y, 'send ctrl+c')
  assert.equal(resolveBinding(restored, 'normal', 'y')?.value, 'send ctrl+c')
  assert.equal(keyMappingTarget(restored.normal.bindings.y, restored), 'ctrl+c')
})

test('mapping keys cannot accidentally select a mode or action', () => {
  for (const action of ['escape', 'normal', 'grid', 'move_left', 'left_click', 'mouse_x1', 'none', 'exec home', ['home'], ['home', 'end']]) {
    assert.equal(keyMappingTarget(action), undefined, String(action))
  }
  assert.deepEqual(parseKeyMapping('escape'), { target: 'escape', binding: 'send escape' })
  assert.equal(keyMappingTarget('send escape'), 'escape')
  assert.equal(parseKeyMapping('esc')?.binding, 'esc')
  assert.equal(keyMappingTarget('shift+home'), 'shift+home')
})

test('validates key names and chords, including literal symbols and native aliases', () => {
  for (const target of ['home', 'page-up', 'PageUp', 'arrow_left', 'left', 'f24', 'numpad_0', 'subtract', 'return', 'space', '+', '-', '?', 'ctrl+shift+/', 'left_ctrl+a', 'cmd+c']) {
    assert.ok(parseKeyMapping(target), target)
  }
  for (const target of ['', ' ', 'hom', 'f25', 'ctrl+', '+c', 'ctrl++c', 'ctrl+ctrl+c', 'ctrl+control+c', 'ctrl+shift', 'home end', 'send ctrl+c', 'none']) {
    assert.equal(parseKeyMapping(target), undefined, target)
  }
})

test('preserves configured aliases and validates resolved platform keys', () => {
  const document = { key_aliases: { Copy: 'c', windows: { Primary: 'left_alt' }, macos: { Primary: 'left_cmd' } } }
  for (const isMac of [false, true]) {
    assert.deepEqual(parseKeyMapping('Primary+Copy', document, isMac), { target: 'primary+copy', binding: 'send primary+copy' })
    assert.equal(keyMappingTarget('send Primary+Copy', document, isMac), 'primary+copy')
  }
  assert.equal(parseKeyMapping('primary+left_alt+c', document, false), undefined)
  assert.equal(parseKeyMapping('primary+left_cmd+c', document, true), undefined)
  assert.equal(parseKeyMapping('copy+c', document), undefined)
})

function keyEvent(key: string, code: string, overrides = {}) {
  return { key, code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, repeat: false, isComposing: false, ...overrides }
}

test('records physical Ctrl+C on release without substituting the configured Primary alias', () => {
  const recorder = new KeyMappingRecorder()
  recorder.press(keyEvent('Control', 'ControlLeft', { ctrlKey: true }))
  assert.equal(recorder.press(keyEvent('c', 'KeyC', { ctrlKey: true })), 'ctrl+c')
  const target = recorder.release(keyEvent('c', 'KeyC', { ctrlKey: true }))!
  assert.equal(target, 'ctrl+c')
  assert.equal(parseKeyMapping(target, { key_aliases: { windows: { Primary: 'left_alt' } } })?.binding, 'send ctrl+c')
  assert.equal(recorder.release(keyEvent('Control', 'ControlLeft')), undefined)
})

test('records navigation, shifted punctuation and physical numpad keys with Num Lock off', () => {
  for (const [key, code, modifiers, expected] of [
    ['Home', 'Home', {}, 'home'], ['ArrowLeft', 'ArrowLeft', {}, 'arrow_left'],
    ['?', 'Slash', { shiftKey: true }, 'shift+/'], ['+', 'Equal', { shiftKey: true }, 'shift+='],
    ['End', 'Numpad1', {}, 'numpad_1'], ['+', 'NumpadAdd', {}, 'add'],
    ['Enter', 'NumpadEnter', {}, 'enter'], ['c', 'KeyC', { metaKey: true }, 'win+c'],
  ] as const) {
    const recorder = new KeyMappingRecorder()
    recorder.press(keyEvent(key, code, modifiers))
    const target = recorder.release(keyEvent(key, code, modifiers))
    assert.equal(target, expected)
    assert.ok(parseKeyMapping(target!), expected)
  }
  assert.equal(keyMappingCaption('win+shift+c', true), 'Cmd + Shift + C')
})

test('recording Enter, Space and Tab requires a keydown inside the active session', () => {
  const recorder = new KeyMappingRecorder()
  for (const [key, code, expected] of [['Enter', 'Enter', 'enter'], [' ', 'Space', 'space'], ['Tab', 'Tab', 'tab']]) {
    assert.equal(recorder.release(keyEvent(key, code)), undefined)
    recorder.press(keyEvent(key, code))
    assert.equal(recorder.release(keyEvent(key, code)), expected)
  }
})

test('cancelled sessions, auto-repeat, IME and unsupported keys do not leak into the next recording', () => {
  const recorder = new KeyMappingRecorder()
  recorder.press(keyEvent('Control', 'ControlLeft', { ctrlKey: true }))
  recorder.reset()
  assert.equal(recorder.release(keyEvent('Control', 'ControlLeft')), undefined)
  assert.equal(recorder.press(keyEvent('c', 'KeyC', { repeat: true })), undefined)
  assert.equal(recorder.press(keyEvent('Process', 'KeyA', { isComposing: true })), undefined)
  assert.equal(recorder.press(keyEvent('AudioVolumeUp', 'AudioVolumeUp')), undefined)
  recorder.press(keyEvent('Home', 'Home'))
  assert.equal(recorder.release(keyEvent('Home', 'Home')), 'home')
})

test('a modifier tap is supported, while modifier-only combinations wait for completion and stay invalid', () => {
  const recorder = new KeyMappingRecorder()
  recorder.press(keyEvent('Control', 'ControlLeft', { ctrlKey: true }))
  assert.equal(recorder.release(keyEvent('Control', 'ControlLeft')), 'ctrl')
  recorder.press(keyEvent('Control', 'ControlLeft', { ctrlKey: true }))
  recorder.press(keyEvent('Shift', 'ShiftLeft', { ctrlKey: true, shiftKey: true }))
  assert.equal(recorder.release(keyEvent('Shift', 'ShiftLeft', { ctrlKey: true })), undefined)
  assert.equal(parseKeyMapping(recorder.release(keyEvent('Control', 'ControlLeft'))!), undefined)
})
