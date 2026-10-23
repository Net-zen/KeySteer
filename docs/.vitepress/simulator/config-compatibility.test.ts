import assert from 'node:assert/strict'
import test from 'node:test'
import cases from '../../../tests/fixtures/config-compatibility.json' with { type: 'json' }
import { parse, stringify } from 'smol-toml'
import { normalizeConfigCompatibility } from '../config-studio/compatibility.ts'
import { parseConfigDocument } from '../config-studio/document.ts'

test('native and browser share retired-field behavior and migration messages', () => {
  for (const entry of cases) {
    const value = parse(entry.source)
    if (entry.error) {
      assert.throws(() => normalizeConfigCompatibility(value), { message: entry.error }, entry.name)
      assert.throws(() => parseConfigDocument(entry.source), { message: entry.error }, entry.name)
    } else {
      const expected = parse(entry.normalized!)
      assert.deepEqual(normalizeConfigCompatibility(value), entry.warnings ?? [], entry.name)
      assert.deepEqual(value, expected, entry.name)
      assert.deepEqual(normalizeConfigCompatibility(value), [], `${entry.name}: no repeated warnings`)
      assert.deepEqual(value, expected, `${entry.name}: idempotent`)
      const parsed = parseConfigDocument(entry.source)
      assert.deepEqual(parsed.warnings, entry.warnings ?? [], entry.name)
      const imported = parsed.document
      assert.deepEqual(imported, expected, entry.name)
      assert.deepEqual(parseConfigDocument(stringify(imported)).document, expected, entry.name)
    }
  }
})
