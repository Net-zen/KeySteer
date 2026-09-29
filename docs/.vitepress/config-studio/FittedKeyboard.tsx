import { observeSize } from './observe-size'
import { defineComponent, onBeforeUnmount, onMounted, onUpdated, ref } from 'vue'

/** Fit the complete keyboard without clipping keys or adding a nested scroller. */
export default defineComponent({
  setup(_, { slots }) {
    const viewport = ref<HTMLElement>()
    const content = ref<HTMLElement>()
    const scale = ref(1)
    const height = ref(220)
    let stopObserving: (() => void) | undefined
    function measure() {
      if (!viewport.value || !content.value) return
      const width = content.value.scrollWidth
      if (!width || !viewport.value.clientWidth) return
      scale.value = Math.min(1, viewport.value.clientWidth / width)
      height.value = content.value.offsetHeight * scale.value
    }
    onMounted(() => {
      stopObserving = observeSize([viewport.value!, content.value!], measure)
    })
    onUpdated(measure)
    onBeforeUnmount(() => stopObserving?.())
    return () => <div ref={viewport} class="ks-keyboard-fit" style={{ height: `${height.value}px` }}>
      <div ref={content} class="ks-keyboard-fit-content" style={{ transform: `scale(${scale.value})` }}>{slots.default?.()}</div>
    </div>
  },
})
