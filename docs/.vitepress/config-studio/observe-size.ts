/** Observe content-box sizes, with a window-resize fallback for older browsers. */
export function observeSize(elements: HTMLElement[], changed: (element: HTMLElement, size: { width: number; height: number }) => void): () => void {
  const measure = () => {
    for (const element of elements) {
      const style = getComputedStyle(element)
      const pixels = (value: string) => parseFloat(value) || 0
      changed(element, {
        width: element.clientWidth - pixels(style.paddingLeft) - pixels(style.paddingRight),
        height: element.clientHeight - pixels(style.paddingTop) - pixels(style.paddingBottom),
      })
    }
  }
  measure()
  if (typeof ResizeObserver !== 'undefined') {
    const observer = new ResizeObserver(entries => {
      for (const entry of entries) changed(entry.target as HTMLElement, entry.contentRect)
    })
    for (const element of elements) observer.observe(element)
    return () => observer.disconnect()
  }
  window.addEventListener('resize', measure)
  return () => window.removeEventListener('resize', measure)
}
