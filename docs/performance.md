---
title: 性能
description: KeySteer 0.12.3 的实际响应延迟与内存占用，覆盖日常操控、单窗口和双屏 UI Hint 识别。
---

<script setup>
import { PerformanceHighlights, PerformanceCore, PerformanceLatency, PerformanceRecognition, PerformanceMemory, PerformanceDetails } from './.vitepress/components/Performance'
import measurements from './.vitepress/data/performance.json'
const { memory, scenarios } = measurements
const dailyMemory = ['move', 'scroll', 'grid', 'recursive', 'window', 'quick'].map(key => memory.scenarios[key].after_mib)
const hintMemory = ['hint_window', 'hint_all'].map(key => memory.scenarios[key].after_mib)
const range = values => `${Math.round(Math.min(...values))}–${Math.round(Math.max(...values))}`
</script>

# 性能

**纳秒级核心处理，轻量常驻。**

<PerformanceHighlights />

以下为 Windows 0.12.3 的实测结果。

## 响应速度

核心按键派发与移动帧计算为**纳秒级**，就绪目标的标签生成为**微秒级**。

<PerformanceCore />

仅测内部 CPU 处理，不含系统输入、识别、绘制与刷新。

## 原生阶段测量

从引擎收到按键测到动作请求返回或标签场景提交，不含外部验收截图与屏幕刷新，并非完整的按键到屏幕延迟。

UI Hint 使用真实 Hybrid 识别：单 4K 窗口、400 个按钮的首行标签约 **{{ scenarios.hint_window.overall_ms.toFixed(1) }} 毫秒**提交；双 4K、800 个按钮约 **{{ scenarios.hint_all.overall_ms.toFixed(1) }} 毫秒**完成两屏首行标签提交。

<PerformanceLatency />

下表分别列出两屏标签提交和扫描终态，单位均为毫秒。每次扫描均核对完整目标，双屏各 400 个按钮。

<PerformanceRecognition />

## 内存占用

启动待机约 **{{ Math.round(memory.idle_mib) }} MiB**，日常操作后约 **{{ range(dailyMemory) }} MiB**，UI Hint 识别后约 **{{ range(hintMemory) }} MiB**。

按任务管理器默认的专用工作集统计；含系统共享部分时，启动约 **{{ memory.idle_working_set_mib.toFixed(1) }} MiB**。

<PerformanceMemory />

## 详细数据

“首次”为本场景第一次操作，“重复”为后续使用；“典型”耗时和“操作后”内存取 15 次操作的中位数。

<PerformanceDetails />

## 测试说明

响应与内存分批测量，每组覆盖 8 个场景、120 次操作，每场景 3 个进程、每进程 5 次。UI Hint 采用 Hybrid 识别：单窗口 400 个控件，双屏各 400 个。

原生响应在引擎就绪 3 秒后测试，内存在程序启动 3 秒后测试；首次操作前完成相应模式准备。

- **响应**：从引擎收到测试按键到动作请求返回或标签场景提交，包含计时开销，不含后续系统执行与屏幕刷新。
- **识别**：记录固定首行区域的标签提交，双屏取较慢的一屏；完整扫描的结束时间另列。识别所需截图计入耗时，外部验收截图不计入。
- **内存**：仅统计 KeySteer 的专用工作集，不含其他程序和显存。峰值取三个进程各自采样峰值的中位数。
