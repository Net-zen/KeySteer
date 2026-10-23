import type { MarkdownRenderer } from 'vitepress'

/** Keep diagrams as ordinary fenced blocks in the documentation source. */
export function mermaidMarkdown(md: MarkdownRenderer): void {
  const fence = md.renderer.rules.fence!
  md.renderer.rules.fence = (tokens, index, options, env, self) => {
    const token = tokens[index]
    if (!['mermaid', 'mmd'].includes(token.info.trim())) {
      return fence(tokens, index, options, env, self)
    }
    return `<MermaidDiagram graph="${md.utils.escapeHtml(encodeURIComponent(token.content))}" />\n`
  }
}
