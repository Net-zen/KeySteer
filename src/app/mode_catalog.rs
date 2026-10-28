//! The only assembly point for built-in modes.
//!
//! Configuration DTOs stop here. Each mode receives only its own immutable,
//! strongly typed settings and remains unaware of TOML or the application
//! configuration document.

use crate::api::{Mode, ModeId, Plugin};
use crate::config::{AppOverride, Bindings, Config, UiHintAppOverride};
use crate::modes::targeting::{BisectGeometry, GridGeometry, LetterGeometry, RecursiveGeometry};
use crate::modes::{self, GridMode, HintMode, IdleMode, NormalMode};
use crate::plugins::BundledSettings;

use super::runtime::{AppRouteOverride, ModeRoute, ModeSpec};

pub(crate) fn normal_settings(config: &Config) -> modes::normal::Settings {
    modes::normal::Settings {
        pointer: modes::normal::PointerSettings {
            initial_speed: config.pointer.initial_speed,
            max_speed: config.pointer.max_speed,
            acceleration: config.pointer.acceleration,
            smooth_acceleration: config.pointer.smooth_acceleration,
            tap_distance: config.pointer.tap_distance,
            slow_multiplier: config.pointer.slow_multiplier,
            precision_multiplier: config.pointer.precision_multiplier,
            fast_multiplier: config.pointer.fast_multiplier,
        },
        scroll: modes::normal::ScrollSettings::new(
            config.scroll.scroll_step,
            config.scroll.scroll_step_half,
            config.scroll.scroll_step_full,
            config.scroll.speed,
        ),
        passthrough_unbound_keys: config.normal.passthrough_unbound_keys,
    }
}

pub(crate) fn hint_settings(config: &Config) -> modes::hint::Settings {
    modes::hint::Settings {
        strategy: config.ui_hint.strategy,
        scan_scope: config.ui_hint.scan_scope,
        vision: config.ui_hint.vision.clone(),
        hint_characters: config.ui_hint.hint_characters.clone(),
        label_direction: config.ui_hint.label_direction,
        max_depth: config.ui_hint.max_depth,
        scan_timeout_ms: config.ui_hint.scan_timeout_ms,
        scan_retry_count: config.ui_hint.scan_retry_count,
        scan_retry_delay_ms: config.ui_hint.scan_retry_delay_ms,
        clickable_roles: config.ui_hint.clickable_roles.clone(),
        ignore_clickable_check: config.ui_hint.ignore_clickable_check,
        visible_check_enabled: config.ui_hint.visible_check_enabled,
        placement: config.ui_hint.placement,
        label_x_offset: config.ui_hint.label_x_offset,
        label_y_offset: config.ui_hint.label_y_offset,
        ui: config.ui_hint.ui.compile(),
        boundary_highlight: config.ui_hint.boundary_highlight.compile(),
        search_highlight: config.ui_hint.search_highlight.compile(),
        search_input_ui: crate::api::style::CompiledSearchPanel::new(
            &config.ui_hint.search_input_ui,
            &config.palette(crate::api::Appearance::Light),
            &config.palette(crate::api::Appearance::Dark),
        )
        .with_point_input_colors(
            config.ui_hint.search_point.input_background_color.as_ref(),
            config.ui_hint.search_point.input_border_color.as_ref(),
        ),
        search_info_ui: crate::api::style::CompiledSearchPanel::new(
            &config.ui_hint.search_info_ui,
            &config.palette(crate::api::Appearance::Light),
            &config.palette(crate::api::Appearance::Dark),
        )
        .with_color_preview(config.ui_hint.search_point.color_preview),
        search_copy_keys: config
            .ui_hint
            .copy_chords()
            .unwrap_or_else(|error| panic!("search requires validated copy keys: {error}"))
            .into_iter()
            .collect(),
        search_edit_keys: config
            .ui_hint
            .search_edit_keys
            .iter()
            .flat_map(|(action, keys)| keys.split_whitespace().map(move |key| (action, key)))
            .map(|(action, key)| {
                (
                    crate::api::KeyChord::parse(key).unwrap_or_else(|error| {
                        panic!("search requires validated editing key: {error}")
                    }),
                    *action,
                )
            })
            .collect(),
        search_match_priority: crate::api::hint::CompiledSearchPriority::new(
            config.ui_hint.search_match_priority,
        ),
        search_titles: ["OCR", "Accessibility", "Coordinates", "Color"].map(String::from),
        search_point: modes::hint::PointSettings {
            field_modes: config.ui_hint.search_point.field_modes,
            keys: config
                .ui_hint
                .search_bindings
                .iter()
                .map(|(key, action)| {
                    (
                        crate::api::KeyChord::parse(key).unwrap_or_else(|error| {
                            panic!("search requires validated point binding: {error}")
                        }),
                        *action,
                    )
                })
                .collect(),
            formats: config.ui_hint.search_point.color_formats.clone(),
            marker_color: config
                .ui_hint
                .search_point
                .marker_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
            marker_radius: config.ui_hint.search_point.marker_radius,
            marker_width: config.ui_hint.search_point.marker_width,
            movement: normal_settings(config),
        },
        lifecycle: config.ui_hint.lifecycle.clone(),
        overlap_cycle_key: config.ui_hint.overlap_cycle_key.clone(),
        app_overrides: config
            .ui_hint
            .app_configs
            .iter()
            .map(|entry| modes::hint::AppStrategyOverride {
                pattern: entry.bundle_id.clone(),
                strategy: entry.strategy,
            })
            .collect(),
    }
}

/// Instantiate the built-in modes enabled by a validated configuration.
#[doc(hidden)]
pub fn normal(config: &Config) -> NormalMode {
    NormalMode::new(normal_settings(config))
}

/// Select the concrete runtime once; ordinary Normal retains its original
/// object size and frame/input implementation when targeting is absent.
fn normal_instance(config: &Config) -> Box<dyn Mode> {
    use crate::config::TargetingReset;
    use modes::targeting::{Layout, TargetingController};
    let Some(targeting) = &config.normal.targeting else {
        return Box::new(normal(config));
    };
    let geometry = targeting.resolve(config);
    let layers: Vec<_> = geometry
        .layers
        .iter()
        .map(|layer| modes::targeting::LayerSettings {
            depth: layer.depth,
            grid_cols: layer.grid_cols,
            grid_rows: layer.grid_rows,
            keys: layer.keys.clone(),
        })
        .collect();
    let grid = GridGeometry::new(
        Layout {
            rows: geometry.grid_rows as usize,
            cols: geometry.grid_cols as usize,
            keys: geometry.keys.chars().collect(),
        },
        &layers,
        geometry.max_depth,
    );
    fn wrap<G: LetterGeometry>(
        config: &Config,
        grid: G,
        depth: u32,
        reset_move: bool,
        reset_click: bool,
    ) -> Box<dyn Mode> {
        Box::new(modes::normal_targeting::NormalTargeting::new(
            normal(config),
            TargetingController::new(grid, depth, true, crate::api::TargetingLifecycle::default()),
            reset_move,
            reset_click,
        ))
    }
    let reset_move = targeting.reset_on.contains(&TargetingReset::Move);
    let reset_click = targeting.reset_on.contains(&TargetingReset::Click);
    match geometry.min_size {
        Some((width, height)) => wrap(
            config,
            RecursiveGeometry {
                grid,
                min_size: (width as f64, height as f64),
            },
            geometry.max_depth,
            reset_move,
            reset_click,
        ),
        None => wrap(config, grid, geometry.max_depth, reset_move, reset_click),
    }
}

/// Compile and validate the optional input alphabet once. No registry/runtime
/// fields or conflict checks are added to ordinary input processing.
pub(crate) fn compile_normal_targeting(
    config: &Config,
    specs: &mut [ModeSpec],
) -> Result<(), String> {
    use crate::api::{Binding, Key, KeyChord};
    use std::collections::{BTreeMap, BTreeSet};
    let Some(targeting) = &config.normal.targeting else {
        return Ok(());
    };
    let geometry = targeting.resolve(config);
    let root_keys = geometry
        .layers
        .iter()
        .find(|layer| layer.depth == 0)
        .and_then(|layer| layer.keys.as_deref())
        .unwrap_or(geometry.keys);
    let alphabet = std::iter::once(if geometry.max_depth == 1 {
        root_keys
    } else {
        geometry.keys
    })
    .chain(
        geometry
            .layers
            .iter()
            .filter(|_| geometry.max_depth != 1)
            .filter_map(|layer| layer.keys.as_deref()),
    )
    .flat_map(str::chars)
    .map(|ch| ch.to_string())
    .collect::<BTreeSet<_>>();
    let mut generated = BTreeMap::new();
    for name in alphabet {
        let chord = KeyChord::parse_with_aliases(&name, config.resolved_key_aliases())?;
        if chord.keys().len() != 1 || chord.activation_key().is_modifier() {
            return Err(format!(
                "normal.targeting key {name:?} must resolve to a single non-modifier key"
            ));
        }
        let binding = Binding::TargetingKey(Key::new(&name)?);
        if generated.insert(chord.canonical(), binding).is_some() {
            return Err(format!(
                "normal.targeting key {name:?} collides with another selection/control key after aliases"
            ));
        }
    }
    let Some(normal) = specs.iter().position(|spec| spec.id() == ModeId::normal()) else {
        return Err("normal.targeting requires the Normal mode in the catalog".into());
    };
    let mut pending = vec![ModeId::normal()];
    let mut visited = BTreeSet::new();
    let mut conflicts = BTreeSet::new();
    // Every possible inherited application override is checked up front. The
    // feature cannot become partially unusable only after focus changes.
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(spec) = specs.iter().find(|spec| spec.id() == id) else {
            continue;
        };
        pending.extend(spec.route.inherits.iter().cloned());
        for (scope, bindings) in std::iter::once((id.to_string(), &spec.route.bindings)).chain(
            spec.route
                .app_overrides
                .iter()
                .map(|entry| (format!("{id} app {:?}", entry.pattern), &entry.bindings)),
        ) {
            for (text, binding) in bindings {
                let chord = KeyChord::parse_with_aliases(text, config.resolved_key_aliases())?;
                let Some(expected) = generated.get(&chord.canonical()) else {
                    continue;
                };
                // An explicit Normal 'none' relinquishes its old single-key
                // action and masks inherited conflicts. App overrides still
                // require validation, because they replace that local entry.
                if id != ModeId::normal()
                    && specs[normal].route.bindings.get(&chord.canonical())
                        == Some(&Binding::Disabled)
                {
                    continue;
                }
                if binding != &Binding::Disabled && binding != expected {
                    conflicts.insert(format!(
                        "{} = {} ({scope})",
                        chord.canonical(),
                        binding.canonical()
                    ));
                }
            }
        }
    }
    if !conflicts.is_empty() {
        return Err(format!(
            "normal.targeting conflicts: {}. Rebind the grid keys or remove these Normal bindings (use none to relinquish an inherited/default action).",
            conflicts.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if geometry.max_depth != 1 {
        let (name, controls) = match targeting.method {
            crate::config::TargetingMethod::Grid => ("grid", &config.grid.bindings),
            crate::config::TargetingMethod::RecursiveGrid => {
                ("recursive_grid", &config.recursive_grid.bindings)
            }
        };
        for (key, binding) in
            crate::config::with_control_defaults(name, controls, config.resolved_key_aliases())
                .map_err(|e| e.to_string())?
        {
            if matches!(binding, Binding::SelectionReset | Binding::SelectionParent)
                && !specs[normal].route.bindings.contains_key(&key)
            {
                generated.entry(key).or_insert(binding);
            }
        }
    }
    // Keep generated inputs local to Normal: inheritors retain their own raw
    // key behavior. Temporary Normal still accesses Normal's compiled table.
    let mut inheritors = BTreeSet::from([ModeId::normal()]);
    loop {
        let before = inheritors.len();
        for spec in specs.iter() {
            if spec.route.inherits.iter().any(|id| inheritors.contains(id)) {
                inheritors.insert(spec.id());
            }
        }
        if inheritors.len() == before {
            break;
        }
    }
    for spec in specs
        .iter_mut()
        .filter(|spec| spec.id() != ModeId::normal() && inheritors.contains(&spec.id()))
    {
        for name in generated.keys() {
            spec.route
                .bindings
                .entry(name.clone())
                .or_insert(Binding::Disabled);
        }
    }
    for entry in &mut specs[normal].route.app_overrides {
        for (key, binding) in &generated {
            if matches!(binding, Binding::TargetingKey(_))
                && entry.bindings.get(key) == Some(&Binding::Disabled)
            {
                entry.bindings.insert(key.clone(), binding.clone());
            }
        }
    }
    specs[normal].route.bindings.extend(generated);
    Ok(())
}

#[doc(hidden)]
pub fn grid(config: &Config) -> GridMode<GridGeometry> {
    use modes::targeting::{Layout, TargetingController};
    let settings = &config.grid;
    GridMode::new(
        TargetingController::new(
            GridGeometry::new(
                Layout {
                    rows: settings.grid_rows.max(1) as usize,
                    cols: settings.grid_cols.max(1) as usize,
                    keys: settings.keys.chars().collect(),
                },
                &[],
                settings.max_depth,
            ),
            settings.max_depth,
            settings.cursor_follow_selection,
            settings.lifecycle.clone(),
        ),
        crate::api::presentation::GridStyle {
            label: config.grid.ui.label.compile(),
            matched_background_color: config
                .grid
                .ui
                .matched_background_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
            matched_border_color: config
                .grid
                .ui
                .matched_border_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
        },
    )
}

#[doc(hidden)]
pub fn recursive_grid(config: &Config) -> GridMode<RecursiveGeometry> {
    use modes::targeting::{Layout, TargetingController};
    let settings = &config.recursive_grid;
    let layers: Vec<_> = settings
        .layers
        .iter()
        .map(|layer| modes::targeting::LayerSettings {
            depth: layer.depth,
            grid_cols: layer.grid_cols,
            grid_rows: layer.grid_rows,
            keys: layer.keys.clone(),
        })
        .collect();
    GridMode::new(
        TargetingController::new(
            RecursiveGeometry {
                grid: GridGeometry::new(
                    Layout {
                        rows: settings.grid_rows.max(1) as usize,
                        cols: settings.grid_cols.max(1) as usize,
                        keys: settings.keys.chars().collect(),
                    },
                    &layers,
                    settings.max_depth,
                ),
                min_size: (
                    settings.min_size_width as f64,
                    settings.min_size_height as f64,
                ),
            },
            settings.max_depth,
            settings.cursor_follow_selection,
            settings.lifecycle.clone(),
        ),
        crate::api::presentation::RecursiveGridStyle {
            label: config.recursive_grid.ui.label.compile(),
            line_width: config.recursive_grid.ui.line_width,
            line_color: config
                .recursive_grid
                .ui
                .line_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
            highlight_color: config
                .recursive_grid
                .ui
                .highlight_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
            label_background: config.recursive_grid.ui.label_background,
            label_background_color: config
                .recursive_grid
                .ui
                .label_background_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
            label_char: config.recursive_grid.ui.label_char.clone(),
            label_min_font_size: config.recursive_grid.ui.label_min_font_size,
            label_autohide_multiplier: config.recursive_grid.ui.label_autohide_multiplier,
            sub_key_preview: config.recursive_grid.ui.sub_key_preview,
            sub_key_preview_font_size: config.recursive_grid.ui.sub_key_preview_font_size,
            sub_key_preview_text_color: config
                .recursive_grid
                .ui
                .sub_key_preview_text_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
            sub_key_preview_autohide_multiplier: config
                .recursive_grid
                .ui
                .sub_key_preview_autohide_multiplier,
        },
    )
}

#[doc(hidden)]
pub fn bisect(config: &Config) -> GridMode<BisectGeometry> {
    let settings = &config.bisect;
    GridMode::new(
        modes::targeting::TargetingController::new(
            BisectGeometry {
                min_size: (
                    settings.min_size_width as f64,
                    settings.min_size_height as f64,
                ),
            },
            settings.max_depth,
            settings.cursor_follow_selection,
            settings.lifecycle.clone(),
        ),
        crate::api::presentation::BisectStyle {
            line_width: settings.ui.line_width,
            line_color: settings
                .ui
                .line_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
            highlight_color: settings
                .ui
                .highlight_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
        },
    )
}

#[doc(hidden)]
pub fn hint(config: &Config) -> HintMode {
    HintMode::new(hint_settings(config))
}

pub(crate) fn quick_switch(config: &Config) -> modes::quick_switch::QuickSwitchMode {
    let style = |appearance| {
        let palette = config.theme.palette(appearance);
        let mut panel = config.quick_switch.ui.resolve(
            &palette,
            palette.surface_label(),
            palette.text,
            palette.accent_border(),
        );
        if config.quick_switch.ui.matched_text_color.is_none() {
            panel.matched_text_color = crate::api::style::HINT_MATCHED_TEXT_COLOR;
        }
        crate::api::style::QuickSwitchStyles::new(panel)
    };
    modes::quick_switch::QuickSwitchMode::new(modes::quick_switch::Settings {
        alphabet: config
            .quick_switch
            .hint_characters
            .chars()
            .map(|ch| {
                crate::api::Key::new(ch.to_string())
                    .and_then(|key| {
                        key.as_char()
                            .ok_or_else(|| "expected a character key".into())
                    })
                    .unwrap_or_else(|error| {
                        panic!("quick_switch requires validated hint characters: {error}")
                    })
            })
            .collect(),
        blacklist: config.quick_switch.blacklist.clone(),
        position: config.quick_switch.position,
        light: style(crate::api::Appearance::Light),
        dark: style(crate::api::Appearance::Dark),
    })
}

pub(crate) fn text_select(config: &Config) -> modes::text_select::TextSelectMode {
    let mut hint = hint_settings(config);
    hint.scan_scope = config.text_select.scan_scope;
    hint.search_info_ui.width = f64::from(config.text_select.ui.panel_width);
    modes::text_select::TextSelectMode::new(modes::text_select::Settings {
        hint,
        alphabet: config.text_select.hint_characters.clone(),
        ui: config.text_select.ui.compile(),
        lifecycle: config.text_select.lifecycle.clone(),
        bindings: config
            .text_select
            .bindings
            .iter()
            .flat_map(|(keys, binding)| {
                keys.split_whitespace().filter_map(|key| {
                    crate::api::KeyChord::parse_with_aliases(key, config.resolved_key_aliases())
                        .ok()
                        .map(|chord| (chord, std::sync::Arc::new(binding.clone())))
                })
            })
            .collect(),
    })
}

#[doc(hidden)]
#[allow(dead_code)]
pub fn built_in(config: &Config) -> Vec<Box<dyn Mode>> {
    let mut catalog: Vec<Box<dyn Mode>> = vec![
        Box::new(IdleMode::new()),
        normal_instance(config),
        Box::new(modes::text_input::TextInputMode::new()),
    ];
    if config.quick_switch.enabled {
        catalog.push(Box::new(quick_switch(config)));
    }
    if config.grid.enabled {
        catalog.push(Box::new(grid(config)));
    }
    if config.recursive_grid.enabled {
        catalog.push(Box::new(recursive_grid(config)));
    }
    if config.bisect.enabled {
        catalog.push(Box::new(bisect(config)));
    }
    if config.ui_hint.enabled {
        catalog.push(Box::new(hint(config)));
    }
    if config.text_select.enabled {
        catalog.push(Box::new(text_select(config)));
    }
    catalog.extend(
        window_family(config)
            .into_iter()
            .map(|mode| Box::new(mode) as Box<dyn Mode>),
    );
    catalog
}

fn window_settings(config: &Config, kind: modes::window::WindowKind) -> modes::window::Settings {
    use modes::window::WindowKind as K;
    let common = match kind {
        K::Move => &config.window.common,
        K::Quick => &config.window_quick.common,
        K::Editor => &config.window_editor.common,
        K::Restore => &config.window_restore.common,
        K::Tab => &config.window_tab.common,
    };
    let card = if kind == K::Editor {
        config.window_editor.card.apply(&config.window.card)
    } else {
        config.window.card.clone()
    };
    modes::window::Settings {
        multi_bindings: crate::config::with_control_defaults(
            "window.multi_select",
            &config.window.multi_select.bindings,
            config.resolved_key_aliases(),
        )
        .unwrap_or_else(|error| panic!("window settings require validated configuration: {error}"))
        .iter()
        .flat_map(|(keys, binding)| {
            keys.split_whitespace().map(|key| {
                (
                    crate::api::KeyChord::parse_with_aliases(key, config.resolved_key_aliases())
                        .unwrap_or_else(|error| {
                            panic!("window settings require validated configuration: {error}")
                        }),
                    std::sync::Arc::new(binding.clone()),
                )
            })
        })
        .collect(),
        target: common.target,
        all_screens: common.screens == crate::config::WindowScreens::All,
        include_minimized: common.include_minimized,
        lifecycle: common.lifecycle.clone(),
        ratio_ticks: config.window_quick.split_ratio_ticks().into(),
        split_ratios: config
            .window_quick
            .parsed_split_ratios()
            .unwrap_or_else(|error| {
                panic!("window settings require validated configuration: {error}")
            })
            .into_iter()
            .chain([1.0])
            .collect(),
        number_timeout_ms: common.number_timeout_ms,
        move_step: config.window.move_step,
        move_speed: config.window.move_speed,
        resize_step: if kind == K::Editor {
            config.window_editor.resize_step
        } else {
            config.window.resize_step
        },
        resize_speed: if kind == K::Editor {
            config.window_editor.resize_speed
        } else {
            config.window.resize_speed
        },
        gap: match kind {
            K::Quick => config.window_quick.gap,
            K::Restore => config.window_restore.gap,
            _ => config.window_editor.gap,
        },
        border_width: common.border_width,
        styles: crate::api::style::WindowStyles::new(
            &common.ui,
            &card,
            &config.palette(crate::api::Appearance::Light),
            &config.palette(crate::api::Appearance::Dark),
            crate::presentation::window::RENDERERS,
        )
        .into(),
    }
}
fn window_family(config: &Config) -> Vec<modes::window::WindowMode> {
    use modes::window::WindowKind as K;
    modes::window::WindowMode::family(
        [K::Move, K::Quick, K::Editor, K::Restore, K::Tab]
            .into_iter()
            .zip(config.window_modes())
            .filter(|(_, (_, mode))| mode.enabled)
            .map(|(kind, _)| (kind, window_settings(config, kind))),
    )
}
#[cfg(test)]
pub(crate) fn window(config: &Config) -> modes::window::WindowSession {
    modes::window::WindowSession::new(window_settings(config, modes::window::WindowKind::Move))
}

pub(crate) fn bundled_plugin_settings(config: &Config) -> BundledSettings {
    BundledSettings {
        key_aliases: config.resolved_key_aliases().clone(),
        screen_selector_preserve: config
            .plugin_setting_bool("plugin:screen-selector", "preserve")
            .unwrap_or(true),
    }
}

#[doc(hidden)]
pub fn bundled_plugins(config: &Config) -> Result<Vec<Box<dyn Plugin>>, String> {
    crate::plugins::bundled(bundled_plugin_settings(config))
}

/// Build the complete built-in catalog. Instance construction and route
/// compilation intentionally live together so adding a mode has one assembly
/// point and cannot produce a route/instance mismatch.
pub(crate) fn built_in_specs(config: &Config) -> Result<Vec<ModeSpec>, String> {
    let mut specs = Vec::with_capacity(5);
    if config.quick_switch.enabled {
        specs.push(ModeSpec::built_in(
            Box::new(quick_switch(config)),
            compile_route(
                &config.quick_switch.bindings,
                &config.quick_switch.inherits,
                config.quick_switch.temporary_mode.as_deref(),
                &config.quick_switch.temporary_mode_keys,
                &config.quick_switch.temporary_mode_passthrough_keys,
                Vec::new(),
            )?,
        ));
    }
    if config.text_select.enabled {
        specs.push(ModeSpec::built_in(
            Box::new(text_select(config)),
            compile_route(
                &config.text_select.bindings,
                &config.text_select.inherits,
                config.text_select.temporary_mode.as_deref(),
                &config.text_select.temporary_mode_keys,
                &config.text_select.temporary_mode_passthrough_keys,
                Vec::new(),
            )?,
        ));
    }
    specs.push(ModeSpec::built_in(
        Box::new(modes::text_input::TextInputMode::new()),
        compile_route(
            &config.text_input.bindings,
            &config.text_input.inherits,
            config.text_input.temporary_mode.as_deref(),
            &config.text_input.temporary_mode_keys,
            &config.text_input.temporary_mode_passthrough_keys,
            Vec::new(),
        )?,
    ));
    specs.push(ModeSpec::built_in(
        Box::new(IdleMode::new()),
        compile_route(
            &config.hotkeys,
            &[],
            None,
            &[],
            &[],
            app_overrides(&config.app_configs),
        )?,
    ));
    specs.push(ModeSpec::built_in(
        normal_instance(config),
        compile_route(
            &config.normal.bindings,
            &config.normal.inherits,
            config.normal.temporary_mode.as_deref(),
            &config.normal.temporary_mode_keys,
            &config.normal.temporary_mode_passthrough_keys,
            app_overrides(&config.normal.app_configs),
        )?,
    ));
    if config.grid.enabled {
        specs.push(ModeSpec::built_in(
            Box::new(grid(config)),
            compile_route(
                &config.grid.bindings,
                &config.grid.inherits,
                config.grid.temporary_mode.as_deref(),
                &config.grid.temporary_mode_keys,
                &config.grid.temporary_mode_passthrough_keys,
                app_overrides(&config.grid.app_configs),
            )?,
        ));
    }
    if config.recursive_grid.enabled {
        specs.push(ModeSpec::built_in(
            Box::new(recursive_grid(config)),
            compile_route(
                &config.recursive_grid.bindings,
                &config.recursive_grid.inherits,
                config.recursive_grid.temporary_mode.as_deref(),
                &config.recursive_grid.temporary_mode_keys,
                &config.recursive_grid.temporary_mode_passthrough_keys,
                app_overrides(&config.recursive_grid.app_configs),
            )?,
        ));
    }
    if config.bisect.enabled {
        specs.push(ModeSpec::built_in(
            Box::new(bisect(config)),
            compile_route(
                &config.bisect.bindings,
                &config.bisect.inherits,
                config.bisect.temporary_mode.as_deref(),
                &config.bisect.temporary_mode_keys,
                &config.bisect.temporary_mode_passthrough_keys,
                app_overrides(&config.bisect.app_configs),
            )?,
        ));
    }
    if config.ui_hint.enabled {
        specs.push(ModeSpec::built_in(
            Box::new(hint(config)),
            compile_route(
                &config.ui_hint.bindings,
                &config.ui_hint.inherits,
                config.ui_hint.temporary_mode.as_deref(),
                &config.ui_hint.temporary_mode_keys,
                &config.ui_hint.temporary_mode_passthrough_keys,
                ui_hint_overrides(&config.ui_hint.app_configs),
            )?,
        ));
    }
    for (mode, (_, section)) in window_family(config).into_iter().zip(
        config
            .window_modes()
            .into_iter()
            .filter(|(_, section)| section.enabled),
    ) {
        specs.push(ModeSpec::built_in(
            Box::new(mode),
            compile_route(
                &section.bindings,
                &section.inherits,
                section.temporary_mode.as_deref(),
                &section.temporary_mode_keys,
                &section.temporary_mode_passthrough_keys,
                app_overrides(&section.app_configs),
            )?,
        ));
    }
    for spec in &mut specs {
        spec.route.bindings = crate::config::with_control_defaults(
            spec.id().as_str(),
            &spec.route.bindings,
            config.resolved_key_aliases(),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(specs)
}

pub(crate) fn bundled_specs(config: &Config) -> Result<Vec<ModeSpec>, String> {
    bundled_plugins(config)?
        .into_iter()
        .map(|plugin| {
            let id = plugin.id();
            let mut route = match config.plugin_modes.get(id.as_str()) {
                Some(section) => compile_route(
                    &section.bindings,
                    &section.inherits,
                    section.temporary_mode.as_deref(),
                    &section.temporary_mode_keys,
                    &section.temporary_mode_passthrough_keys,
                    app_overrides(&section.app_configs),
                )?,
                None => empty_route(),
            };
            route.bindings = crate::config::with_control_defaults(
                id.as_str(),
                &route.bindings,
                config.resolved_key_aliases(),
            )
            .map_err(|e| e.to_string())?;
            Ok(ModeSpec::plugin(plugin, route))
        })
        .collect()
}

fn compile_route(
    bindings: &Bindings,
    inherits: &[String],
    temporary_mode: Option<&str>,
    temporary_keys: &[String],
    temporary_passthrough: &[String],
    app_overrides: Vec<AppRouteOverride>,
) -> Result<ModeRoute, String> {
    // Disabled layers carry no target, trigger list or passthrough matcher.
    // The input hot path only checks the registry's precompiled empty slot.
    let temporary_mode = temporary_mode.filter(|_| !temporary_keys.is_empty());
    Ok(ModeRoute {
        bindings: bindings.clone(),
        inherits: inherits
            .iter()
            .map(|source| {
                if source == "hotkeys" {
                    Ok(ModeId::idle())
                } else {
                    ModeId::parse_borrowed(source)
                }
            })
            .collect::<Result<Vec<_>, _>>()?,
        temporary_mode: temporary_mode.map(ModeId::parse_borrowed).transpose()?,
        temporary_keys: if temporary_mode.is_some() {
            temporary_keys.to_vec()
        } else {
            Vec::new()
        },
        temporary_passthrough: temporary_passthrough
            .iter()
            .filter(|_| temporary_mode.is_some())
            .map(|key| crate::api::KeyChord::parse(key))
            .collect::<Result<_, _>>()?,
        app_overrides,
    })
}

fn empty_route() -> ModeRoute {
    ModeRoute {
        bindings: Bindings::new(),
        inherits: Vec::new(),
        temporary_mode: None,
        temporary_keys: Vec::new(),
        temporary_passthrough: Vec::new(),
        app_overrides: Vec::new(),
    }
}

fn app_overrides(values: &[AppOverride]) -> Vec<AppRouteOverride> {
    values
        .iter()
        .map(|value| AppRouteOverride {
            pattern: value.bundle_id.clone(),
            bindings: value.bindings.clone(),
        })
        .collect()
}

fn ui_hint_overrides(values: &[UiHintAppOverride]) -> Vec<AppRouteOverride> {
    values
        .iter()
        .map(|value| AppRouteOverride {
            pattern: value.bundle_id.clone(),
            bindings: value.bindings.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ModeId;
    use std::collections::BTreeSet;

    #[test]
    fn tabs_are_available_on_supported_platforms() {
        assert!(
            built_in(&Config::default())
                .iter()
                .any(|mode| mode.id() == ModeId::window_tab())
        );
    }

    #[test]
    fn default_catalog_has_unique_ids_and_all_built_ins() {
        let modes = built_in(&Config::default());
        let ids: Vec<_> = modes.iter().map(|mode| mode.id()).collect();
        let unique: BTreeSet<_> = ids.iter().cloned().collect();
        assert_eq!(ids.len(), unique.len());
        for expected in [
            ModeId::idle(),
            ModeId::normal(),
            ModeId::grid(),
            ModeId::recursive_grid(),
            ModeId::bisect(),
            ModeId::ui_hint(),
        ] {
            assert!(unique.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn enable_flags_only_remove_optional_modes() {
        let mut config = Config::default();
        config.text_select.enabled = false;
        config.quick_switch.enabled = false;
        config.grid.enabled = false;
        config.recursive_grid.enabled = false;
        config.bisect.enabled = false;
        config.ui_hint.enabled = false;
        config.window.enabled = false;
        config.window_quick.enabled = false;
        config.window_editor.enabled = false;
        config.window_restore.enabled = false;
        config.window_tab.enabled = false;
        let ids: Vec<_> = built_in(&config).iter().map(|mode| mode.id()).collect();
        assert_eq!(
            ids,
            vec![ModeId::idle(), ModeId::normal(), ModeId::text_input()]
        );
    }

    #[test]
    fn default_capture_policy_is_owned_by_each_mode() {
        for mode in built_in(&Config::default()) {
            let expected = !matches!(mode.id().as_str(), "idle" | "normal" | "text_input");
            assert_eq!(mode.captures_keyboard(), expected, "{}", mode.id());
        }
    }
}
