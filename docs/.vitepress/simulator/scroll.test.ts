import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'
import { parseConfigDocument } from '../config-studio/document.ts'
import { compileScrollSettings, ScrollMotion } from './scroll.ts'

test('shipped scroll defaults match the sparse configuration fallback', async () => {
  const document = parseConfigDocument(await readFile(new URL('../../../keysteer.default.toml', import.meta.url), 'utf8')).document
  for (const mac of [false, true]) assert.deepEqual(compileScrollSettings(document, mac), compileScrollSettings({}, mac))
})

test('scroll starts immediately, integrates the first frame and stops on release', () => {
  const motion = new ScrollMotion(compileScrollSettings({}))
  assert.deepEqual(motion.press('KeyM', 'scroll_down', 0), { x: 0, y: 50 })
  assert.deepEqual(motion.press('KeyM', 'scroll_down', 1), { x: 0, y: 0 })
  assert.deepEqual(motion.frame(16), { x: 0, y: 8 })
  assert.deepEqual(motion.frame(100), { x: 0, y: 42 })
  motion.release('KeyM')
  assert.equal(motion.active, false)
  assert.deepEqual(motion.frame(200), { x: 0, y: 0 })
})

test('pixel speeds are independent of tap distance', () => {
  for (const rate of [250, 500, 1000]) {
    for (const tap of [20, 50, 200]) {
      const motion = new ScrollMotion(compileScrollSettings({ scroll: { speed: rate, scroll_step: tap } }))
      assert.deepEqual(motion.press('KeyM', 'scroll_down', 0), { x: 0, y: tap })
      assert.deepEqual(motion.frame(1000), { x: 0, y: rate })
    }
  }
})

test('page scrolls are discrete and do not stop another held scroll', () => {
  for (const action of ['scroll_half_down', 'scroll_full_down']) {
    const motion = new ScrollMotion(compileScrollSettings({}))
    assert.ok(motion.press('KeyE', action, 0)!.y > 0)
    assert.equal(motion.active, false)
    assert.deepEqual(motion.press('KeyE', action, 1), { x: 0, y: 0 })
    motion.press('KeyM', 'scroll_down', 1)
    assert.equal(motion.active, true)
    motion.release('KeyE')
    assert.deepEqual(motion.frame(101), { x: 0, y: 50 })
    motion.release('KeyM')
    assert.equal(motion.active, false)
  }
})

test('compiled fractional speed applies only to ordinary scrolling', () => {
  const settings = compileScrollSettings({ scroll: { scroll_step: 20, scroll_step_half: 100, scroll_step_full: 400, speed: 52.5 } })
  for (const [action, tap, held] of [['wheel_down', 20, 10], ['scroll_half_down', 100, 0], ['scroll_full_down', 400, 0]] as const) {
    const motion = new ScrollMotion(settings)
    assert.deepEqual(motion.press('KeyM', action, 0), { x: 0, y: tap })
    assert.deepEqual(motion.frame(200), { x: 0, y: held })
  }
})

test('elapsed time and subpixel remainders preserve distance across refresh rates', () => {
  for (const hz of [60, 120, 144, 240]) {
    const motion = new ScrollMotion(compileScrollSettings({ scroll: { scroll_step: 1, speed: 10 } }))
    motion.press('CapsLock', 'precision', 0)
    motion.press('KeyM', 'scroll_down', 0)
    let distance = 0
    for (let frame = 1; frame <= hz * 10; frame++) distance += motion.frame(frame * 1000 / hz).y
    assert.ok(Math.abs(distance - 12) <= 1, `${hz} Hz: ${distance}`)
  }
})

test('zero speed permits a tap without continuous scrolling or autorepeat', () => {
  const motion = new ScrollMotion(compileScrollSettings({ scroll: { speed: 0 } }))
  assert.deepEqual(motion.press('KeyM', 'scroll_down', 0), { x: 0, y: 50 })
  assert.equal(motion.active, false)
  assert.deepEqual(motion.press('KeyM', 'wheel_down', 1), { x: 0, y: 0 })
  assert.deepEqual(motion.frame(1000), { x: 0, y: 0 })
})

test('speed modifiers and toggles update a held scroll with native precedence', () => {
  const motion = new ScrollMotion(compileScrollSettings({}))
  motion.press('KeyM', 'scroll_down', 0)
  motion.press('KeyV', 'fast_toggle', 0)
  motion.release('KeyV')
  assert.equal(motion.frame(100).y, 100)
  motion.press('ShiftLeft', 'slow', 100)
  assert.equal(motion.frame(200).y, 17)
  motion.press('KeyB', 'fast', 200)
  motion.press('CapsLock', 'precision', 200)
  assert.equal(motion.frame(300).y, 6)
  motion.release('CapsLock')
  assert.equal(motion.frame(400).y, 100)
  motion.release('KeyB'); motion.release('ShiftLeft')
  assert.equal(motion.frame(500).y, 100)
})

test('all six speed actions scale held scrolling and restore its base speed', () => {
  for (const [speed, multiplier] of [['precision', .12], ['slow', .35], ['fast', 2]] as const) {
    for (const toggle of [false, true]) {
      const motion = new ScrollMotion(compileScrollSettings({}))
      motion.press('KeyM', 'scroll_down', 0)
      const action = speed + (toggle ? '_toggle' : '')
      motion.press('ShiftLeft', action, 0)
      if (toggle) motion.release('ShiftLeft')
      assert.equal(motion.frame(1000).y, Math.trunc(500 * multiplier), action)
      if (toggle) motion.press('ShiftLeft', action, 1000)
      else motion.release('ShiftLeft')
      assert.equal(motion.frame(2000).y, 500, action)
    }
  }
})

test('multiple gestures share key ownership and cancelling directions resume on release', () => {
  const motion = new ScrollMotion(compileScrollSettings({}))
  motion.press('KeyM', 'scroll_down', 0)
  motion.press('KeyM', 'wheel_right', 0)
  motion.press('Comma', 'scroll_up', 0)
  assert.deepEqual(motion.frame(100), { x: 50, y: 0 })
  motion.release('Comma')
  assert.deepEqual(motion.frame(200), { x: 50, y: 50 })
  motion.release('KeyM')
  assert.equal(motion.active, false)
})

test('macOS inversion and configuration changes apply to taps and held frames', () => {
  const motion = new ScrollMotion(compileScrollSettings({}, true))
  assert.deepEqual(motion.press('KeyM', 'scroll_down', 0), { x: 0, y: -50 })
  assert.deepEqual(motion.frame(100), { x: 0, y: -50 })
  motion.configure(compileScrollSettings({ platform: { macos: { scroll: { invert_horizontal: true, invert_vertical: false } } } }, true))
  assert.equal(motion.active, false)
  assert.deepEqual(motion.frame(200), { x: 0, y: 0 })
  assert.deepEqual(motion.press('Period', 'scroll_right', 200), { x: -50, y: 0 })
  assert.deepEqual(motion.frame(300), { x: -50, y: 0 })
})
