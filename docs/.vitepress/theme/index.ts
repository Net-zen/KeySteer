import { defineComponent, h, onMounted, onUnmounted } from 'vue'
import { onContentUpdated } from 'vitepress'
import DefaultTheme from 'vitepress/theme'
import DownloadButton from '../components/DownloadSection'
import MermaidDiagram from '../components/MermaidDiagram'
import type { Theme } from 'vitepress'
import './custom.css'

const Layout = defineComponent({
  name: 'KeySteerLayout',
  setup() {
    const revealRelease = (hash: string) => {
      let id: string
      try { id = decodeURIComponent(hash.slice(1)) } catch { return }
      const version = document.getElementById(id)?.closest<HTMLDetailsElement>('details.ks-release-history')
      if (version) version.open = true
    }
    const revealCurrentRelease = () => revealRelease(window.location.hash)
    const revealLinkedRelease = (event: MouseEvent) => {
      const link = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>('a[href]') : null
      if (!link) return
      const url = new URL(link.href, window.location.href)
      if (url.origin === window.location.origin && url.pathname === window.location.pathname && url.hash) {
        revealRelease(url.hash)
      }
    }
    onContentUpdated(revealCurrentRelease)
    onMounted(() => {
      revealCurrentRelease()
      window.addEventListener('hashchange', revealCurrentRelease)
      document.addEventListener('click', revealLinkedRelease, true)
    })
    onUnmounted(() => {
      window.removeEventListener('hashchange', revealCurrentRelease)
      document.removeEventListener('click', revealLinkedRelease, true)
    })
    return () => h(DefaultTheme.Layout, null, {
      'home-hero-info-after': () => h(DownloadButton),
    })
  },
})

export default {
  ...DefaultTheme,
  Layout,
  enhanceApp({ app }) {
    app.component('MermaidDiagram', MermaidDiagram)
  },
} satisfies Theme
