import { defineComponent, h, onMounted, ref, useId, watch } from 'vue'
import { useData } from 'vitepress'

// Mermaid has global configuration: serialize initialization together with rendering
// so simultaneous diagrams and theme changes cannot overwrite each other's settings.
let renderQueue = Promise.resolve()

function renderDiagram(id: string, source: string, dark: boolean, current: () => boolean) {
  const result = renderQueue.then(async () => {
    const { default: mermaid } = await import('mermaid')
    await document.fonts.ready
    if (!current()) return ''

    // Keep Mermaid's temporary DOM separate from Vue's rendered SVG, including
    // failures and navigation away while a diagram is being laid out.
    const container = document.createElement('div')
    container.className = 'vp-doc'
    container.style.cssText = 'position:absolute;left:-100000px;top:0;visibility:hidden;'
    // Measure with the same label wrapping and font weight as the final SVG.
    const stage = document.createElement('div')
    stage.className = 'mermaid'
    container.append(stage)
    document.body.append(container)
    try {
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: 'loose',
        suppressErrorRendering: true,
        theme: dark ? 'dark' : 'default',
      })
      return (await mermaid.render(id, source, stage)).svg
    } finally {
      container.remove()
    }
  })
  // A malformed diagram must not prevent subsequent diagrams from rendering.
  renderQueue = result.then(() => {}, () => {})
  return result
}

export default defineComponent({
  name: 'MermaidDiagram',
  props: { graph: { type: String, required: true } },
  setup(props) {
    const { isDark, lang } = useData()
    const id = `ks-mermaid-${useId().replace(/[^a-zA-Z0-9_-]/g, '-')}`
    const svg = ref('')
    const error = ref('')
    let revision = 0

    // Import and render only in the browser; SSR displays the source as a fallback.
    onMounted(() => {
      watch([() => props.graph, isDark], async ([graph, dark], _, onCleanup) => {
        let active = true
        onCleanup(() => { active = false })
        error.value = ''
        try {
          const rendered = await renderDiagram(
            `${id}-${++revision}`, decodeURIComponent(graph), dark, () => active,
          )
          if (active) svg.value = rendered
        } catch (cause) {
          if (!active) return
          svg.value = ''
          error.value = cause instanceof Error ? cause.message : String(cause)
        }
      }, { immediate: true })
    })

    return () => svg.value
      ? h('div', { class: 'mermaid', innerHTML: svg.value })
      : h('div', { class: 'mermaid' }, [
        error.value && h('p', { role: 'alert' }, [
          lang.value.startsWith('en') ? 'Diagram rendering failed: ' : '图表渲染失败：',
          error.value,
        ]),
        h('pre', [h('code', decodeURIComponent(props.graph))]),
      ])
  },
})
