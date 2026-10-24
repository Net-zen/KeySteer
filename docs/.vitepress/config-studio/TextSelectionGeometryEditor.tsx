import { computed, defineComponent, ref } from 'vue'
import { useStudioI18n } from './i18n'
import type { ConfigDocument } from './document'
import {
  clampSelectionPair, geometryAspect, selectionGeometryViewport, selectionPairAtPoint,
  selectionPointInViewport, selectionSliderBounds, textRegion,
  type GeometryViewport, type Pair, type SelectionGeometryField,
} from './text-selection-geometry.ts'

export default defineComponent({
  name: 'TextSelectionGeometryEditor',
  props: {
    document: { type: Object as () => ConfigDocument, required: true },
    appearance: { type: String, required: true },
    inheritedPadding: { type: Boolean, required: true },
    inheritedPointer: { type: Boolean, required: true },
  },
  emits: {
    change: (_field: SelectionGeometryField, _value: Pair) => true,
    reset: (_field: SelectionGeometryField) => true,
  },
  setup(props, { emit }) {
    const { t } = useStudioI18n()
    const active = ref<SelectionGeometryField>('padding')
    const gesture = ref<{ pointer: number; field: SelectionGeometryField; previous: Pair; inherited: boolean; viewport: GeometryViewport }>()
    const ui = computed(() => props.document.text_select?.ui ?? {})
    const padding = computed<Pair>(() => ui.value.padding ?? [2, 2])
    const pointer = computed<Pair>(() => ui.value.pointer_offset ?? [12, 12])
    const pair = (field: SelectionGeometryField) => field === 'padding' ? padding.value : pointer.value
    const viewport = computed(() => gesture.value?.viewport ?? selectionGeometryViewport(padding.value, pointer.value))
    const color = computed(() => {
      const configured = ui.value.border_color
      return (typeof configured === 'string' ? configured : configured?.[props.appearance])
        ?? props.document.theme?.[props.appearance]?.accent ?? '#4965D9FF'
    })
    const change = (field: SelectionGeometryField, value: Pair) => emit('change', field, clampSelectionPair(field, value))
    const draw = (event: PointerEvent) => {
      if (!gesture.value || event.pointerId !== gesture.value.pointer) return
      const bounds = (event.currentTarget as SVGSVGElement).getBoundingClientRect()
      if (!bounds.width || !bounds.height) return
      change(gesture.value.field, selectionPairAtPoint(gesture.value.field, selectionPointInViewport(
        [(event.clientX - bounds.left) / bounds.width, (event.clientY - bounds.top) / bounds.height], gesture.value.viewport,
      )))
    }
    const cancel = () => {
      if (gesture.value) {
        if (gesture.value.inherited) emit('reset', gesture.value.field)
        else emit('change', gesture.value.field, gesture.value.previous)
      }
      gesture.value = undefined
    }
    const names: Record<SelectionGeometryField, string> = { padding: '包围框边距', pointer_offset: '鼠标落点' }
    return () => {
      const view = viewport.value, current = pair(active.value)
      const [min, max] = selectionSliderBounds(active.value, current)
      const x = textRegion.width + pointer.value[0], y = textRegion.height + pointer.value[1]
      const theme = props.document.theme?.[props.appearance] ?? {}
      const background = theme.surface ?? (props.appearance === 'dark' ? '#0A1338FF' : '#EEF2FFFF')
      return <div class="ks-text-geometry-editor ks-position-editor">
        <div class="ks-text-geometry-fields">
          {(['padding', 'pointer_offset'] as const).map(field => {
            const value = pair(field), inherited = field === 'padding' ? props.inheritedPadding : props.inheritedPointer
            return <div class="ks-text-geometry-field" data-config-path={`text_select.ui.${field}`}>
              <button type="button" aria-pressed={active.value === field} onClick={() => { cancel(); active.value = field }}>
                {t(names[field])}<output>X {value[0]} · Y {value[1]}</output>
              </button>
              <button type="button" disabled={inherited} aria-label={t(field === 'padding' ? '重置包围框边距' : '重置鼠标偏移')}
                onClick={() => { cancel(); emit('reset', field) }}>{t(inherited ? '默认' : '重置')}</button>
            </div>
          })}
        </div>
        <p>{t(active.value === 'padding' ? '拖动包围框右下角的方块，调整左右和上下边距。' : '点击或拖动鼠标落点，相对原始识别框右下角定位。')}</p>
        <svg class={{ 'ks-text-geometry-canvas': true, dragging: !!gesture.value }}
          viewBox={`${view.x} ${view.y} ${view.width} ${view.height}`} style={{ aspectRatio: String(geometryAspect), background,
            color: theme.text ?? (props.appearance === 'dark' ? '#E8EEFFFF' : '#17327AFF'), '--ks-geometry-background': background }}
          tabindex="0" role="group" aria-label={t(active.value === 'padding' ? '拖动调整包围框边距' : '拖动调整鼠标落点')}
          onPointerdown={event => {
            if (!event.isPrimary || event.button !== 0 || gesture.value) return
            event.preventDefault()
            gesture.value = { pointer: event.pointerId, field: active.value, previous: [...pair(active.value)],
              inherited: active.value === 'padding' ? props.inheritedPadding : props.inheritedPointer, viewport: { ...viewport.value } }
            const element = event.currentTarget as SVGSVGElement
            element.focus(); element.setPointerCapture(event.pointerId); draw(event)
          }}
          onPointermove={draw}
          onPointerup={event => { if (event.pointerId === gesture.value?.pointer) { draw(event); gesture.value = undefined } }}
          onPointercancel={cancel} onLostpointercapture={cancel}
          onKeydown={event => {
            if (event.key === 'Escape' && gesture.value) { event.preventDefault(); event.stopPropagation(); cancel(); return }
            if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) return
            event.preventDefault(); event.stopPropagation()
            const next: Pair = [...current], axis = ['ArrowLeft', 'ArrowRight'].includes(event.key) ? 0 : 1
            next[axis] += (['ArrowLeft', 'ArrowUp'].includes(event.key) ? -1 : 1) * (event.shiftKey ? 10 : 1)
            change(active.value, next)
          }}>
          <rect class="ks-text-geometry-original" width={textRegion.width} height={textRegion.height} rx="3" />
          <text x={textRegion.width / 2} y={textRegion.height / 2} text-anchor="middle" dominant-baseline="middle" class="ks-text-geometry-sample">KeySteer · Text Select</text>
          <text x="0" y="-13" class="ks-text-geometry-caption">{t('原始识别区域')}</text>
          <rect data-selection-outline x={-padding.value[0]} y={-padding.value[1]}
            width={textRegion.width + 2 * padding.value[0]} height={textRegion.height + 2 * padding.value[1]}
            fill="none" stroke={color.value} stroke-width={ui.value.border_width ?? 1.5} />
          <path d={`M ${textRegion.width} ${textRegion.height} H ${x} V ${y}`} class="ks-text-geometry-guide" />
          <circle cx={textRegion.width} cy={textRegion.height} r="3" class="ks-text-geometry-origin" />
          <g transform={`translate(${x} ${y})`} data-pointer-hotspot>
            <path d="M 0 0 L 0 22 L 6 16 L 11 27 L 16 25 L 11 14 L 20 14 Z" class="ks-text-geometry-cursor" />
            {active.value === 'pointer_offset' && <circle r="8" fill="none" stroke={color.value} stroke-width="2" />}
          </g>
          {active.value === 'padding' && <rect data-padding-handle x={textRegion.width + padding.value[0] - 5}
            y={textRegion.height + padding.value[1] - 5} width="10" height="10" rx="2" fill={color.value} class="ks-text-geometry-handle" />}
        </svg>
        <p class="ks-text-geometry-help">{t('方向键微调，Shift 加速；拖动时按 Esc 撤销。')}</p>
        <div class="ks-position-sliders">
          {(['水平', '垂直'] as const).map((name, axis) => <label>
            <span>{t(name)} {axis === 0 ? 'X' : 'Y'} <output>{current[axis]}</output></span>
            <input type="range" aria-label={t('{0} · {1}', [t(names[active.value]), t(name)])} min={min} max={max} step="1" value={current[axis]}
              onInput={event => { const next: Pair = [...current]; next[axis] = Number((event.target as HTMLInputElement).value); change(active.value, next) }} />
          </label>)}
        </div>
      </div>
    }
  },
})
