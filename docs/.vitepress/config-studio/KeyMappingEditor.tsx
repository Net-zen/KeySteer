import { computed, defineComponent, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useStudioI18n } from './i18n.ts'
import { KeyMappingRecorder, keyMappingCaption, keyMappingTarget, parseKeyMapping } from '../simulator/key-mapping.ts'
import type { ConfigDocument } from './document.ts'

const commonKeys = [
  ['home', 'Home'], ['end', 'End'], ['page_up', 'PgUp'], ['page_down', 'PgDn'],
  ['arrow_left', '←'], ['arrow_down', '↓'], ['arrow_up', '↑'], ['arrow_right', '→'],
  ['enter', 'Enter'], ['tab', 'Tab'], ['esc', 'Esc'], ['backspace', 'Backspace'], ['delete', 'Delete'], ['space', 'Space'],
]

export default defineComponent({
  name: 'KeyMappingEditor',
  props: {
    chord: { type: String, required: true },
    action: { type: String, required: true },
    document: { type: Object as () => ConfigDocument, required: true },
    isMac: Boolean,
  },
  emits: { apply: (_binding: string) => true },
  setup(props, { emit }) {
    const { t } = useStudioI18n()
    const target = ref('')
    const recording = ref(false)
    const draft = ref('')
    const error = ref('')
    const recorder = new KeyMappingRecorder()
    function cancel(): void { recording.value = false; draft.value = ''; recorder.reset() }
    watch(() => [props.chord, props.action, props.isMac, props.document], () => {
      cancel(); error.value = ''
      target.value = keyMappingTarget(props.action, props.document, props.isMac) ?? ''
    }, { immediate: true })
    const mapping = computed(() => parseKeyMapping(target.value, props.document, props.isMac))
    function apply(): void {
      if (props.chord.trim() && mapping.value) emit('apply', mapping.value.binding)
    }
    function keyDown(event: KeyboardEvent): void {
      if (!recording.value) return
      event.preventDefault(); event.stopPropagation()
      if (event.key === 'Escape') { cancel(); error.value = ''; return }
      const chord = recorder.press(event)
      draft.value = chord ?? ''
      error.value = chord ? '' : '暂不支持这个按键，请重试或选择下方常用键。'
    }
    function keyUp(event: KeyboardEvent): void {
      if (!recording.value) return
      event.preventDefault(); event.stopPropagation()
      const chord = recorder.release(event)
      if (!chord) return
      if (!parseKeyMapping(chord, props.document, props.isMac)) {
        draft.value = ''; error.value = '请再加一个普通按键，例如 Ctrl + C。'; return
      }
      target.value = chord
      cancel(); error.value = ''; apply()
    }
    function visibilityChanged(): void { if (globalThis.document.hidden) cancel() }
    const presets = computed(() => [
      ...commonKeys,
      ...['a', 'c', 'v', 'z'].map(key => {
        const target = `${props.isMac ? 'win' : 'ctrl'}+${key}`
        return [target, keyMappingCaption(target, props.isMac)]
      }),
    ])
    onMounted(() => {
      window.addEventListener('blur', cancel)
      globalThis.document.addEventListener('visibilitychange', visibilityChanged)
    })
    onBeforeUnmount(() => {
      window.removeEventListener('blur', cancel)
      globalThis.document.removeEventListener('visibilitychange', visibilityChanged)
      cancel()
    })
    return () => <div class="ks-key-mapping">
      <button type="button" class="ks-key-recorder" aria-label={t('录制目标按键')} aria-pressed={recording.value}
        aria-describedby="ks-mapping-hint" disabled={!props.chord.trim()} onBlur={cancel}
        onClick={event => {
          if (recording.value) { cancel(); return }
          recorder.reset(); draft.value = ''; error.value = ''; recording.value = true
          ;(event.currentTarget as HTMLButtonElement).focus({ preventScroll: true })
        }} onKeydown={keyDown} onKeyup={keyUp}>
        <span class="ks-recording-dot" aria-hidden="true" />
        <span class="ks-recorder-value">{recording.value
          ? draft.value ? keyMappingCaption(draft.value, props.isMac) : t('请按下目标按键…')
          : target.value ? keyMappingCaption(target.value, props.isMac) : t('点击录制按键')}</span>
        <small>{recording.value ? t('录制中 · 点击取消') : t('点击录制')}</small>
      </button>
      <div class="ks-key-mapping-presets" role="group" aria-label={t('常用目标键')}>
        {presets.value.map(([value, label]) => <button type="button" aria-label={value} aria-pressed={mapping.value?.target === value} disabled={!props.chord.trim()}
          onClick={() => { cancel(); error.value = ''; target.value = value; apply() }}>{label}</button>)}
      </div>
      <p id="ks-mapping-hint" class={{ 'ks-mapping-error': !!error.value }} role={error.value ? 'alert' : undefined}>
        {error.value ? t(error.value) : !props.chord.trim() ? t('先在上方选择要修改的按键。')
          : recording.value ? t('按下单键或组合键，松开后自动应用。Esc 取消录制。') : t('点击录制后直接按键；被浏览器拦截的快捷键可在下方点选。')}
      </p>
      <p class="ks-key-mapping-note">{t('程序会将目标按键发送给当前应用；网页预览仅显示发送结果。')}</p>
    </div>
  },
})
