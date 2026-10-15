import { computed, defineComponent, onBeforeUnmount, onMounted, ref } from 'vue'
import { useData, withBase } from 'vitepress'
import {
  type DOWNLOAD_TARGETS,
  type LatestRelease,
  RELEASES_URL,
} from '../latest-release'

declare const __KEYSTEER_LATEST_RELEASE__: LatestRelease
const latestRelease = __KEYSTEER_LATEST_RELEASE__

interface DownloadAsset {
  key: string
  platform: 'windows' | 'macos'
  platformLabel: string
  label: { zh: string; en: string }
  description: { zh: string; en: string }
  target: (typeof DOWNLOAD_TARGETS)[number]
  optional?: boolean
}

const ASSETS: DownloadAsset[] = [
  {
    key: 'windows-x64',
    platform: 'windows',
    platformLabel: 'Windows',
    label: { zh: '默认版', en: 'Default' },
    description: { zh: 'x64 · AVX2', en: 'x64 · AVX2' },
    target: 'x86_64-pc-windows-msvc',
  },
  {
    key: 'windows-arm64',
    platform: 'windows',
    platformLabel: 'Windows',
    label: { zh: 'ARM64', en: 'ARM64' },
    description: { zh: 'ARM 电脑', en: 'ARM PCs' },
    target: 'aarch64-pc-windows-msvc',
  },
  {
    key: 'macos-apple-silicon',
    platform: 'macos',
    platformLabel: 'macOS',
    label: { zh: 'Apple Silicon', en: 'Apple Silicon' },
    description: { zh: 'M 系列芯片', en: 'M-series chips' },
    target: 'aarch64-apple-darwin',
  },
  {
    key: 'macos-intel',
    platform: 'macos',
    platformLabel: 'macOS',
    label: { zh: 'Intel', en: 'Intel' },
    description: { zh: 'Intel Mac', en: 'Intel Mac' },
    target: 'x86_64-apple-darwin',
  },
  {
    key: 'windows-x64-compatible',
    platform: 'windows',
    platformLabel: 'Windows',
    label: { zh: '兼容版', en: 'Compatible' },
    description: { zh: 'x64 · 无需 AVX2', en: 'x64 · No AVX2' },
    target: 'x86_64-pc-windows-msvc-compatible',
    optional: true,
  },
  {
    key: 'windows-x64-avx512',
    platform: 'windows',
    platformLabel: 'Windows',
    label: { zh: 'AVX-512 版', en: 'AVX-512' },
    description: { zh: 'x64 · 需 AVX-512', en: 'x64 · Requires AVX-512' },
    target: 'x86_64-pc-windows-msvc-avx512',
    optional: true,
  },
]

function isAppleSilicon() {
  const userAgentData = (navigator as Navigator & {
    userAgentData?: { architecture?: string }
  }).userAgentData
  if (userAgentData?.architecture === 'arm') return true

  try {
    const canvas = document.createElement('canvas')
    const context = canvas.getContext('webgl')
    const debugInfo = context?.getExtension('WEBGL_debug_renderer_info')
    const renderer = debugInfo
      ? context?.getParameter(debugInfo.UNMASKED_RENDERER_WEBGL) as string
      : ''
    return /Apple GPU|Apple M[1-9]/i.test(renderer)
  } catch {
    return false
  }
}

function detectAsset(): DownloadAsset {
  if (typeof navigator === 'undefined') return ASSETS[0]

  const userAgent = navigator.userAgent
  const userAgentData = (navigator as Navigator & {
    userAgentData?: { architecture?: string }
  }).userAgentData

  if (/Windows/i.test(userAgent)) {
    const isArm = /ARM64|AARCH64/i.test(userAgent) || userAgentData?.architecture === 'arm'
    return ASSETS[isArm ? 1 : 0]
  }

  if (/Macintosh|Mac OS X/i.test(userAgent)) {
    return ASSETS[isAppleSilicon() ? 2 : 3]
  }

  return ASSETS[0]
}

export default defineComponent({
  name: 'DownloadButton',
  setup() {
    const { lang } = useData()
    const detected = ref<DownloadAsset>(ASSETS[0])
    const menuOpen = ref(false)
    const isEnglish = () => lang.value === 'en-US'
    const text = (zh: string, en: string) => isEnglish() ? en : zh
    const localPath = (path: string) => withBase(`${isEnglish() ? '/en' : ''}${path}`)
    const downloadLabel = computed(() => text('立即下载', 'Download now'))
    const assetLabel = (asset: DownloadAsset) => asset.label[isEnglish() ? 'en' : 'zh']
    const assetUrl = (asset: DownloadAsset) => (
      latestRelease.assets[asset.target] ?? latestRelease.url
    )

    // Browser architecture detection does not prove support for an instruction set.
    // Keep advanced builds manual, and hide them until the selected release has them.
    const availableAssets = ASSETS.filter((asset) => !asset.optional || latestRelease.assets[asset.target])
    const compatibleUrl = latestRelease.assets['x86_64-pc-windows-msvc-compatible']

    const closeMenu = () => {
      menuOpen.value = false
    }

    onMounted(() => {
      detected.value = detectAsset()
      document.addEventListener('click', closeMenu)
    })

    onBeforeUnmount(() => {
      document.removeEventListener('click', closeMenu)
    })

    return () => (
      <div class="hero-download-section">
        <div class="hero-action-row">
          <div class="hero-download" onClick={(event) => event.stopPropagation()}>
            <a
              class="hero-download-main"
              href={assetUrl(detected.value)}
              title={text(`下载 KeySteer ${detected.value.platformLabel} ${assetLabel(detected.value)}`, `Download KeySteer for ${detected.value.platformLabel} (${assetLabel(detected.value)})`)}
            >
              <span class="hero-download-emoji" aria-hidden="true">💾</span>
              <span class="hero-download-copy">
                <strong>{downloadLabel.value}</strong>
                <small>{latestRelease.tag} · {assetLabel(detected.value)}{detected.value.key === 'windows-x64' && ' · AVX2'}</small>
              </span>
            </a>
            <button
              class="hero-download-toggle"
              type="button"
              aria-label={text('选择下载平台和架构', 'Choose download platform and architecture')}
              aria-expanded={menuOpen.value}
              onClick={() => { menuOpen.value = !menuOpen.value }}
            >
              <svg
                class={`hero-download-chevron ${menuOpen.value ? 'open' : ''}`}
                width="16"
                height="16"
                viewBox="0 0 16 16"
                fill="none"
                aria-hidden="true"
              >
                <path d="m4 6 4 4 4-4" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
              </svg>
            </button>
            {menuOpen.value && (
              <div class="hero-download-menu" role="menu">
                <div class="hero-download-group">
                  <div class="hero-download-group-title"><strong>Windows</strong></div>
                  {availableAssets.filter((asset) => asset.platform === 'windows').map((asset) => (
                    <a
                      key={asset.key}
                      class={`hero-download-option ${asset.key === detected.value.key ? 'detected' : ''}`}
                      href={assetUrl(asset)}
                      role="menuitem"
                      onClick={closeMenu}
                    >
                      <span class="hero-download-option-emoji" aria-hidden="true">📦</span>
                      <span class="hero-download-option-copy">
                        <strong>{assetLabel(asset)}</strong>
                        <small>{asset.description[isEnglish() ? 'en' : 'zh']}</small>
                      </span>
                      {asset.key === detected.value.key && <span class="hero-download-option-status">{text('当前设备', 'This device')}</span>}
                    </a>
                  ))}
                </div>
                <div class="hero-download-group">
                  <div class="hero-download-group-title"><strong>macOS</strong></div>
                  {availableAssets.filter((asset) => asset.platform === 'macos').map((asset) => (
                    <a
                      key={asset.key}
                      class={`hero-download-option ${asset.key === detected.value.key ? 'detected' : ''}`}
                      href={assetUrl(asset)}
                      role="menuitem"
                      onClick={closeMenu}
                    >
                      <span class="hero-download-option-emoji" aria-hidden="true">📦</span>
                      <span class="hero-download-option-copy">
                        <strong>{assetLabel(asset)}</strong>
                        <small>{asset.description[isEnglish() ? 'en' : 'zh']}</small>
                      </span>
                      {asset.key === detected.value.key && <span class="hero-download-option-status">{text('当前设备', 'This device')}</span>}
                    </a>
                  ))}
                </div>
                <div class="hero-download-help">
                  <svg width="16" height="16" viewBox="0 0 20 20" fill="none" aria-hidden="true">
                    <circle cx="10" cy="10" r="7.25" stroke="currentColor" stroke-width="1.5" />
                    <path d="M10 9v4m0-7v.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
                  </svg>
                  <div>
                    <p>{text('Windows 启动失败？试试', 'Windows: cannot start? Try ')}{compatibleUrl ? <a href={compatibleUrl} onClick={closeMenu}>{text('兼容版', 'Compatible')}</a> : text('兼容版', 'Compatible')}</p>
                    <p>{text('「检查更新」自动匹配合适版本', 'Check for Updates picks the best build.')}</p>
                  </div>
                </div>
                <a class="hero-download-all" href={RELEASES_URL} target="_blank" rel="noopener" onClick={closeMenu}>
                  {text('查看全部 Release →', 'View all releases →')}
                </a>
              </div>
            )}
          </div>
          <a class="hero-action-link" href={localPath('/guide/getting-started')}>{text('快速开始', 'Get started')}</a>
          <a class="hero-action-link" href={localPath(isEnglish() ? '/editor/' : '/development/architecture')}>
            {text('一起开发', 'Configuration & simulator')}
          </a>
        </div>
      </div>
    )
  },
})
