//! Opt-in measurements of the real session path with deterministic native data.
//! Includes enumeration/cloning, excludes OS calls, worker queueing and pointer injection.
use super::*;
use crate::api::window_tabs::{TabGroup, TabGroupId, TabState};
use std::hint::black_box;

fn fixture(count: u64, case: &str) -> Fake {
    let mut access = Fake::new(count);
    for (id, window) in &mut access.windows {
        window.info.bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
        window.info.app = format!("app{}", id.0 % 4);
    }
    access.selected.set(Some(WindowId(1)));
    if case == "no_anchor" {
        access.selected.set(None);
    } else if case == "isolated" {
        access.windows.get_mut(&WindowId(1)).unwrap().info.bounds.x = -1000.0;
    } else if matches!(case, "tabs_hit" | "tabs_fallback" | "window_tabs") {
        let members = if case == "tabs_fallback" { 2 } else { 4 };
        access.tabs = Some(TabState {
            groups: (0..count / members)
                .map(|group| TabGroup {
                    id: TabGroupId(group as u32 + 1),
                    members: (group * members + 1..=(group + 1) * members)
                        .map(WindowId)
                        .collect(),
                    active: WindowId(group * members + 1),
                })
                .collect(),
            numbers: (1..=count).map(|id| (WindowId(id), id as u32)).collect(),
            ..TabState::default()
        });
        if case == "tabs_fallback" {
            access.unavailable.push(WindowId(2));
        }
    }
    access
}

#[test]
#[ignore = "release only; run alone with --ignored --nocapture --test-threads=1"]
#[allow(clippy::assertions_on_constants)] // Runtime guard for an opt-in release-only probe.
fn overlap_cycle_performance() {
    assert!(!cfg!(debug_assertions), "use --release");
    let screens = screens();
    for count in [4, 7, 8, 9, 15, 16, 17, 23, 24, 25, 31, 32, 33, 48, 96] {
        for case in [
            "no_anchor",
            "tabs_hit",
            "tabs_fallback",
            "isolated",
            "ungrouped",
            "active",
            "window_tabs",
            "active_explicit",
            "mouse_explicit",
            "overlap_active",
            "overlap_mouse",
        ] {
            let mut access = fixture(count, case);
            let mut session = Session {
                target: Some(WindowId(1)),
                ..Session::default()
            };
            let operation = match case {
                "active" => WindowOperation::CycleActive { backwards: false },
                "active_explicit" | "mouse_explicit" | "overlap_active" | "overlap_mouse" => {
                    WindowOperation::CycleFrom {
                        backwards: false,
                        overlapping: case.starts_with("overlap_"),
                        source: if matches!(case, "active_explicit" | "overlap_active") {
                            crate::api::window::WindowTarget::Active
                        } else {
                            crate::api::window::WindowTarget::Mouse
                        },
                    }
                }
                "window_tabs" => WindowOperation::Cycle,
                _ => WindowOperation::CycleOverlapping { backwards: false },
            };
            let request = WindowRequest {
                scope: None,
                session: 0,
                id: 0,
                operation,
            };
            let mut step = || {
                if matches!(case, "mouse_explicit" | "overlap_mouse") {
                    access.pointer_target = access.selected.get();
                }
                if case == "tabs_fallback" {
                    access.selected.set(Some(WindowId(1)));
                }
                let result = session.execute(&mut access, request.clone(), &screens, &|| false);
                assert!(result.message.is_none());
                black_box(result)
            };
            for _ in 0..256 {
                drop(step());
            }
            let allocation = stats_alloc::Region::new(crate::TEST_ALLOCATOR);
            drop(step());
            let stats = allocation.change();
            let mut samples = Vec::with_capacity(5000);
            let mut checksum = 0u64;
            for _ in 0..5000 {
                let start = Instant::now();
                for _ in 0..4 {
                    let result = step();
                    checksum = checksum.wrapping_add(result.target.as_ref().map_or(0, |w| w.id.0));
                }
                samples.push(start.elapsed().as_nanos() as u64 / 4);
            }
            samples.sort_unstable();
            println!(
                "OVERLAP_PERF case={case} n={count} p50={} p95={} p99={} allocs={} bytes={} checksum={checksum}",
                samples[2500],
                samples[4750],
                samples[4950],
                stats.allocations,
                stats.bytes_allocated
            );
        }
    }
}

#[test]
#[ignore = "release only; run alone with --ignored --nocapture --test-threads=1"]
#[allow(clippy::assertions_on_constants)]
fn refresh_kernel_performance() {
    assert!(!cfg!(debug_assertions), "use --release");
    for count in [4, 8, 9, 16, 24, 32, 33, 48, 96, 128, 129] {
        for case in ["stable", "reordered", "churn", "churn_last", "duplicate"] {
            let access = Fake::new(count + 1);
            let mut all: Vec<_> = access.windows.values().map(|w| w.info.clone()).collect();
            all.sort_by_key(|w| w.id.0);
            let first = all[..count as usize].to_vec();
            let mut second = first.clone();
            match case {
                "reordered" => second.reverse(),
                "churn" => second[0] = all[count as usize].clone(),
                "churn_last" => second[count as usize - 1] = all[count as usize].clone(),
                "duplicate" => second[0] = second[1].clone(),
                _ => {}
            }
            let mut cycle = Vec::with_capacity(count as usize + 1);
            let mut calls = 0;
            let mut step = || {
                let windows = if calls % 2 == 0 { &first } else { &second };
                calls += 1;
                refresh_cycle_order(black_box(&mut cycle), black_box(windows));
                black_box(&cycle);
            };
            for _ in 0..256 {
                step();
            }
            let region = stats_alloc::Region::new(crate::TEST_ALLOCATOR);
            step();
            let stats = region.change();
            let mut samples = Vec::with_capacity(1000);
            for _ in 0..1000 {
                let start = Instant::now();
                for _ in 0..64 {
                    step();
                }
                samples.push(start.elapsed().as_nanos() as u64 / 64);
            }
            samples.sort_unstable();
            println!(
                "REFRESH_PERF case={case} n={count} p50={} p99={} allocs={} reallocs={} bytes={}",
                samples[500],
                samples[990],
                stats.allocations,
                stats.reallocations,
                stats.bytes_allocated
            );
        }
    }
}
