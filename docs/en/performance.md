---
title: Performance
description: KeySteer 0.12.3 response time and memory usage for everyday actions, single-window UI Hint, and dual-display recognition.
---

<script setup>
import { PerformanceHighlights, PerformanceCore, PerformanceLatency, PerformanceRecognition, PerformanceMemory, PerformanceDetails } from '../.vitepress/components/Performance'
import measurements from '../.vitepress/data/performance.json'
const { memory, scenarios } = measurements
const dailyMemory = ['move', 'scroll', 'grid', 'recursive', 'window', 'quick'].map(key => memory.scenarios[key].after_mib)
const hintMemory = ['hint_window', 'hint_all'].map(key => memory.scenarios[key].after_mib)
const range = values => `${Math.round(Math.min(...values))}–${Math.round(Math.max(...values))}`
</script>

# Performance

**Core processing in nanoseconds. A light memory footprint.**

<PerformanceHighlights />

Measured with KeySteer 0.12.3 on Windows.

## Response time

Core key dispatch and movement frame calculation take **nanoseconds**; label creation from ready targets takes **microseconds**.

<PerformanceCore />

These measure internal CPU work, excluding system input, recognition, rendering, and refresh.

## Native stage measurements

Timing runs from engine key receipt to action request return or label scene submission, excluding external verification screenshots and display refresh. These are not full key-to-screen timings.

Real Hybrid recognition submits top-row labels in about **{{ scenarios.hint_window.overall_ms.toFixed(1) }} ms** for one 4K window with 400 buttons, or **{{ scenarios.hint_all.overall_ms.toFixed(1) }} ms** for both 4K displays with 800 buttons. More results continue to arrive.

<PerformanceLatency />

The table separates label submission on each display from the scan terminal result, all in milliseconds. Every scan verifies all targets, including 400 buttons per display.

<PerformanceRecognition />

## Memory usage

About **{{ Math.round(memory.idle_mib) }} MiB** at startup idle, **{{ range(dailyMemory) }} MiB** after everyday actions, and **{{ range(hintMemory) }} MiB** after UI Hint recognition.

Memory uses Task Manager's default private working set. Including shared memory, startup usage is about **{{ memory.idle_working_set_mib.toFixed(1) }} MiB**. The chart also shows sampled peaks during recognition.

<PerformanceMemory />

## Detailed results

“First” is the scenario's first action; “Repeat” is continued use. “Typical” time and “After” memory are medians across 15 actions. Time units appear beside each scenario.

<PerformanceDetails />

<a id="test-conditions"></a>
<a id="reading-the-results"></a>

## About these tests

<a id="core-cpu"></a>
<a id="native-stages-and-memory"></a>

Response and memory are measured in separate batches, each covering eight scenarios and 120 actions: three processes per scenario, five actions per process. UI Hint uses Hybrid recognition, with 400 controls in one window or 400 per display.

Native response tests wait three seconds after engine readiness; memory tests wait three seconds after startup. The required modes are prepared before the first action.

- **Response:** from engine receipt of the test key to action request return or label scene submission. Includes measurement overhead, excluding later system execution and display refresh.
- **Recognition:** records label submission in fixed top-row regions, using the slower display for dual-display results. Full-scan completion is listed separately. Captures needed for recognition are included; external verification screenshots are excluded.
- **Memory:** KeySteer's private working set only, excluding other programs and graphics memory. Peaks are medians of the three processes' individual sampled peaks.
