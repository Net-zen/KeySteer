import { computed, defineComponent, ref, useId, type PropType } from 'vue'
import { useStudioI18n } from './i18n'

interface MouseButton {
  key: string
  label: string
}
interface BindingInfo { text: string; tone: string }

const buttons = [
  { key: 'mouse_left', label: '左键', shape: 'M66 103V80C66 48 87 28 117 26V120H80Q66 120 66 103Z', x: 91, y: 81 },
  { key: 'mouse_right', label: '右键', shape: 'M129 26C159 28 180 48 180 80V103Q180 120 166 120H129Z', x: 155, y: 81 },
  { key: 'mouse_middle', label: '中键', shape: 'M116 49Q116 43 123 43T130 49V87Q130 93 123 93T116 87Z', x: 123, y: 106 },
  { key: 'mouse_x1', label: '侧键 1', shape: 'M46 154Q46 148 51 148T56 154V180Q56 186 51 186T46 180Z', x: 29, y: 171 },
  { key: 'mouse_x2', label: '侧键 2', shape: 'M46 109Q46 103 51 103T56 109V135Q56 141 51 141T46 135Z', x: 29, y: 126 },
]

/** Emits binding keys and click actions; configuration and drag state stay in the editor. */
export default defineComponent({
  name: 'MouseDiagram',
  props: {
    selected: { type: String, default: '' },
    draggedAction: { type: String, default: '' },
    bindingInfo: { type: Function as PropType<(button: MouseButton) => BindingInfo | undefined>, required: true },
  },
  emits: {
    select: (_button: MouseButton, _event: MouseEvent | KeyboardEvent) => true,
    actionStart: (_event: PointerEvent, _action: string) => true,
    actionMove: (_event: PointerEvent) => true,
    actionEnd: (_event: PointerEvent) => true,
    actionCancel: () => true,
  },
  setup(props, { emit }) {
    const { t } = useStudioI18n()
    const id = useId()
    const hovered = ref('')
    const selectedKey = computed(() => props.selected.split('+').at(-1))
    const active = computed(() => buttons.find(button => button.key === (hovered.value || selectedKey.value)))
    const activeBinding = computed(() => active.value && props.bindingInfo(active.value))
    return () => <div class="ks-mouse-map">
      <svg class="ks-mouse-diagram" viewBox="0 0 220 270" role="group" aria-label={t('鼠标按键')}>
        <defs>
          <linearGradient id={`${id}-shell`} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stop-color="var(--vp-c-bg)" />
            <stop offset="1" stop-color="var(--vp-c-bg-alt)" />
          </linearGradient>
          <linearGradient id={`${id}-button`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stop-color="var(--vp-c-bg)" />
            <stop offset="1" stop-color="var(--vp-c-bg-soft)" />
          </linearGradient>
        </defs>
        <ellipse class="ks-mouse-shadow" cx="123" cy="249" rx="62" ry="8" />
        <path class="ks-mouse-shell" fill={`url(#${id}-shell)`}
          d="M123 19C164 19 189 46 189 91V178C189 224 164 250 123 250S57 224 57 178V91C57 46 82 19 123 19Z" />
        <path class="ks-mouse-seam" d="M66 130H180M123 134V158" />
        <path class="ks-mouse-palm" d="M80 195C84 217 99 230 123 230S162 217 166 195" />
        <circle class="ks-mouse-mark" cx="123" cy="192" r="4" />
        {buttons.map(button => {
          const binding = props.bindingInfo(button)
          const selected = selectedKey.value === button.key
          return <g key={button.key} role="button" tabindex="0" aria-label={t(button.label)} aria-pressed={selected}
            class={{ 'ks-mouse-key': true, selected, dragging: props.draggedAction === button.key, bound: !!binding, [`tone-${binding?.tone ?? 'none'}`]: !!binding }}
            data-map-key={button.key} onClick={event => emit('select', button, event)}
            onPointerdown={event => emit('actionStart', event, button.key)}
            onPointermove={event => emit('actionMove', event)} onPointerup={event => emit('actionEnd', event)}
            onPointercancel={() => emit('actionCancel')} onLostpointercapture={() => emit('actionCancel')}
            onKeydown={event => {
              if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); emit('select', button, event) }
            }}
            onMouseenter={() => { hovered.value = button.key }} onMouseleave={() => { hovered.value = '' }}
            onFocus={() => { hovered.value = button.key }} onBlur={() => { hovered.value = '' }}>
            <title>{`${t(button.label)} · ${button.key}${binding ? ` · ${binding.text}` : ''}`}</title>
            {button.key.startsWith('mouse_x') && <rect class="ks-mouse-hit" x="13" y={button.key === 'mouse_x1' ? 145 : 100} width="45" height="44" rx="8" />}
            <path class="ks-mouse-plate" fill={`url(#${id}-button)`} d={button.shape} />
            {button.key === 'mouse_middle' && <path class="ks-mouse-wheel" d="M119 53H127M119 59H127M119 65H127M119 71H127M119 77H127M119 83H127" />}
            <text x={button.x} y={button.y} text-anchor="middle">{button.key === 'mouse_x1' ? '4' : button.key === 'mouse_x2' ? '5' : t(button.label)}</text>
            {binding && button.key !== 'mouse_middle' && !button.key.startsWith('mouse_x') && <circle class="ks-mouse-binding-dot" cx={button.x} cy="96" r="2.5" />}
          </g>
        })}
      </svg>
      <div class="ks-mouse-caption" aria-live="polite">
        <strong>{active.value ? t(active.value.label) : t('鼠标按键')}</strong>
        <span>{active.value ? activeBinding.value?.text || t('未绑定') : t('点击编辑 · 拖到键盘绑定点击')}</span>
        <small>{t('4 · 侧键 1　5 · 侧键 2')}</small>
      </div>
    </div>
  },
})
