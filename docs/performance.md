---
title: 性能
description: KeySteer 0.12.3 的实际响应延迟与内存占用，覆盖日常操控、单窗口和双屏 UI Hint 识别。
---

<script setup>
import { PerformanceHighlights, PerformanceLatency, PerformanceMemory, PerformanceDetails } from './.vitepress/components/Performance'
</script>

# 性能

**操作跟手，双屏识别也轻快。**

<PerformanceHighlights />

以下为 Windows 0.12.3 的实测结果。

## 响应速度

滚动、定位和分屏，典型响应约 **8–18 毫秒**。单窗口 400 个控件时约 **0.17 秒**出现首批标签；双 4K、800 个控件时，约 **0.21 秒**两屏都能看到第一批标签；看到目标就能输入选择。

<PerformanceLatency />

## 内存占用

启动待机约 **12 MiB**，日常操作后约 **16–20 MiB**，UI Hint 识别后约 **27–31 MiB**。识别时会短暂多用一些内存，图中也列出测试中记录到的最高占用。

内存按任务管理器的显示统计；算上与系统共用的部分，启动时约 **38.7 MiB**。

<PerformanceMemory />

## 详细数据

“首次”是启动后的第一次操作，“重复”是接着使用；“典型”响应和“操作后”内存取多次测试的中间值。

<PerformanceDetails />

<a id="测试条件"></a>
<a id="如何理解这些数字"></a>

## 测试说明

响应和内存分别测了 8 个场景、120 次操作。UI Hint 采用 Hybrid 识别：单窗口 400 个控件，双屏各 400 个。

- **响应**：从按下测试按键到画面变化；鼠标移动看位置变化。不含实体键盘和显示器的额外延迟。
- **识别**：计时到两屏都出现第一批标签，后续标签继续补全。搜索和退出另算。
- **内存**：只算 KeySteer，不含其他程序和显存。最高占用是测试中采样记录到的最高值。
