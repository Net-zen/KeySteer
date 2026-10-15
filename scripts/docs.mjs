import { fileURLToPath } from 'node:url'

const [command, ...args] = process.argv.slice(2)
if (!['dev', 'build', 'check'].includes(command)) {
  throw new Error('Usage: node scripts/docs.mjs <dev|build|check> [options]')
}

// Finish syncing before loading the CLI, using this same Node process.
await import('./sync-doc-assets.mjs')

const checking = command === 'check'
const cli = new URL(checking
  ? '../node_modules/typescript/bin/tsc'
  : '../node_modules/vitepress/bin/vitepress.js', import.meta.url)
process.argv = [
  process.execPath,
  fileURLToPath(cli),
  ...(checking ? ['--noEmit', ...args] : [command, 'docs', ...args]),
]
await import(cli.href)
