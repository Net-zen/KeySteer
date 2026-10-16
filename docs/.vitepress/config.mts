import vueJsx from '@vitejs/plugin-vue-jsx'
import type { MarkdownRenderer } from 'vitepress'
import { loadLatestRelease } from './latest-release.ts'
import { releaseHistory } from './release-history.ts'
import { mermaidMarkdown } from './mermaid-markdown.ts'

const base = process.env.KEYSTEER_DOCS_BASE || '/'
const windowSidebar = (prefix = '', english = false) => ({
  text: english ? 'Window management' : '窗口管理',
  items: [
    { text: english ? 'Overview' : '窗口管理总览', link: `${prefix}/window-management/` },
    { text: english ? 'Window · Move and control' : 'Window 窗口操作', link: `${prefix}/window-management/window` },
    { text: english ? 'Quick · Split layouts' : 'Quick 快捷分屏', link: `${prefix}/window-management/quick` },
    { text: english ? 'Editor · Tile and edit' : 'Editor 平铺与编辑', link: `${prefix}/window-management/editor` },
    { text: english ? 'Tabs · Window groups' : 'Tabs 标签分组', link: `${prefix}/window-management/tabs` },
    { text: english ? 'Restore · Saved workspaces' : 'Restore 保存与恢复', link: `${prefix}/window-management/restore` },
  ],
})
const developmentSidebar = (prefix = '', english = false) => ({
  text: english ? 'Development' : '开发',
  items: [
    { text: english ? 'Architecture' : '架构', link: `${prefix}/development/architecture` },
    { text: english ? 'Extension guide' : '扩展指南', link: `${prefix}/development/extension-guide` },
    { text: english ? 'Workflow and tests' : '开发流程与测试', link: `${prefix}/development/workflow` },
    { text: english ? 'Plugin development' : '插件开发', link: `${prefix}/development/plugin-development` },
    { text: english ? 'Safety and performance' : '安全与性能约束', link: `${prefix}/development/safety-and-performance` },
  ],
})
const englishTheme = {
  nav: [
    { text: 'Get started', link: '/en/guide/getting-started' },
    { text: 'Modes', link: '/en/modes/' },
    { text: 'Configuration', link: '/en/reference/configuration' },
    { text: 'Development', link: '/en/development/architecture' },
    { text: 'Configuration & simulator', link: '/en/editor/' },
  ],
  sidebar: [
    { text: 'Get started', items: [{ text: 'Getting started', link: '/en/guide/getting-started' }, { text: 'macOS', link: '/en/guide/macos' }] },
    { text: 'Modes', items: [
      { text: 'Overview', link: '/en/modes/' }, { text: 'Normal', link: '/en/modes/normal' },
      { text: 'Grid', link: '/en/modes/grid' }, { text: 'Recursive Grid', link: '/en/modes/recursive-grid' },
      { text: 'UI Hint', link: '/en/modes/ui-hint' },
    ] },
    windowSidebar('/en', true),
    { text: 'Reference', items: [
      { text: 'Configuration', link: '/en/reference/configuration' },
      { text: 'Modes and actions', link: '/en/reference/modes-and-actions' }, { text: 'Release notes', link: '/en/releases/' },
    ] },
    developmentSidebar('/en', true),
    { text: 'Tools', items: [{ text: 'Configuration & simulator', link: '/en/editor/' }] },
  ],
  search: {
    provider: 'local',
    options: {
      translations: {
        button: { buttonText: 'Search', buttonAriaLabel: 'Search documentation' },
        modal: {
          displayDetails: 'Display detailed list', resetButtonTitle: 'Reset search', backButtonTitle: 'Close search', noResultsText: 'No results for',
          footer: { selectText: 'Select', selectKeyAriaLabel: 'Enter', navigateText: 'Navigate', navigateUpKeyAriaLabel: 'Arrow up', navigateDownKeyAriaLabel: 'Arrow down', closeText: 'Close', closeKeyAriaLabel: 'Esc' },
        },
      },
    },
  },
  outline: { label: 'On this page', level: [2, 3] as [number, number] },
  docFooter: { prev: 'Previous page', next: 'Next page' },
  lastUpdated: { text: 'Last updated' },
}
const languageStateScript = `(() => {
  const base = ${JSON.stringify(base)}
  const key = 'keysteer-docs-language'
  const englishPages = new Set([
    '', 'guide/getting-started', 'guide/macos', 'modes/', 'modes/grid',
    'modes/window', 'modes/normal', 'modes/recursive-grid', 'modes/ui-hint', 'reference/configuration',
    'reference/modes-and-actions', 'editor/', 'simulator', 'releases/',
    'window-management/', 'window-management/window', 'window-management/quick',
    'window-management/editor', 'window-management/tabs', 'window-management/restore',
    'development/architecture', 'development/extension-guide', 'development/workflow',
    'development/plugin-development', 'development/safety-and-performance',
  ])
  const relativePath = () => decodeURIComponent(location.pathname.startsWith(base)
    ? location.pathname.slice(base.length) : location.pathname.slice(1))
  const normalise = (path) => path.replace(/^\\/+/, '').replace(/index\\.html$/, '')
  const switchPath = (english) => {
    const current = normalise(relativePath())
    const withoutLanguage = current.startsWith('en/') ? current.slice(3) : current
    if (english && !englishPages.has(withoutLanguage)) return null
    return base + (english ? 'en/' : '') + withoutLanguage
  }
  const currentIsEnglish = () => normalise(relativePath()).startsWith('en/')
  let stored = null
  try { stored = localStorage.getItem(key) } catch {}
  if (stored === 'en' && !currentIsEnglish()) {
    const destination = switchPath(true)
    if (destination && destination !== location.pathname) location.replace(destination)
  }
  document.addEventListener('click', (event) => {
    const target = event.target instanceof Element
      ? event.target.closest('.VPNavBarTranslations a') : null
    if (!target) return
    const english = target.getAttribute('href')?.includes('/en/') ?? false
    const destination = switchPath(english)
    try { localStorage.setItem(key, english ? 'en' : 'zh') } catch {}
    if (!destination) return
    event.preventDefault()
    location.assign(destination)
  }, true)
})()`

export default async ({ command }: { command: unknown }) => {
  // Use the supplied command rather than global argv or NODE_ENV.
  const latestRelease = await loadLatestRelease(command, process.env.GITHUB_TOKEN, process.env.KEYSTEER_DOCS_RELEASE_TAG)
  return {
  // Internal maintenance notes are repository documentation, not site pages.
  srcExclude: ['ai/**'],
  markdown: { config(md: MarkdownRenderer) { releaseHistory(md); mermaidMarkdown(md) } },
  base,
  lang: 'zh-CN',
  locales: {
    root: { label: '简体中文', lang: 'zh-CN' },
    en: {
      label: 'English',
      lang: 'en-US',
      title: 'KeySteer',
      description: 'Native keyboard control for your pointer, windows, and workspace.',
      themeConfig: englishTheme,
    },
  },
  title: 'KeySteer',
  description: '用键盘完成鼠标操作、窗口分屏与标签分组，搭建可复用的工作区。',
  cleanUrls: true,
  ignoreDeadLinks: [/^\/generated\//],
  head: [
    ['link', { rel: 'icon', type: 'image/png', href: `${base}generated/keysteer-icon.png` }],
    ['meta', { name: 'theme-color', content: '#6578d4' }],
    ['style', { id: 'ks-html-page-style' }, '.ks-standalone{min-width:0}'],
    ['script', {}, languageStateScript],
  ],
  vite: {
    optimizeDeps: {
      // Mermaid's lazy diagram chunks import CommonJS dependencies such as elkjs.
      // Prebundle the entry so those imports are converted before browser loading.
      include: ['mermaid'],
    },
    build: {
      // Pages serves the built files; gzip size reporting is only a console statistic.
      reportCompressedSize: false,
    },
    define: {
      __KEYSTEER_LATEST_RELEASE__: JSON.stringify(latestRelease),
    },
    plugins: [vueJsx()],
    ssr: {
      // Diagrams import Mermaid only after mounting in the browser.
      external: ['mermaid'],
    },
  },
  themeConfig: {
    logo: '/generated/keysteer-icon.png',
    nav: [
      { text: '快速上手', link: '/guide/getting-started' },
      { text: '模式', link: '/modes/' },
      { text: '配置参考', link: '/reference/configuration' },
      { text: '一起开发', link: '/development/architecture' },
      { text: '配置与模拟器', link: '/editor/' },
    ],
    sidebar: [
      {
        text: '开始使用',
        items: [
          { text: '快速上手', link: '/guide/getting-started' },
          { text: 'macOS', link: '/guide/macos' },
        ],
      },
      {
        text: '模式',
        items: [
          { text: '模式总览', link: '/modes/' },
          { text: 'Normal 普通模式', link: '/modes/normal' },
          { text: 'Grid 网格模式', link: '/modes/grid' },
          { text: 'Recursive Grid 递归网格', link: '/modes/recursive-grid' },
          { text: 'UI 标签模式', link: '/modes/ui-hint' },
        ],
      },
      windowSidebar(),
      {
        text: '参考',
        items: [
          { text: '配置文件', link: '/reference/configuration' },
          { text: '模式与动作', link: '/reference/modes-and-actions' },
          { text: '更新日志 / Release Notes', link: '/releases/' },
        ],
      },
      developmentSidebar(),
      {
        text: '工具',
        items: [{ text: '配置与模拟器', link: '/editor/' }],
      },
    ],
    socialLinks: [{ icon: 'github', link: 'https://github.com/dccif/KeySteer' }],
    search: {
      provider: 'local',
      options: {
        locales: {
          en: {
            translations: {
              button: {
                buttonText: 'Search',
                buttonAriaLabel: 'Search documentation',
              },
              modal: {
                displayDetails: 'Display detailed list',
                resetButtonTitle: 'Reset search',
                backButtonTitle: 'Close search',
                noResultsText: 'No results for',
                footer: {
                  selectText: 'Select',
                  selectKeyAriaLabel: 'Enter',
                  navigateText: 'Navigate',
                  navigateUpKeyAriaLabel: 'Arrow up',
                  navigateDownKeyAriaLabel: 'Arrow down',
                  closeText: 'Close',
                  closeKeyAriaLabel: 'Esc',
                },
              },
            },
          },
        },
        translations: {
          button: {
            buttonText: '搜索',
            buttonAriaLabel: '搜索文档',
          },
          modal: {
            displayDetails: '显示详细结果',
            resetButtonTitle: '清除搜索',
            backButtonTitle: '关闭搜索',
            noResultsText: '没有找到匹配结果',
            footer: {
              selectText: '选择',
              selectKeyAriaLabel: 'Enter',
              navigateText: '切换',
              navigateUpKeyAriaLabel: '上箭头',
              navigateDownKeyAriaLabel: '下箭头',
              closeText: '关闭',
              closeKeyAriaLabel: 'Esc',
            },
          },
        },
      },
    },
    outline: { label: '本页内容', level: [2, 3] as [number, number] },
    docFooter: { prev: '上一页', next: '下一页' },
    lastUpdated: { text: '最近更新' },
    locales: {
      root: {
        label: '简体中文',
      },
      en: {
        label: 'English',
        nav: [
          { text: 'Get started', link: '/en/guide/getting-started' },
          { text: 'Modes', link: '/en/modes/' },
          { text: 'Configuration', link: '/en/reference/configuration' },
          { text: 'Development', link: '/en/development/architecture' },
          { text: 'Configuration & simulator', link: '/en/editor/' },
        ],
        sidebar: [
          {
            text: 'Get started',
            items: [
              { text: 'Getting started', link: '/en/guide/getting-started' },
              { text: 'macOS', link: '/en/guide/macos' },
            ],
          },
          {
            text: 'Modes',
            items: [
              { text: 'Overview', link: '/en/modes/' },
              { text: 'Normal', link: '/en/modes/normal' },
              { text: 'Grid', link: '/en/modes/grid' },
              { text: 'Recursive Grid', link: '/en/modes/recursive-grid' },
              { text: 'UI Hint', link: '/en/modes/ui-hint' },
            ],
          },
          windowSidebar('/en', true),
          {
            text: 'Reference',
            items: [
              { text: 'Configuration', link: '/en/reference/configuration' },
              { text: 'Modes and actions', link: '/en/reference/modes-and-actions' },
              { text: 'Release notes', link: '/en/releases/' },
            ],
          },
          developmentSidebar('/en', true),
          {
            text: 'Tools',
            items: [{ text: 'Configuration & simulator', link: '/en/editor/' }],
          },
        ],
        search: {
          provider: 'local',
          options: {
            translations: {
              button: {
                buttonText: 'Search',
                buttonAriaLabel: 'Search documentation',
              },
              modal: {
                displayDetails: 'Display detailed list',
                resetButtonTitle: 'Reset search',
                backButtonTitle: 'Close search',
                noResultsText: 'No results for',
                footer: {
                  selectText: 'Select',
                  selectKeyAriaLabel: 'Enter',
                  navigateText: 'Navigate',
                  navigateUpKeyAriaLabel: 'Arrow up',
                  navigateDownKeyAriaLabel: 'Arrow down',
                  closeText: 'Close',
                  closeKeyAriaLabel: 'Esc',
                },
              },
            },
          },
        },
        outline: { label: 'On this page', level: [2, 3] as [number, number] },
        docFooter: { prev: 'Previous page', next: 'Next page' },
        lastUpdated: { text: 'Last updated' },
      },
    },
  },
  }
}
