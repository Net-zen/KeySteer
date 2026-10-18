import { defineComponent, ref } from 'vue'
import { useData } from 'vitepress'
import measurements from '../data/performance.json'
import './performance.css'

type Scenario = keyof typeof measurements.scenarios
type Latency = 'overall_ms' | 'first_ms' | 'repeat_ms'
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

export const PerformanceHighlights = defineComponent({
  name: 'PerformanceHighlights',
  setup() {
    const { lang } = useData()
    return () => {
      const en = lang.value === 'en-US'
      const response = actions.map(key => measurements.scenarios[key].overall_ms)
      const cards = [
        { value: `${(measurements.scenarios.hint_all.overall_ms / 1000).toFixed(2)} s`,
          title: pick(en, ['双屏首批标签', 'First labels on both displays']),
          detail: pick(en, ['双 4K · 800 控件 · Hybrid', 'Two 4K displays · 800 controls · Hybrid']) },
        { value: `${Math.round(Math.min(...response))}–${Math.round(Math.max(...response))} ms`,
          title: pick(en, ['日常操作响应', 'Everyday response']),
          detail: pick(en, ['滚动 · 网格定位 · 窗口管理', 'Scroll · Grid · Window controls']) },
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
      const chart = (title: string, rows: Scenario[], maximum: number) => <div class="ks-perf-chart">
        <div class="ks-perf-chart-heading"><strong>{title}</strong><span>{pick(en, ['毫秒 · 越低越快', 'ms · lower is faster'])}</span></div>
        <div class="ks-perf-bars">
          {rows.map(key => {
            const value = measurements.scenarios[key][metric.value]
            return <div key={key} class="ks-perf-row">
              <span class="ks-perf-name">{pick(en, names[key])}</span>
              <div class="ks-perf-track" aria-hidden="true"><span class="ks-perf-bar" style={{ width: `${value / maximum * 100}%` }} /></div>
              <strong class="ks-perf-value">{number(value)}</strong>
            </div>
          })}
          <div class="ks-perf-axis" aria-hidden="true">{[0, 1, 2, 3, 4].map(tick => <span key={tick}>{tick * maximum / 4}</span>)}</div>
        </div>
      </div>
      return <div class="ks-perf-latency">
        <div class="ks-perf-switch" role="group" aria-label={pick(en, ['选择响应统计', 'Choose response statistic'])}>
          {options.map(([key, title]) => <button key={key} type="button" aria-pressed={metric.value === key} onClick={() => { metric.value = key }}>{title}</button>)}
        </div>
        <div aria-live="polite">
          {chart(pick(en, ['滚动、定位与窗口操作', 'Scrolling, targeting & window actions']), actions, 40)}
          {chart(pick(en, ['UI Hint 首批标签', 'UI Hint · first labels']), hints, 500)}
          <p class="ks-perf-note">{pick(en, ['鼠标位置更新：', 'Pointer position update: '])}
            <strong>{number(measurements.scenarios.move[metric.value])} ms</strong>
            {pick(en, ['（按系统坐标计时）', ' (system coordinates)'])}</p>
        </div>
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
        <div class="ks-perf-legend"><span><i class="ks-perf-dot" />{pick(en, ['操作后内存', 'Memory after action'])}</span><span><i class="ks-perf-dot peak" />{pick(en, ['最高占用', 'Highest recorded'])}</span></div>
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
        ['最慢', 'Slowest'], ['操作后', 'After'], ['最高占用', 'Highest recorded'],
      ] as [string, string][]
      return <div class="ks-perf-table" role="region" aria-label={pick(en, ['完整性能数据', 'Full performance measurements'])}>
        <table>
          <colgroup><col class="ks-perf-scenario-column" /><col span={6} /></colgroup>
          <thead>
            <tr><th scope="col" rowspan={2}>{pick(en, ['场景', 'Scenario'])}</th>
              <th scope="colgroup" colspan={4}>{pick(en, ['响应时间（毫秒）', 'Response time (ms)'])}</th>
              <th scope="colgroup" colspan={2}>{pick(en, ['内存（MiB）', 'Memory (MiB)'])}</th></tr>
            <tr>{columns.map(column => <th key={column[1]} scope="col">{pick(en, column)}</th>)}</tr>
          </thead>
          <tbody>{scenarios.map(key => {
            const row = measurements.scenarios[key]
            const memory = measurements.memory.scenarios[key]
            return <tr key={key}><th scope="row">{pick(en, names[key])}</th>
              {[row.first_ms, row.repeat_ms, row.overall_ms, row.maximum_ms, memory.after_mib, memory.sampled_peak_mib].map((value, index) => <td key={index}>{number(value)}</td>)}
            </tr>
          })}</tbody>
        </table>
      </div>
    }
  },
})
