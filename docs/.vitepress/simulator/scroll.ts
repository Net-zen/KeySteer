type Amount = 'step' | 'half' | 'full'
type Speed = 'precision' | 'fast' | 'slow'
export interface ScrollDelta { x: number; y: number }
export interface CompiledScrollSettings {
  pixels: Record<Amount, number>
  speeds: Record<Amount, number>
  multipliers: Record<Speed, number>
  continuous: boolean
  xSign: number
  ySign: number
}

/** Compile distances, velocities and platform direction once per configuration. */
export function compileScrollSettings(document: Record<string, any>, mac = false): CompiledScrollSettings {
  const scroll = document.scroll ?? {}, pointer = document.pointer ?? {}
  const rate = Number(scroll.steps_per_second ?? 10)
  const pixels = { step: Number(scroll.scroll_step ?? 50), half: Number(scroll.scroll_step_half ?? 500), full: Number(scroll.scroll_step_full ?? 1000000) }
  const validRate = Number.isFinite(rate) && rate >= 0 && rate <= 120 ? rate : 0
  const platform = document.platform?.macos?.scroll ?? {}
  const legacy = platform.invert ?? scroll.invert_scroll
  return {
    pixels,
    speeds: { step: pixels.step * validRate, half: pixels.half * validRate, full: pixels.full * validRate },
    multipliers: { precision: Number(pointer.precision_multiplier ?? .12), fast: Number(pointer.fast_multiplier ?? 2), slow: Number(pointer.slow_multiplier ?? .35) },
    continuous: validRate > 0,
    xSign: mac && (platform.invert_horizontal ?? legacy ?? false) ? -1 : 1,
    ySign: mac && (platform.invert_vertical ?? legacy ?? true) ? -1 : 1,
  }
}

function gesture(action: string): { amount: Amount; x: number; y: number } | undefined {
  const match = /^(?:scroll|wheel)_(?:(half|full)_)?(up|down|left|right)$/.exec(action)
  if (!match || match[1] && ['left', 'right'].includes(match[2])) return
  return { amount: (match[1] ?? 'step') as Amount, x: match[2] === 'right' ? 1 : match[2] === 'left' ? -1 : 0, y: match[2] === 'down' ? 1 : match[2] === 'up' ? -1 : 0 }
}

export function isScrollAction(action: string): boolean { return gesture(action) !== undefined }

/** Key-owned scroll state; native-style pixel integration on animation frames. */
export class ScrollMotion {
  private held = new Map<string, Set<string>>()
  private toggled: Speed | undefined
  private velocity: ScrollDelta = { x: 0, y: 0 }
  private remainder: ScrollDelta = { x: 0, y: 0 }
  private lastFrame: number | undefined
  private scrollCount = 0
  private multiplier = 1
  private settings: CompiledScrollSettings

  constructor(settings: CompiledScrollSettings) { this.settings = settings }

  configure(settings: CompiledScrollSettings): void { this.settings = settings; this.clear() }
  get active(): boolean { return this.settings.continuous && this.scrollCount > 0 }

  tap(action: string): ScrollDelta | undefined {
    const scroll = gesture(action)
    if (!scroll) return
    const distance = this.settings.pixels[scroll.amount] * this.multiplier
    return { x: scroll.x * distance * this.settings.xSign || 0, y: scroll.y * distance * this.settings.ySign || 0 }
  }

  press(key: string, action: string, timestamp: number): ScrollDelta | undefined {
    const scroll = gesture(action)
    const speed = /^(precision|fast|slow)(_toggle)?$/.exec(action)
    if (!scroll && !speed) return
    const canonical = action.replace(/^wheel_/, 'scroll_')
    const actions = this.held.get(key) ?? new Set<string>()
    if (actions.has(canonical)) return { x: 0, y: 0 }
    const wasActive = this.active
    actions.add(canonical); this.held.set(key, actions)
    if (speed?.[2]) this.toggled = this.toggled === speed[1] ? undefined : speed[1] as Speed
    this.refresh()
    if (!wasActive && this.active) this.lastFrame = timestamp
    if (!scroll) return { x: 0, y: 0 }
    this.remainder = { x: 0, y: 0 }
    return this.tap(action)
  }

  release(key: string): void {
    const actions = this.held.get(key)
    if (!actions) return
    this.held.delete(key); this.refresh()
    if ([...actions].some(isScrollAction)) this.remainder = { x: 0, y: 0 }
    if (!this.active) this.lastFrame = undefined
  }

  frame(timestamp: number): ScrollDelta {
    if (!this.active || this.lastFrame === undefined) return { x: 0, y: 0 }
    const seconds = Math.max(0, timestamp - this.lastFrame) / 1000
    this.lastFrame = timestamp
    this.remainder.x += this.velocity.x * seconds
    this.remainder.y += this.velocity.y * seconds
    const delta = { x: Math.trunc(this.remainder.x) || 0, y: Math.trunc(this.remainder.y) || 0 }
    this.remainder.x -= delta.x; this.remainder.y -= delta.y
    return delta
  }

  clear(): void {
    this.held.clear(); this.toggled = undefined; this.scrollCount = 0; this.multiplier = 1
    this.velocity = { x: 0, y: 0 }; this.remainder = { x: 0, y: 0 }; this.lastFrame = undefined
  }

  private refresh(): void {
    const actions = [...this.held.values()].flatMap(held => [...held])
    const speed = (['precision', 'fast', 'slow'] as const).find(speed => actions.includes(speed)) ?? this.toggled
    this.multiplier = speed ? this.settings.multipliers[speed] : 1
    this.velocity = { x: 0, y: 0 }; this.scrollCount = 0
    for (const action of actions) {
      const scroll = gesture(action)
      if (!scroll) continue
      this.scrollCount += 1
      const velocity = this.settings.speeds[scroll.amount] * this.multiplier
      this.velocity.x += scroll.x * velocity * this.settings.xSign
      this.velocity.y += scroll.y * velocity * this.settings.ySign
    }
  }
}
