---
title: Performance
description: Measured KeySteer response latency and memory usage across everyday actions, single-window recognition, and dual-display UI Hint.
---

<script setup>
import { PerformanceHighlights, PerformanceLatency, PerformanceMemory, PerformanceDetails } from '../.vitepress/components/Performance'
</script>

# Performance

**Responsive controls. Fast recognition across two displays.**

<PerformanceHighlights />

Measured with KeySteer 0.12.2 on Windows.

## Response time

Scrolling, targeting, and window actions typically respond in **9–18 ms**. With 800 controls on two 4K displays, the first labels appear on both in about **0.28 s**. Start typing as soon as you see your target.

<PerformanceLatency />

## Memory usage

About **11 MiB** at startup idle, **15–19 MiB** after everyday actions, and **26–28 MiB** after UI Hint recognition. Recognition briefly uses more memory, so the chart also shows the highest usage recorded during testing.

Memory is measured as shown in Task Manager. Including the parts shared with Windows, startup usage is about **37.5 MiB**.

<PerformanceMemory />

## Detailed results

“First” is the first action after startup; “Repeat” is continued use. “Typical” response and “After” memory show the middle values across repeated tests.

<PerformanceDetails />

<a id="test-conditions"></a>
<a id="reading-the-results"></a>

## About these tests

Tested on 2026-10-18 with KeySteer 0.12.2 on Windows. Response and memory were tested separately, each covering eight scenarios and 120 actions. UI Hint used Hybrid recognition, with 400 controls in one window or 400 per display.

- **Response:** from a test keypress within the mode to a picture change; pointer movement uses position changes. Physical keyboard and monitor delays are excluded.
- **Recognition:** timing ends when the first labels appear on both displays. More labels continue to arrive. Search and exit time are separate.
- **Memory:** KeySteer only, excluding other programs and graphics memory. The highest value is the highest usage recorded during testing.
