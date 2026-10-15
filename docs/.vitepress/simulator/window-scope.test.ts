import assert from 'node:assert/strict'
import test from 'node:test'
import { createSimulatorState } from './state.ts'
import { applyWindowAction, chooseWindowNumber, enterWindow, leaveWindow, switchWindowMode } from './window.ts'

test('reentry releases minimized and closed numbers while retaining persistent groups', () => {
  const state = createSimulatorState()
  state.pointer = { x: 99, y: 99 }
  state.window.tabs.groups = [{ id: 1, members: [1, 2], active: 2 }]
  state.window.tabs.nextId = 2
  enterWindow(state)
  assert.deepEqual(Object.values(state.window.numbers).sort(), [1, 2, 3, 4])
  leaveWindow(state); state.mode = 'idle'
  state.window.windows.find(w => w.id === 2)!.minimized = true
  state.window.windows = state.window.windows.filter(w => w.id !== 4)
  enterWindow(state)
  assert.deepEqual(state.window.numbers, { 3: 1 })
  assert.deepEqual(state.window.tabs.groups[0].members, [1, 2])
})

test('scope defaults cover all screens and exclude minimized windows', () => {
  const state = createSimulatorState()
  state.window.windows.find(w => w.id === 3)!.minimized = true
  enterWindow(state)
  assert.equal(state.window.numbers[3], undefined)
  assert.notEqual(state.window.numbers[4], undefined)
  const screens = new Set<number>()
  for (let i = 0; i < 5; i++) {
    applyWindowAction(state, 'window_select')
    screens.add(state.window.screen)
    assert.notEqual(state.window.target, 3)
  }
  assert.deepEqual([...screens].sort(), [0, 1])
  switchWindowMode(state, 'window', { include_minimized: true })
  assert.deepEqual(Object.values(state.window.numbers).sort(), [1, 2, 3, 4])
  chooseWindowNumber(state, state.window.numbers[3])
  assert.equal(state.window.target, 3)
  assert.equal(state.window.windows.find(w => w.id === 3)!.minimized, false)
})

test('other window modes default to the current screen and preserve explicit all-screen settings', () => {
  for (const mode of ['window_quick', 'window_editor', 'window_restore', 'window_tab'] as const) {
    for (const fromWindow of [false, true]) {
      const state = createSimulatorState()
      if (fromWindow) switchWindowMode(state, 'window')
      switchWindowMode(state, mode)
      assert.equal(state.window.screens, 'current', mode)
      assert.equal(state.window.numbers[4], undefined, mode)
      switchWindowMode(state, 'window')
      assert.equal(state.window.screens, 'all')
      assert.notEqual(state.window.numbers[4], undefined)
      switchWindowMode(state, mode, { screens: 'all' })
      assert.equal(state.window.screens, 'all', mode)
      assert.notEqual(state.window.numbers[4], undefined, mode)
    }
  }
})

test('all-screen editor arranges each display independently and one undo restores both', () => {
  const state = createSimulatorState()
  const original = structuredClone(state.window.windows)
  switchWindowMode(state, 'window_editor', { screens: 'all' })
  for (const before of original) {
    const after = state.window.windows.find(w => w.id === before.id)!
    assert.equal(after.screen, before.screen)
    assert.notDeepEqual(after, before)
  }
  switchWindowMode(state, 'window')
  applyWindowAction(state, 'window_undo')
  assert.deepEqual(state.window.windows, original)
})


test('F cycles preserve number six while minimized candidates remain unselectable', () => {
  const state = createSimulatorState()
  state.window.windows = Array.from({ length: 7 }, (_, i) => ({ id: i + 1, app: 'Browser', title: `Browser ${i + 1}`, screen: 0, x: 0, y: 0, width: 400, height: 300 }))
  state.pointer = { x: 99, y: 99 }; enterWindow(state)
  chooseWindowNumber(state, 6)
  assert.equal(state.window.target, 6)
  for (let cycle = 0; cycle < 3; cycle++) {
    for (let step = 0; step < 3; step++) {
      applyWindowAction(state, 'size_cycle')
      assert.equal(state.window.numbers[6], 6)
      assert.equal(state.window.target, 6)
      assert.equal(state.window.windowIndex.has('6'), step !== 1)
    }
  }
})
