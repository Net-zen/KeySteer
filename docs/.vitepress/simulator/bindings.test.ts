import assert from 'node:assert/strict'
import test from 'node:test'
import { displayedBindings, effectiveBindings, resolveBinding, resolveLayeredPhysicalBinding, temporaryPhysicalKeys } from './bindings.ts'
import { resolveConfigDocument } from '../config-studio/document.ts'

test('all mode layers require explicit triggers and use the same precedence', () => {
  for (const mode of ['normal', 'quick_switch', 'text_input', 'text_select', 'grid', 'recursive_grid', 'ui_hint', 'window', 'window_quick', 'window_editor', 'window_restore', 'window_tab']) {
    const borrowed = mode === 'normal' ? 'text_input' : 'normal'
    const defaults = {
      [borrowed]: { bindings: { f9: 'move_left', 'right_alt+f10': 'move_right', f11: 'move_left' } },
      [mode]: { temporary_mode: borrowed, temporary_mode_keys: ['right_alt'], temporary_mode_passthrough_keys: ['f11', 'right_alt'], bindings: { 'right_alt+f10': 'move_up', f11: 'move_up', right_alt: 'fast' } },
    }
    for (const document of [{}, { [mode]: {} }, { [mode]: { temporary_mode_keys: [] } }]) {
      const effective = resolveConfigDocument(defaults, document)
      assert.deepEqual(temporaryPhysicalKeys(effective, mode, ['right_alt'], false), [], mode)
      assert.equal(resolveLayeredPhysicalBinding(effective, mode, ['right_alt', 'f9'], 'f9', false), undefined, mode)
    }
    const enabled = resolveConfigDocument(defaults, { [mode]: { temporary_mode_keys: ['right_alt'] } })
    for (const [key, action] of [['f9', 'move_left'], ['f10', 'move_right'], ['f11', 'move_up'], ['right_alt', 'fast']]) {
      const pressed = key === 'right_alt' ? [key] : ['right_alt', key]
      assert.equal(resolveLayeredPhysicalBinding(enabled, mode, pressed, key, false)?.value, action, mode)
    }
  }
})

test('temporary Normal keycap highlights appear only while the trigger is held', () => {
  const document = {
    normal: { bindings: { h: 'move_left' } },
    grid: { inherits: ['normal'], temporary_mode: 'normal', temporary_mode_keys: ['primary'], bindings: { a: 'finish' } },
  }
  const held = (pressed: string[]) => temporaryPhysicalKeys(document, 'grid', pressed, false).length > 0
  assert.equal(displayedBindings(document, 'grid', held([])).has('h'), false)
  assert.equal(displayedBindings(document, 'grid', held([])).get('a')?.source, 'grid')
  assert.equal(displayedBindings(document, 'grid', held(['left_ctrl'])).get('h')?.source, 'normal')
  assert.equal(displayedBindings(document, 'grid', held(['left_ctrl'])).get('a')?.source, 'grid')
  assert.equal(effectiveBindings(document, 'grid').get('h')?.source, 'normal')
})

test('targeting modes inherit normal bindings after their local bindings', () => {
  const document = {
    hotkeys: { 'primary+e': 'normal' },
    normal: { inherits: ['hotkeys'], bindings: { h: 'move_left', ';': 'left_click' } },
    grid: { inherits: ['hotkeys', 'normal'], bindings: { '`': 'follow' } },
  }
  assert.equal(resolveBinding(document, 'grid', 'h')?.value, 'move_left')
  assert.equal(resolveBinding(document, 'grid', 'primary+e')?.value, 'normal')
  assert.equal(resolveBinding(document, 'grid', '`')?.value, 'follow')
  assert.equal(effectiveBindings(document, 'grid').get(';')?.source, 'normal')
})

test('local none blocks an inherited binding', () => {
  const document = {
    normal: { bindings: { h: 'move_left' } },
    ui_hint: { inherits: ['normal'], bindings: { h: 'none' } },
  }
  assert.equal(resolveBinding(document, 'ui_hint', 'h')?.value, 'none')
  assert.equal(effectiveBindings(document, 'ui_hint').has('h'), false)
})

test('whitespace-separated binding aliases color and resolve each key', () => {
  const document = {
    normal: { bindings: { 'v b': 'fast' } },
  }

  assert.equal(resolveBinding(document, 'normal', 'v')?.value, 'fast')
  assert.equal(resolveBinding(document, 'normal', 'b')?.value, 'fast')
  assert.equal(effectiveBindings(document, 'normal').has('v b'), false)
})

test('symbol bindings are literal and can be omitted', () => {
  const document = { normal: { bindings: { '?': 'key_help' } } }
  assert.equal(resolveBinding(document, 'normal', 'shift+/'), undefined)
  assert.equal(resolveBinding(document, 'normal', '?')?.value, 'key_help')
  assert.equal(resolveBinding(document, 'normal', '/'), undefined)
  assert.equal(resolveBinding({ normal: { bindings: {} } }, 'normal', 'shift+/'), undefined)
})
