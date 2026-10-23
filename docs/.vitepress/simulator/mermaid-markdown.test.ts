import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createMarkdownRenderer } from 'vitepress'
import { mermaidMarkdown } from '../mermaid-markdown.ts'

const md = await createMarkdownRenderer(process.cwd())
md.use(mermaidMarkdown)

test('diagram fences preserve Unicode, HTML labels and Vue-like text as data', async () => {
  const source = 'flowchart LR\n A["中文<br/> & \'quotes\' {{ braces }}"] --> B\n'
  for (const language of ['mermaid', 'mmd']) {
    const html = await md.renderAsync(`\`\`\`${language}\n${source}\`\`\`\n`, {})
    const encoded = /<MermaidDiagram graph="([^"]*)" \/>/.exec(html)?.[1]
    assert.ok(encoded)
    assert.equal(decodeURIComponent(encoded), source)
    assert.doesNotMatch(html, /<br\/>|{{ braces }}|<Suspense>/)
  }
})

test('ordinary code fences retain VitePress syntax highlighting', async () => {
  const plain = await createMarkdownRenderer(process.cwd())
  const source = '```ts\nconst value = 1\n```\n'
  assert.equal(await md.renderAsync(source, {}), await plain.renderAsync(source, {}))
})

test('existing diagram pages use the local component without rewriting Markdown', async () => {
  for (const file of [
    'docs/guide/getting-started.md', 'docs/en/guide/getting-started.md',
    'docs/modes/index.md', 'docs/en/modes/index.md', 'docs/development/architecture.md',
  ]) {
    const source = readFileSync(file, 'utf8')
    const expected = [...source.matchAll(/^```mermaid\s*$/gm)].length
    const html = await md.renderAsync(source, {})
    assert.equal((html.match(/<MermaidDiagram graph=/g) ?? []).length, expected, file)
    assert.ok(expected > 0)
  }
})
