import assert from 'node:assert/strict'
import test, { type TestContext } from 'node:test'
import { observeSize } from '../config-studio/observe-size.ts'

function globalStub(t: TestContext, key: string, value: unknown) {
  const original = Object.getOwnPropertyDescriptor(globalThis, key)
  Object.defineProperty(globalThis, key, { configurable: true, writable: true, value })
  t.after(() => {
    if (original) Object.defineProperty(globalThis, key, original)
    else Reflect.deleteProperty(globalThis, key)
  })
}

test('size fallback measures the content box, handles resize and removes its listener', t => {
  const browser = new EventTarget()
  globalStub(t, 'getComputedStyle', () => ({ paddingLeft: '10px', paddingRight: '10px', paddingTop: '5px', paddingBottom: '5px' }))
  globalStub(t, 'window', browser as any)
  globalStub(t, 'ResizeObserver', undefined as any)
  const element = { clientWidth: 380, clientHeight: 250 } as HTMLElement
  const sizes: unknown[] = []
  const stop = observeSize([element], (target, size) => {
    assert.equal(target, element)
    sizes.push(size)
  })
  assert.deepEqual(sizes, [{ width: 360, height: 240 }])
  Object.assign(element, { clientWidth: 200 })
  browser.dispatchEvent(new Event('resize'))
  assert.deepEqual(sizes[1], { width: 180, height: 240 })
  stop()
  browser.dispatchEvent(new Event('resize'))
  assert.equal(sizes.length, 2)
})

test('native size observer reports initial size, streams changes and disconnects', t => {
  let callback: ResizeObserverCallback
  const observed: Element[] = []
  let disconnected = false
  class Observer {
    constructor(changed: ResizeObserverCallback) { callback = changed }
    observe(element: Element) { observed.push(element) }
    disconnect() { disconnected = true }
  }
  globalStub(t, 'getComputedStyle', () => ({}))
  globalStub(t, 'ResizeObserver', Observer as any)
  const element = { clientWidth: 100, clientHeight: 80 } as HTMLElement
  const sizes: unknown[] = []
  const stop = observeSize([element], (_, size) => sizes.push(size))
  assert.deepEqual(observed, [element])
  callback!([{ target: element, contentRect: { width: 200, height: 160 } } as ResizeObserverEntry], {} as ResizeObserver)
  assert.deepEqual(sizes, [{ width: 100, height: 80 }, { width: 200, height: 160 }])
  stop()
  assert.equal(disconnected, true)
})
