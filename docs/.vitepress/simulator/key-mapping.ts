import { resolvedChordKeys, type BindingDocument } from './bindings.ts'

// Keep output key names aligned with Key::is_known / builtin_key in api/input.rs.
const aliases: Record<string, string> = {
  escape: 'esc', return: 'enter', del: 'delete',
  up: 'arrow_up', down: 'arrow_down', left: 'arrow_left', right: 'arrow_right',
  pageup: 'page_up', pgup: 'page_up', pagedown: 'page_down', pgdn: 'page_down', pgdown: 'page_down',
  capslock: 'caps_lock', numlock: 'num_lock', scrolllock: 'scroll_lock', printscreen: 'print_screen', prtsc: 'print_screen',
  control: 'ctrl', command: 'win', cmd: 'win', meta: 'win', super: 'win', option: 'alt', opt: 'alt',
  leftcontrol: 'left_ctrl', leftcontrolkey: 'left_ctrl', lctrl: 'left_ctrl', left_control: 'left_ctrl',
  rightcontrol: 'right_ctrl', rightcontrolkey: 'right_ctrl', rctrl: 'right_ctrl', right_control: 'right_ctrl',
  leftalt: 'left_alt', lalt: 'left_alt', left_option: 'left_alt', left_opt: 'left_alt',
  rightalt: 'right_alt', ralt: 'right_alt', right_option: 'right_alt', right_opt: 'right_alt',
  leftshift: 'left_shift', lshift: 'left_shift', rightshift: 'right_shift', rshift: 'right_shift',
  leftwin: 'left_win', lwin: 'left_win', left_command: 'left_win', left_cmd: 'left_win', left_super: 'left_win', left_meta: 'left_win',
  rightwin: 'right_win', rwin: 'right_win', right_command: 'right_win', right_cmd: 'right_win', right_super: 'right_win', right_meta: 'right_win',
  semicolon: ';', apostrophe: "'", quote: "'", comma: ',', period: '.', dot: '.', slash: '/', backslash: '\\',
  grave: '`', backtick: '`', minus: '-', hyphen: '-', equal: '=', equals: '=',
  leftbracket: '[', left_bracket: '[', rightbracket: ']', right_bracket: ']',
}
const namedKeys = new Set('esc enter tab space backspace delete insert home end page_up page_down arrow_up arrow_down arrow_left arrow_right caps_lock num_lock scroll_lock print_screen pause menu add subtract multiply divide decimal fn'.split(' '))
const modifier = /^(?:(?:left|right)_)?(?:ctrl|alt|shift|win)$/

function normalize(value: string): string {
  const key = value.trim().toLowerCase()
  return [...key].length === 1 ? key : key.replace(/[ -]/g, '_')
}

function knownKey(key: string): boolean {
  return [...key].length === 1 && !/\s/.test(key) || namedKeys.has(key) || modifier.test(key)
    || /^f\d+$/.test(key) && Number(key.slice(1)) >= 1 && Number(key.slice(1)) <= 24 || /^numpad_[0-9]$/.test(key)
}

/** Validate a target without changing user aliases or their platform-specific meaning. */
export function parseKeyMapping(input: string, document: BindingDocument = {}, isMac = false): { target: string; binding: string } | undefined {
  const value = input.trim()
  if (!value) return undefined
  const keys = (value === '+' ? [value] : value.split('+')).map(normalize)
  if (keys.some(key => !key)) return undefined
  const target = keys.join('+')
  const resolved = resolvedChordKeys(document, target, isMac).map(key => {
    const normalized = normalize(key)
    return aliases[normalized] ?? (normalized === 'mod' ? isMac ? 'win' : 'ctrl' : normalized)
  })
  if (resolved.some(key => !knownKey(key)) || new Set(resolved).size !== resolved.length
    || resolved.length > 1 && resolved.every(key => modifier.test(key))) return undefined
  // Explicit send also disambiguates escape (the mode-exit verb) and user aliases.
  const bare = keys.length === 1 && knownKey(target) && target !== '+'
  return { target, binding: bare ? target : `send ${target}` }
}

/** Sequences and mode/action bindings must stay in the action editor. */
export function keyMappingTarget(value: unknown, document: BindingDocument = {}, isMac = false): string | undefined {
  if (typeof value !== 'string') return undefined
  const action = value.trim()
  if (action.startsWith('send ')) return parseKeyMapping(action.slice(5), document, isMac)?.target
  if (action === 'escape' || /\s/.test(action)) return undefined
  // A bare user-defined alias may shadow a native verb; only infer built-in keys here.
  const key = normalize(action)
  if (!action.includes('+') && !knownKey(aliases[key] ?? key) && !['primary', 'mod'].includes(key)) return undefined
  return parseKeyMapping(action, document, isMac)?.target
}

type MappingKeyEvent = Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey' | 'repeat' | 'isComposing'>

const browserKeys: Record<string, string> = {
  Backquote: '`', Minus: '-', Equal: '=', BracketLeft: '[', BracketRight: ']', Backslash: '\\',
  Semicolon: ';', Quote: "'", Comma: ',', Period: '.', Slash: '/',
  Space: 'space', CapsLock: 'caps_lock', NumLock: 'num_lock', ScrollLock: 'scroll_lock',
  ArrowLeft: 'arrow_left', ArrowRight: 'arrow_right', ArrowUp: 'arrow_up', ArrowDown: 'arrow_down',
  PrintScreen: 'print_screen', ContextMenu: 'menu',
  NumpadAdd: 'add', NumpadSubtract: 'subtract', NumpadMultiply: 'multiply', NumpadDivide: 'divide',
  NumpadDecimal: 'decimal', NumpadEnter: 'enter',
}

function capturedKey(event: MappingKeyEvent): string | undefined {
  if (event.isComposing || ['Process', 'Dead', 'Unidentified'].includes(event.key)) return undefined
  if (/^Key[A-Z]$/.test(event.code)) return event.code.slice(3).toLowerCase()
  if (/^Digit[0-9]$/.test(event.code)) return event.code.slice(5)
  if (/^Numpad[0-9]$/.test(event.code)) return `numpad_${event.code.slice(6)}`
  if (browserKeys[event.code]) return browserKeys[event.code]
  const key = normalize(event.key === ' ' ? 'space' : event.key)
  const name = aliases[key] ?? key
  return knownKey(name) ? name : undefined
}

function heldModifiers(event: MappingKeyEvent): string[] {
  // Record the actual modifiers, never Primary: users may map Primary to Alt.
  return [event.ctrlKey && 'ctrl', event.altKey && 'alt', event.shiftKey && 'shift', event.metaKey && 'win'].filter(Boolean) as string[]
}

/** Scoped to one focused recorder. Key-up completes a chord, so Enter/Space cannot activate its button. */
export class KeyMappingRecorder {
  private held = new Map<string, string>()
  private chord = ''

  reset(): void { this.held.clear(); this.chord = '' }

  press(event: MappingKeyEvent): string | undefined {
    if (event.repeat) return this.chord || undefined
    const key = capturedKey(event)
    if (!key) { this.reset(); return undefined }
    this.held.set(event.code || event.key, key)
    const modifiers = heldModifiers(event)
    if (modifier.test(key) && !modifiers.includes(key)) modifiers.push(key)
    const keys = [...this.held.values()].filter(name => !modifier.test(name))
    this.chord = [...new Set([...modifiers, ...keys])].join('+')
    return this.chord
  }

  release(event: MappingKeyEvent): string | undefined {
    const id = event.code || event.key
    const key = this.held.get(id)
    if (!key) return undefined // Ignore the key that originally opened the recorder.
    this.held.delete(id)
    if (modifier.test(key) && (this.held.size || heldModifiers(event).length)) return undefined
    const chord = this.chord
    this.reset()
    return chord || undefined
  }
}

export function keyMappingCaption(target: string, isMac = false): string {
  const names: Record<string, string> = {
    ctrl: 'Ctrl', alt: isMac ? 'Option' : 'Alt', shift: 'Shift', win: isMac ? 'Cmd' : 'Win', cmd: 'Cmd', primary: 'Primary',
    arrow_left: '←', arrow_right: '→', arrow_up: '↑', arrow_down: '↓',
    page_up: 'Page Up', page_down: 'Page Down', esc: 'Esc', backspace: 'Backspace',
  }
  return (target === '+' ? ['+'] : target.split('+')).map(key => names[key] ?? key.replaceAll('_', ' ').replace(/\b\w/g, char => char.toUpperCase())).join(' + ')
}
