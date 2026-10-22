import { defineComponent, ref } from 'vue'
import { useData } from 'vitepress'
import measurements from '../data/performance.json'
import './performance.css'

type Scenario = keyof typeof measurements.scenarios
type Latency = 'overall_ms' | 'first_ms' | 'repeat_ms'
type CoreScenario = keyof typeof measurements.core_cpu.metrics
const names: Record<Scenario, [string, string]> = {
  move: ['鼠标移动', 'Pointer movement'],
  scroll: ['页面滚动', 'Scrolling'],
  grid: ['Grid 网格定位', 'Grid'],
  recursive: ['递归网格定位', 'Recursive Grid'],
  window: ['窗口移动', 'Window movement'],
  quick: ['快速分屏', 'Quick split'],
  hint_window: ['UI Hint · 单窗口', 'UI Hint · one window'],
  hint_all: ['UI Hint · 双屏', 'UI Hint · both displays'],
}
const actions: Scenario[] = ['scroll', 'grid', 'recursive', 'window', 'quick']
const hints: Scenario[] = ['hint_window', 'hint_all']
const scenarios = Object.keys(names) as Scenario[]
const number = (value: number) => value.toFixed(value < 1 ? 2 : 1)
const pick = (english: boolean, pair: [string, string]) => pair[english ? 1 : 0]
const cpuTime = (nanoseconds: number) => nanoseconds < 1000
  ? `${Math.round(nanoseconds)} ns`
  : `${(nanoseconds / 1000).toFixed(2)} μs`
const coreNames: Record<CoreScenario, [string, string]> = {
  runtime_default_keys: ['默认按键派发', 'Default key dispatch'],
  runtime_wasd_keys: ['自定义 WASD 按键派发', 'Custom WASD key dispatch'],
  normal_frame: ['持续移动帧计算', 'Continuous movement frame'],
  hint_24: ['24 个就绪目标生成标签', 'Labels for 24 ready targets'],
  hint_64: ['64 个就绪目标生成标签', 'Labels for 64 ready targets'],
  hint_100: ['100 个就绪目标生成标签', 'Labels for 100 ready targets'],
  hint_128: ['128 个就绪目标生成标签', 'Labels for 128 ready targets'],
  hint_500: ['500 个就绪目标生成标签', 'Labels for 500 ready targets'],
  hint_2000: ['2000 个就绪目标生成标签', 'Labels for 2000 ready targets'],
}

export const PerformanceHighlights = defineComponent({
  name: 'PerformanceHighlights',
  setup() {
    const { lang } = useData()
    return () => {
      const en = lang.value === 'en-US'
      const cards = [
        { value: cpuTime(measurements.core_cpu.metrics.runtime_default_keys.p50_ns),
          title: pick(en, ['按键派发（CPU）', 'Key dispatch (CPU)']),
          detail: pick(en, ['预热后 · 默认键位 · 每事件均摊', 'Warmed · default keys · per-event average']) },
        { value: cpuTime(measurements.core_cpu.metrics.normal_frame.p50_ns),
          title: pick(en, ['移动帧计算（CPU）', 'Movement frame (CPU)']),
          detail: pick(en, ['持续移动 · 每帧均摊', 'Continuous movement · per-frame average']) },
        { value: `${Math.round(measurements.memory.idle_mib)} MiB`,
          title: pick(en, ['启动待机内存', 'Startup idle memory']),
          detail: pick(en, ['按任务管理器显示统计', 'As shown in Task Manager']) },
      ]
      return <div class="ks-perf-highlights">
        {cards.map(card => <div key={card.title} class="ks-perf-highlight">
          <span>{card.title}</span><strong>{card.value}</strong><small>{card.detail}</small>
        </div>)}
      </div>
    }
  },
})

export const PerformanceCore = defineComponent({
  name: 'PerformanceCore',
  setup() {
    const { lang } = useData()
    return () => {
      const en = lang.value === 'en-US'
      return <div class="ks-perf-table" role="region" aria-label={pick(en, ['核心 CPU 处理耗时', 'Core CPU processing time'])}>
        <table>
          <colgroup><col class="ks-perf-scenario-column" /><col span={3} /></colgroup>
          <thead><tr><th scope="col">{pick(en, ['核心路径', 'Core path'])}</th>
            <th scope="col">p50</th><th scope="col">p95</th><th scope="col">p99</th></tr></thead>
          <tbody>{(Object.keys(coreNames) as CoreScenario[]).map(key => {
            const row = measurements.core_cpu.metrics[key]
            return <tr key={key}><th scope="row">{pick(en, coreNames[key])}</th>
              {[row.p50_ns, row.p95_ns, row.p99_ns].map((value, index) => <td key={index}>{cpuTime(value)}</td>)}
            </tr>
          })}</tbody>
        </table>
      </div>
    }
  },
})

export const PerformanceLatency = defineComponent({
  name: 'PerformanceLatency',
  setup() {
    const { lang } = useData()
    const metric = ref<Latency>('overall_ms')
    return () => {
      const en = lang.value === 'en-US'
      const options: [Latency, string][] = [
        ['overall_ms', pick(en, ['全部操作', 'All operations'])],
        ['first_ms', pick(en, ['首次操作', 'First operation'])],
        ['repeat_ms', pick(en, ['重复操作', 'Repeat operations'])],
      ]
      const chart = (title: string, rows: Scenario[], unit: 'μs' | 'ms', step: number) => {
        const scale = unit === 'μs' ? 1000 : 1
        const largest = Math.max(...rows.flatMap(key => (['overall_ms', 'first_ms', 'repeat_ms'] as Latency[])
          .map(statistic => measurements.scenarios[key][statistic] * scale)))
        const maximum = Math.max(step, Math.ceil(largest / step) * step)
        return <div class="ks-perf-chart">
          <div class="ks-perf-chart-heading"><strong>{title}</strong><span>{unit} · {pick(en, ['越低越快', 'lower is faster'])}</span></div>
          <div class="ks-perf-bars">
            {rows.map(key => {
              const value = measurements.scenarios[key][metric.value] * scale
              return <div key={key} class="ks-perf-row">
                <span class="ks-perf-name">{pick(en, names[key])}</span>
                <div class="ks-perf-track" aria-hidden="true"><span class="ks-perf-bar" style={{ width: `${value / maximum * 100}%` }} /></div>
                <strong class="ks-perf-value">{number(value)}</strong>
              </div>
            })}
            <div class="ks-perf-axis" aria-hidden="true">{[0, 1, 2, 3, 4].map(tick => <span key={tick}>{tick * maximum / 4}</span>)}</div>
          </div>
        </div>
      }
      return <div class="ks-perf-latency">
        <div class="ks-perf-switch" role="group" aria-label={pick(en, ['选择观测统计', 'Choose observation statistic'])}>
          {options.map(([key, title]) => <button key={key} type="button" aria-pressed={metric.value === key} onClick={() => { metric.value = key }}>{title}</button>)}
        </div>
        <div aria-live="polite">
          {chart(pick(en, ['输入处理与原生请求提交', 'Input handling & native request submission']), actions, 'μs', 100)}
          {chart(pick(en, ['UI Hint 首批标签场景提交', 'UI Hint · first label scene commits']), hints, 'ms', 50)}
          <p class="ks-perf-note">{pick(en, ['鼠标位置更新：', 'Pointer position update: '])}
            <strong>{Math.round(measurements.scenarios.move.effect_observed_ms * 1000)} μs</strong>
            {pick(en, ['（系统坐标观测中位数，含轮询间隔）', ' (median system-coordinate observation, including polling interval)'])}</p>
        </div>
      </div>
    }
  },
})

export const PerformanceRecognition = defineComponent({
  name: 'PerformanceRecognition',
  setup() {
    const { lang } = useData()
    return () => {
      const en = lang.value === 'en-US'
      const columns: [string, string][] = [
        ['场景', 'Scenario'], ['主屏首批', 'Primary first labels'],
        ['副屏首批', 'Secondary first labels'], ['全部测试屏首批', 'First labels on all tested displays'],
        ['扫描结果终态', 'Scan terminal result'], ['按钮校验', 'Verified buttons'],
      ]
      return <div class="ks-perf-table ks-perf-recognition" role="region" aria-label={pick(en, ['真实 UI Hint 识别阶段（毫秒）', 'Real UI Hint recognition stages (ms)'])}>
        <table>
          <thead><tr>{columns.map(column => <th key={column[1]} scope="col">{pick(en, column)}</th>)}</tr></thead>
          <tbody>{hints.map(key => {
            const row = measurements.scenarios[key as 'hint_window' | 'hint_all']
            return <tr key={key}><th scope="row">{pick(en, names[key])}</th>
              <td>{number(row.primary_first_labels_ms)}</td>
              <td>{row.secondary_first_labels_ms === null ? '—' : number(row.secondary_first_labels_ms)}</td>
              <td>{number(row.overall_ms)}</td><td>{number(row.scan_complete_ms)}</td>
              <td>{row.verified_buttons}</td>
            </tr>
          })}</tbody>
        </table>
      </div>
    }
  },
})

export const PerformanceMemory = defineComponent({
  name: 'PerformanceMemory',
  setup() {
    const { lang } = useData()
    return () => {
      const en = lang.value === 'en-US'
      return <div class="ks-perf-chart">
        <div class="ks-perf-chart-heading"><strong>{pick(en, ['各场景内存', 'Memory by scenario'])}</strong><span>MiB</span></div>
        <div class="ks-perf-legend"><span><i class="ks-perf-dot" />{pick(en, ['操作后内存', 'Memory after action'])}</span><span><i class="ks-perf-dot peak" />{pick(en, ['采样峰值', 'Sampled peak'])}</span></div>
        <div class="ks-perf-bars memory">
          {scenarios.map(key => {
            const row = measurements.memory.scenarios[key]
            return <div key={key} class="ks-perf-row">
              <span class="ks-perf-name">{pick(en, names[key])}</span>
              <div class="ks-perf-memory-tracks" aria-hidden="true">
                <div class="ks-perf-track"><span class="ks-perf-bar" style={{ width: `${row.after_mib / 200 * 100}%` }} /></div>
                <div class="ks-perf-track"><span class="ks-perf-bar peak" style={{ width: `${row.sampled_peak_mib / 200 * 100}%` }} /></div>
              </div>
              <span class="ks-perf-value"><strong>{number(row.after_mib)}</strong><small>{number(row.sampled_peak_mib)}</small></span>
            </div>
          })}
          <div class="ks-perf-axis" aria-hidden="true">{[0, 50, 100, 150, 200].map(tick => <span key={tick}>{tick}</span>)}</div>
        </div>
        <p class="ks-perf-note">{pick(en, ['启动待机 ', 'Startup idle '])}<strong>{number(measurements.memory.idle_mib)} MiB</strong>
          {' · '}{pick(en, ['使用后待机 ', 'Idle after use '])}<strong>{number(measurements.memory.idle_after_mib)} MiB</strong></p>
      </div>
    }
  },
})

export const PerformanceDetails = defineComponent({
  name: 'PerformanceDetails',
  setup() {
    const { lang } = useData()
    return () => {
      const en = lang.value === 'en-US'
      const columns = [
        ['首次', 'First'], ['重复', 'Repeat'], ['典型', 'Typical'],
        ['最慢', 'Slowest'], ['操作后', 'After'], ['采样峰值', 'Sampled peak'],
      ] as [string, string][]
      return <div class="ks-perf-table" role="region" aria-label={pick(en, ['完整性能数据', 'Full performance measurements'])}>
        <table>
          <colgroup><col class="ks-perf-scenario-column" /><col span={6} /></colgroup>
          <thead>
            <tr><th scope="col" rowspan={2}>{pick(en, ['场景', 'Scenario'])}</th>
              <th scope="colgroup" colspan={4}>{pick(en, ['阶段耗时（单位见场景）', 'Stage time (unit beside scenario)'])}</th>
              <th scope="colgroup" colspan={2}>{pick(en, ['内存（MiB）', 'Memory (MiB)'])}</th></tr>
            <tr>{columns.map(column => <th key={column[1]} scope="col">{pick(en, column)}</th>)}</tr>
          </thead>
          <tbody>{scenarios.map(key => {
            const row = measurements.scenarios[key]
            const memory = measurements.memory.scenarios[key]
            const hint = hints.includes(key)
            const scale = hint ? 1 : 1000
            return <tr key={key}><th scope="row">{pick(en, names[key])} ({hint ? 'ms' : 'μs'})</th>
              {[row.first_ms, row.repeat_ms, row.overall_ms, row.maximum_ms].map((value, index) => <td key={index}>{number(value * scale)}</td>)}
              <td>{number(memory.after_mib)}</td><td>{number(memory.sampled_peak_mib)}</td>
            </tr>
          })}</tbody>
        </table>
      </div>
    }
  },
})
