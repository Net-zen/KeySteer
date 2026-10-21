---
title: Performance
description: Measured KeySteer 0.12.3 response latency and memory usage across everyday actions, single-window recognition, and dual-display UI Hint.
---

<script setup>
import { PerformanceHighlights, PerformanceLatency, PerformanceMemory, PerformanceDetails } from '../.vitepress/components/Performance'
</script>

# Performance

**Responsive controls. Fast recognition across two displays.**

<PerformanceHighlights />

Measured with KeySteer 0.12.3 on Windows.

## Response time

Scrolling, targeting, and window actions typically respond in **8–17 ms**. First labels appear in about **143 ms** with 400 controls in one window, or **225 ms** on both displays with 800 controls across two 4K displays. Start typing as soon as you see your target.

<PerformanceLatency />

## Memory usage

About **12 MiB** at startup idle, **16–19 MiB** after everyday actions, and **27–31 MiB** after UI Hint recognition. Recognition briefly uses more memory, so the chart also shows the highest usage recorded during testing.

Memory uses the private working set shown by default in Task Manager. Including the parts shared with Windows, startup usage is about **38.1 MiB**.

<PerformanceMemory />

## Detailed results

“First” is the first test action after the app is ready; “Repeat” is continued use. “Typical” response and “After” memory show the medians across repeated tests.

<PerformanceDetails />

<a id="test-conditions"></a>
<a id="reading-the-results"></a>

## About these tests

Response and memory were measured in separate batches, each covering eight scenarios and 120 actions: three processes per scenario, with five actions per process. UI Hint used Hybrid recognition, with 400 controls in one window or 400 per display.

Everyday response and memory use the earlier full-scenario retest; UI Hint response uses the post-fix, same-scene A/B retest. Each process waits three seconds after startup before its first action.

- **Response:** from a test keypress to a picture change; pointer movement uses position changes. Physical keyboard and monitor delays are excluded.
- **Recognition:** fixed top-row label regions are observed, using the slower display for dual-display results. More labels continue to arrive; this is not full-scan completion time. Search and exit time are separate.
- **Memory:** KeySteer only, excluding other programs and graphics memory. The highest value is the highest usage sampled during testing.
