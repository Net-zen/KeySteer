# Safety and performance constraints

For input, rendering, or asynchronous work, preserve three things: immediate key response, no stuck input after exit, and no stale work affecting a new session.

## Core invariants

| Area | Required behavior |
| --- | --- |
| Input hook | Return Consume/Forward immediately; no scans, config I/O, process waits, or sleep. Defer remains for compatibility and acts as Forward in the built-in runtime |
| Key ownership | Physical down/up events retain one owner; that owner ends held actions after mode switches |
| Modes / plugins | Use ModeEvent, Command, and HostContext; native calls stay in the Backend |
| Rust boundaries | api, app, config, modes, plugins, and presentation use safe Rust; FFI stays in the platform layer |
| Command storage | Safe inline CommandBatch storage for common 0/1/2-command results; no raw pointers, MaybeUninit, or handwritten drop |
| Session cleanup | Held buttons, click indicators, sequences, scans, overlays, and clocks have completion, cancellation, reload, disable, and exit paths |

::: info Invalid reloads preserve the current session
Failed validation keeps the active configuration and runtime state. A valid reload restarts the runtime. Both paths preserve physical down/up pairing.
:::

## Continuous movement and display frames

Compute distance from each frame's actual `elapsed`, not frame count or an assumed 16 ms interval. Equal durations should give consistent total movement on 60 Hz, 144 Hz, and occasionally delayed frames.

| Platform | Frame source |
| --- | --- |
| Windows 11 | DirectComposition compositor clock, waiting on a stop event too |
| Older Windows | Current output's DXGI VBlank, then DwmFlush |
| macOS | CADisplayLink attached to the overlay View, following its display |

Frame waits belong on workers, without blocking the hook, Engine, or overlay thread or polling refresh rates. Stopping wakes, joins, and releases the worker.

Smooth acceleration uses smootherstep; disabling it selects linear acceleration. Releasing the last direction stops immediately. Long frames crossing the acceleration endpoint integrate the accelerating and constant-speed segments separately. Subpixel remainders, diagonal normalization, and speed multipliers also use actual frame intervals.

## Overlays and asynchronous tasks

| Path | Preserve this behavior |
| --- | --- |
| Windows scene submission | Latest-frame slot and shared OverlayScene; submission does not wait for drawing |
| Static labels and shapes | Copy-on-write; pointer/indicator updates do not copy, sort, or compare the whole Grid |
| Windows high-DPI cache | Rebuild only for static-label or scale changes; release on empty scenes, 100% scale, Hide/Shutdown |
| macOS overlays | UI work runs on the correct thread; Hide detaches the content view/layer tree without retaining a large scene |
| UIA / AX / Vision scans | Workers return partial results with scan ID and owner; discard old modes, displays, and cancelled results |
| Scan memory | Reuse arrays while active, release large allocations on exit |

Before adding asynchronous work, define **result ownership, cancellation, queue/result bounds, and delivery to the Engine**. Delivery must not block the event loop.

## Change checklist

Test on the target system and run the checks below:

- Down/up, repeats, mode switches, and exit leave no stuck keys or mouse buttons.
- Movement remains time-based at low/high refresh rates, across displays, and after dropped frames.
- Failure recovery leaves no overlays, click colors, timers, scans, or frame workers.
- Windows and macOS have corresponding behavior; unsupported returns clear errors.

### Windows input failures

| Check | Requirement |
| --- | --- |
| Evidence | Retain actual SendInput insertion count and immediate GetLastError; zero does not always mean a permission failure |
| Diagnostic cost | Query session, integrity, elevation, and UIAccess and format diagnostics only on failure; no extra diagnostic allocations or system queries on success |
| Click fallback | Prefer complete batches; per-edge retry only when zero were inserted, never replay partial success |
| Release safeguard | Conservatively Release after fallback failure; retain button release at process exit |

### Avoid mutual hook and Engine waits

After deciding key disposition, the Engine must not synchronously wait for the low-level hook to execute SendInput. The next input may already have the hook waiting for the Engine, causing deadlock.

Mouse-button, wheel, and keyboard injection use a preallocated **32-entry bounded FIFO**. The hook executes it serially after returning to its message loop. Adjacent requests share a wakeup; failures reset input asynchronously. Keep the queue bounded, without polling, sleep, extra injection threads, or waits on movement hot paths.

Before release, run:

```bash
cargo fmt --check
cargo test --all-features --lib --tests -- --test-threads=1
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --no-deps
pnpm docs:check
pnpm docs:build
```

Use isolated local adapters for benchmarks without expanding production APIs. See [Workflow and tests](/en/development/workflow) for commands.
