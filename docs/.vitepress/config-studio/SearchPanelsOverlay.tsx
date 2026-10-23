import { defineComponent } from 'vue'
import type { ConfigDocument } from './document'
import { useStudioI18n } from './i18n'
import { searchPanelLayout, searchPanelSettings, type PanelRect, type SearchPanelBlock } from '../simulator/search-panel-position.ts'
import { searchPanelColors } from '../simulator/search-input-style.ts'
import { PREVIEW_SCREEN, PREVIEW_WINDOW } from './preview-geometry.ts'

export default defineComponent({
  props: {
    document: { type: Object as () => ConfigDocument, required: true },
    appearance: { type: String, required: true },
  },
  setup(props) {
    const { t } = useStudioI18n()
    return () => {
      const layout = searchPanelLayout(props.document, PREVIEW_SCREEN, PREVIEW_WINDOW)
      const inputUi = searchPanelSettings(props.document, 'search_input_ui')
      const infoUi = searchPanelSettings(props.document, 'search_info_ui')
      const inputColors = searchPanelColors(props.document, props.appearance)
      const infoColors = searchPanelColors(props.document, props.appearance, 'search_info_ui')
      const infoPadding = infoUi.padding_y >= 0 ? infoUi.padding_y : Math.round(infoUi.font_size * .2)
      const infoPaddingX = Math.max(10, infoUi.padding_x >= 0 ? infoUi.padding_x : Math.round(infoUi.font_size * .4))
      const line = infoUi.font_size * 1.8, gap = infoUi.font_size * .8, textGap = infoUi.font_size * .5
      const columnWidth = Math.max(1, (layout.info.width - infoPaddingX * 2 - gap) / 2)
      const panel = (block: SearchPanelBlock, rect: PanelRect) => {
        const ui = searchPanelSettings(props.document, block)
        const colors = block === 'search_info_ui' ? infoColors : inputColors
        return { x: rect.x, y: rect.y, width: rect.width, height: rect.height, rx: ui.border_radius >= 0 ? ui.border_radius : Math.round(ui.font_size * .35),
          fill: colors.background_color, stroke: colors.border_color, 'stroke-width': Math.max(0, ui.border_width ?? 1) }
      }
      return <svg class="ks-search-panels-overlay" viewBox={`0 0 ${PREVIEW_SCREEN.width} ${PREVIEW_SCREEN.height}`} role="img" aria-label={t('搜索框与信息面板布局预览')}>
          <g data-search-panel="search_info_ui">
            <rect {...panel('search_info_ui', layout.info)} />
            {[['1 OCR', 'KeySteer'], ['2 Accessibility', 'Button · Open'], ['3 Coordinates', '420, 280'], ['4 Color', '#49A98A']].map(([title, value], index) => {
              const x = layout.info.x + infoPaddingX + index % 2 * (columnWidth + gap)
              const y = layout.info.y + infoPadding + line / 2 + Math.floor(index / 2) * (line * 2 + textGap + gap)
              return <text x={x} y={y} dominant-baseline="central" fill={infoColors.text_color} font-size={infoUi.font_size} font-family={infoUi.font_family || 'inherit'}>
                <tspan x={x}>{title}</tspan><tspan x={x} dy={line + textGap}>{value}</tspan>
              </text>
            })}
          </g>
          <g data-search-panel="search_input_ui">
            <rect {...panel('search_input_ui', layout.input)} />
            <text x={layout.input.x + (inputUi.padding_x >= 0 ? inputUi.padding_x : Math.round(inputUi.font_size * .4)) + Math.max(0, inputUi.border_width ?? 1)}
              y={layout.input.y + layout.input.height / 2} dominant-baseline="central" font-size={inputUi.font_size} font-family={inputUi.font_family || 'inherit'}
              fill={inputColors.text_color}>{t('搜索输入')} · ajh akl</text>
          </g>
        </svg>
    }
  },
})
