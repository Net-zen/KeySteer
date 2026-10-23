import rules from '../../../src/config/compatibility.json' with { type: 'json' }
import { searchEditActions } from './search-edit-keys.ts'

type Table = Record<string, unknown>
interface Rule {
  sections: string[]
  ignore?: string[]
  reject?: Record<string, string>
  copy?: Record<string, { value_type: 'boolean'; targets: string[]; existing_parent_only?: boolean; warning?: string }>
  reject_values?: Record<string, string>
  transform?: 'search_edit_keys' | 'search_copy_keys'
}
interface Pending {
  copies: Array<{ path: string; value: unknown; existingParentOnly: boolean }>
  warnings: string[]
}

function isTable(value: unknown): value is Table {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

/** Apply the native loader's retired-field rules before validating current fields. */
export function normalizeConfigCompatibility(document: Table): string[] {
  const pending: Pending = { copies: [], warnings: [] }
  for (const rule of rules as Rule[]) {
    for (const section of rule.sections) visit(document, section, '', rule, pending)
  }
  for (const copy of pending.copies) copyIfMissing(document, copy.path, copy.value, copy.existingParentOnly)
  return pending.warnings
}

function visit(value: unknown, remaining: string, section: string, rule: Rule, pending: Pending): void {
  if (!remaining) {
    if (rule.transform === 'search_edit_keys' && isTable(value)) normalizeEditKeys(value)
    if (rule.transform === 'search_copy_keys' && Array.isArray(value) && value.length === 5) value.length = 4
    if (!isTable(value)) return
    for (const field of Object.keys(rule.reject ?? {}).sort()) {
      if (Object.hasOwn(value, field)) throw new Error(`${section}.${field} was removed; use ${rule.reject![field]}`)
    }
    for (const field of rule.ignore ?? []) delete value[field]
    for (const field of Object.keys(rule.copy ?? {}).sort()) {
      if (!Object.hasOwn(value, field)) continue
      const migration = rule.copy![field], source = value[field]
      if (migration.value_type === 'boolean' && typeof source !== 'boolean') throw new Error(`${section}.${field} must be a boolean`)
      delete value[field]
      for (const path of migration.targets) pending.copies.push({ path, value: source, existingParentOnly: migration.existing_parent_only ?? false })
      if (migration.warning) pending.warnings.push(migration.warning)
    }
    if (rule.reject_values) for (const key of Object.keys(value).sort()) checkBinding(value[key], section, key, rule.reject_values)
    return
  }
  const dot = remaining.indexOf('.')
  const segment = dot < 0 ? remaining : remaining.slice(0, dot)
  const rest = dot < 0 ? '' : remaining.slice(dot + 1)
  if (Array.isArray(value)) {
    if (segment === '*') value.forEach((child, index) => visit(child, rest, `${section}.${index}`, rule, pending))
  } else if (isTable(value)) for (const key of Object.keys(value).sort()) {
    if (segment === '*' || segment === key) visit(value[key], rest, section ? `${section}.${key}` : key, rule, pending)
  }
}

function copyIfMissing(value: unknown, path: string, source: unknown, existingParentOnly: boolean): void {
  if (!isTable(value)) throw new Error(`parent of ${path} must be a table`)
  const dot = path.indexOf('.')
  if (dot < 0) {
    if (!Object.hasOwn(value, path)) value[path] = source
  } else {
    const segment = path.slice(0, dot)
    if (existingParentOnly && !Object.hasOwn(value, segment)) return
    if (!Object.hasOwn(value, segment)) value[segment] = {}
    copyIfMissing(value[segment], path.slice(dot + 1), source, existingParentOnly)
  }
}

function normalizeEditKeys(table: Table): void {
  const entries = Object.entries(table).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)
  if (entries.every(([, action]) => typeof action === 'string' && searchEditActions.has(action))) return
  if (!entries.every(([action, keys]) => searchEditActions.has(action) && typeof keys === 'string')) return
  const result: Table = Object.create(null)
  for (const [action, alternatives] of entries) {
    let keys = alternatives as string
    while (Object.hasOwn(result, keys)) {
      if (keys.trim()) throw new Error('duplicate search editing key group')
      keys += ' '
    }
    result[keys] = action
  }
  for (const key of Object.keys(table)) delete table[key]
  for (const [key, action] of Object.entries(result)) Object.defineProperty(table, key, { value: action, enumerable: true, configurable: true, writable: true })
}

function checkBinding(value: unknown, section: string, key: string, removed: Record<string, string>): void {
  if (Array.isArray(value)) value.forEach(action => checkBinding(action, section, key, removed))
  else if (typeof value === 'string') {
    const head = value.trim().split(/\s+/)[0]
    if (Object.hasOwn(removed, head)) throw new Error(`${section}.${key}: ${head} was removed; use ${removed[head]}`)
  }
}
