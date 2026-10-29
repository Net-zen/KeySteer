//! The only assembly point for built-in modes.
//!
//! Configuration DTOs stop here. Each mode receives only its own immutable,
//! strongly typed settings and remains unaware of TOML or the application
//! configuration document.

use crate::api::{KeyChord, Mode, ModeId, Plugin};
use crate::config::{AppOverride, Bindings, Config, Routing};
use crate::modes::targeting::{
    BisectGeometry, GridGeometry, LayerSettings, Layout, LetterGeometry, RecursiveGeometry,
};
use crate::modes::{self, GridMode, HintMode, IdleMode, NormalMode};
use crate::plugins::BundledSettings;

use super::runtime::{AppRouteOverride, ModeRoute, ModeSpec};

impl From<&crate::config::GridLayer> for LayerSettings {
    fn from(layer: &crate::config::GridLayer) -> Self {
        Self {
            depth: layer.depth,
            grid_cols: layer.grid_cols,
            grid_rows: layer.grid_rows,
            keys: layer.keys.clone(),
        }
    }
}

fn grid_layout(rows: u32, cols: u32, keys: &str) -> Layout {
    Layout {
        rows: rows as usize,
        cols: cols as usize,
        keys: keys.chars().collect(),
    }
}

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
        overlap_cycle_key: KeyChord::parse(&config.ui_hint.overlap_cycle_key)
            .unwrap_or_else(|error| panic!("hint requires validated overlap key: {error}")),
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
    use modes::targeting::TargetingController;
    let Some(targeting) = &config.normal.targeting else {
        return Box::new(normal(config));
    };
    let geometry = targeting.resolve(config);
    let layers: Vec<_> = geometry.layers.iter().map(LayerSettings::from).collect();
    let grid = GridGeometry::new(
        grid_layout(geometry.grid_rows, geometry.grid_cols, geometry.keys),
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
        let mut controls = controls.clone();
        crate::config::apply_control_defaults(name, &mut controls, config.resolved_key_aliases())
            .map_err(|e| e.to_string())?;
        for (key, binding) in controls {
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
    use modes::targeting::TargetingController;
    let settings = &config.grid;
    let ui = &settings.ui;
    GridMode::new(
        TargetingController::new(
            GridGeometry::new(
                grid_layout(
                    settings.grid_rows.max(1),
                    settings.grid_cols.max(1),
                    &settings.keys,
                ),
                &[],
                settings.max_depth,
            ),
            settings.max_depth,
            settings.cursor_follow_selection,
            settings.lifecycle.clone(),
        ),
        crate::api::presentation::GridStyle {
            label: ui.label.compile(),
            matched_background_color: ui
                .matched_background_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
            matched_border_color: ui
                .matched_border_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
        },
    )
}

#[doc(hidden)]
pub fn recursive_grid(config: &Config) -> GridMode<RecursiveGeometry> {
    use modes::targeting::TargetingController;
    let settings = &config.recursive_grid;
    let ui = &settings.ui;
    let layers: Vec<_> = settings.layers.iter().map(LayerSettings::from).collect();
    GridMode::new(
        TargetingController::new(
            RecursiveGeometry {
                grid: GridGeometry::new(
                    grid_layout(
                        settings.grid_rows.max(1),
                        settings.grid_cols.max(1),
                        &settings.keys,
                    ),
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
            label: ui.label.compile(),
            line_width: ui.line_width,
            line_color: ui
                .line_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
            highlight_color: ui
                .highlight_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
            label_background: ui.label_background,
            label_background_color: ui
                .label_background_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
            label_char: ui.label_char.clone(),
            label_min_font_size: ui.label_min_font_size,
            label_autohide_multiplier: ui.label_autohide_multiplier,
            sub_key_preview: ui.sub_key_preview,
            sub_key_preview_font_size: ui.sub_key_preview_font_size,
            sub_key_preview_text_color: ui
                .sub_key_preview_text_color
                .as_ref()
                .map(crate::api::theme::CompiledColor::from),
            sub_key_preview_autohide_multiplier: ui.sub_key_preview_autohide_multiplier,
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
    HintMode::new(
        hint_settings(config),
        config
            .ui_hint
            .hint_characters
            .chars()
            .filter_map(|character| {
                crate::api::Key::new(character.encode_utf8(&mut [0; 4]))
                    .ok()?
                    .as_char()
            })
            .collect(),
    )
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
                crate::api::Key::new(ch.encode_utf8(&mut [0; 4]))
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
    let mut alphabet: Vec<_> = config.text_select.hint_characters.chars().collect();
    let mut bindings = Vec::new();
    for (keys, binding) in &config.text_select.bindings {
        for key in keys.split_whitespace() {
            let Ok(chord) = KeyChord::parse_with_aliases(key, config.resolved_key_aliases()) else {
                continue;
            };
            if !matches!(binding, crate::api::Binding::Disabled)
                && let [key] = chord.keys()
                && let Some(character) = key.as_char()
            {
                alphabet.retain(|&c| c != character);
            }
            bindings.push((chord, std::sync::Arc::new(binding.clone())));
        }
    }
    modes::text_select::TextSelectMode::new(modes::text_select::Settings {
        hint,
        alphabet,
        ui: config.text_select.ui.compile(),
        lifecycle: config.text_select.lifecycle.clone(),
        bindings,
    })
}

#[doc(hidden)]
#[allow(dead_code)]
pub fn built_in(config: &Config) -> Vec<Box<dyn Mode>> {
    let mut catalog = Vec::with_capacity(ModeId::BUILT_IN.len());
    catalog.extend(
        built_in_factories(config, CatalogOrder::Instances).map(|factory| (factory.create)(config)),
    );
    catalog.extend(
        window_family(config)
            .into_iter()
            .map(|mode| Box::new(mode) as Box<dyn Mode>),
    );
    catalog
}

struct ModeFactory {
    enabled: bool,
    create: fn(&Config) -> Box<dyn Mode>,
    route: fn(&Config) -> Result<ModeRoute, String>,
}

enum CatalogOrder {
    Instances,
    Runtime,
}

// Each factory is declared once. Merely enumerating it does not construct a
// mode, clone bindings or compile routes; callers perform only the work they need.
fn built_in_factories(config: &Config, order: CatalogOrder) -> impl Iterator<Item = ModeFactory> {
    let idle = ModeFactory {
        enabled: true,
        create: |_| Box::new(IdleMode::new()),
        route: |config| compile_app_route(&config.hotkeys, Routing::default(), &config.app_configs),
    };
    let normal = ModeFactory {
        enabled: true,
        create: normal_instance,
        route: |config| {
            compile_app_route(
                &config.normal.bindings,
                config.normal.routing(),
                &config.normal.app_configs,
            )
        },
    };
    let text_input = ModeFactory {
        enabled: true,
        create: |_| Box::new(modes::text_input::TextInputMode::new()),
        route: |config| compile_route(&config.text_input.bindings, config.text_input.routing()),
    };
    let quick_switch = ModeFactory {
        enabled: config.quick_switch.enabled,
        create: |config| Box::new(quick_switch(config)),
        route: |config| compile_route(&config.quick_switch.bindings, config.quick_switch.routing()),
    };
    let text_select = ModeFactory {
        enabled: config.text_select.enabled,
        create: |config| Box::new(text_select(config)),
        route: |config| {
            compile_hint_route(
                &config.text_select.bindings,
                config.text_select.routing(),
                &config.ui_hint.overlap_cycle_key,
            )
        },
    };
    let shared = [
        ModeFactory {
            enabled: config.grid.enabled,
            create: |config| Box::new(grid(config)),
            route: |config| {
                compile_app_route(
                    &config.grid.bindings,
                    config.grid.routing(),
                    &config.grid.app_configs,
                )
            },
        },
        ModeFactory {
            enabled: config.recursive_grid.enabled,
            create: |config| Box::new(recursive_grid(config)),
            route: |config| {
                compile_app_route(
                    &config.recursive_grid.bindings,
                    config.recursive_grid.routing(),
                    &config.recursive_grid.app_configs,
                )
            },
        },
        ModeFactory {
            enabled: config.bisect.enabled,
            create: |config| Box::new(bisect(config)),
            route: |config| {
                compile_app_route(
                    &config.bisect.bindings,
                    config.bisect.routing(),
                    &config.bisect.app_configs,
                )
            },
        },
        ModeFactory {
            enabled: config.ui_hint.enabled,
            create: |config| Box::new(hint(config)),
            route: compile_ui_hint_route,
        },
    ];
    // The historical entry points differ only around this shared group.
    // Assemble those positions directly, before invoking any factory.
    let mut head = match order {
        CatalogOrder::Instances => [idle, normal, text_input, quick_switch, text_select],
        CatalogOrder::Runtime => [quick_switch, text_select, text_input, idle, normal],
    }
    .into_iter();
    // Instance enumeration places Text Select after the shared modes.
    let tail = match order {
        CatalogOrder::Instances => head.next_back(),
        CatalogOrder::Runtime => None,
    };
    head.chain(shared)
        .chain(tail)
        .filter(|factory| factory.enabled)
}

fn window_common(
    config: &Config,
    kind: modes::window::WindowKind,
) -> &crate::config::WindowModeConfig {
    use modes::window::WindowKind as K;
    match kind {
        K::Move => &config.window.common,
        K::Quick => &config.window_quick.common,
        K::Editor => &config.window_editor.common,
        K::Restore => &config.window_restore.common,
        K::Tab => &config.window_tab.common,
    }
}

fn window_settings(config: &Config, kind: modes::window::WindowKind) -> modes::window::Settings {
    use modes::window::WindowKind as K;
    let common = window_common(config, kind);
    let card = if kind == K::Editor {
        config.window_editor.card.apply(&config.window.card)
    } else {
        config.window.card.clone()
    };
    let mut multi_bindings = config.window.multi_select.bindings.clone();
    crate::config::apply_control_defaults(
        "window.multi_select",
        &mut multi_bindings,
        config.resolved_key_aliases(),
    )
    .unwrap_or_else(|error| panic!("window settings require validated configuration: {error}"));
    modes::window::Settings {
        multi_bindings: multi_bindings
            .iter()
            .flat_map(|(keys, binding)| {
                keys.split_whitespace().map(|key| {
                    (
                        crate::api::KeyChord::parse_with_aliases(
                            key,
                            config.resolved_key_aliases(),
                        )
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
            .filter(|&kind| window_common(config, kind).enabled)
            .map(|kind| (kind, window_settings(config, kind))),
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

/// Build the production catalog, pairing each instance with its route here.
pub(crate) fn built_in_specs(config: &Config) -> Result<Vec<ModeSpec>, String> {
    let mut specs = Vec::with_capacity(ModeId::BUILT_IN.len());
    for factory in built_in_factories(config, CatalogOrder::Runtime) {
        let mode = (factory.create)(config);
        specs.push(ModeSpec::built_in(mode, (factory.route)(config)?));
    }
    for mode in window_family(config) {
        let id = mode.id();
        let (_, section) = config
            .window_modes()
            .into_iter()
            .find(|(name, _)| *name == id.as_str())
            .ok_or_else(|| format!("missing configuration for window mode {id}"))?;
        specs.push(ModeSpec::built_in(
            Box::new(mode),
            compile_app_route(&section.bindings, section.routing(), &section.app_configs)?,
        ));
    }
    for spec in &mut specs {
        crate::config::apply_control_defaults(
            spec.id().as_str(),
            &mut spec.route.bindings,
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
                Some(section) => {
                    compile_app_route(&section.bindings, section.routing(), &section.app_configs)?
                }
                None => ModeRoute::default(),
            };
            crate::config::apply_control_defaults(
                id.as_str(),
                &mut route.bindings,
                config.resolved_key_aliases(),
            )
            .map_err(|e| e.to_string())?;
            Ok(ModeSpec::plugin(plugin, route))
        })
        .collect()
}

fn compile_route(bindings: &Bindings, routing: Routing<'_>) -> Result<ModeRoute, String> {
    let Routing {
        inherits,
        temporary_mode,
        temporary_mode_keys: temporary_keys,
        temporary_mode_passthrough_keys: temporary_passthrough,
    } = routing;
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
        temporary_keys: temporary_mode
            .map(|_| {
                temporary_keys
                    .iter()
                    .map(|key| KeyChord::parse(key))
                    .collect()
            })
            .transpose()?,
        temporary_passthrough: temporary_passthrough
            .iter()
            .filter(|_| temporary_mode.is_some())
            .map(|key| crate::api::KeyChord::parse(key))
            .collect::<Result<_, _>>()?,
        app_overrides: Vec::new(),
    })
}

fn compile_app_override(bundle_id: &str, bindings: &Bindings) -> AppRouteOverride {
    AppRouteOverride {
        pattern: bundle_id.to_owned(),
        bindings: bindings.clone(),
    }
}

fn compile_app_route(
    bindings: &Bindings,
    routing: Routing<'_>,
    app_configs: &[AppOverride],
) -> Result<ModeRoute, String> {
    let mut route = compile_route(bindings, routing)?;
    route.app_overrides = app_configs
        .iter()
        .map(|value| compile_app_override(&value.bundle_id, &value.bindings))
        .collect();
    Ok(route)
}

fn compile_ui_hint_route(config: &Config) -> Result<ModeRoute, String> {
    // UI Hint overrides also contain scan strategy; routes only use their bindings.
    let hint = &config.ui_hint;
    let mut route = compile_hint_route(&hint.bindings, hint.routing(), &hint.overlap_cycle_key)?;
    route.app_overrides = hint
        .app_configs
        .iter()
        .map(|value| compile_app_override(&value.bundle_id, &value.bindings))
        .collect();
    Ok(route)
}

// UI Hint and Text Select reserve the same overlap modifier before routes
// reach the generic keymap builder. Side-agnostic modifiers match either side.
fn compile_hint_route(
    bindings: &Bindings,
    routing: Routing<'_>,
    overlap_key: &str,
) -> Result<ModeRoute, String> {
    let mut route = compile_route(bindings, routing)?;
    if let Some(triggers) = &mut route.temporary_keys
        && let Ok(overlap) = KeyChord::parse(overlap_key)
    {
        triggers.retain(|trigger| {
            !overlap.activation_matches(trigger.activation_key())
                && !trigger.activation_matches(overlap.activation_key())
        });
        if triggers.is_empty() {
            route.temporary_keys = None;
            route.temporary_mode = None;
            route.temporary_passthrough = Vec::new();
        }
    }
    Ok(route)
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
        let expected: BTreeSet<_> = ModeId::BUILT_IN
            .map(|name| ModeId::new(name).unwrap())
            .into_iter()
            .collect();
        assert_eq!(unique, expected);
        let specs = built_in_specs(&Config::default()).unwrap();
        assert_eq!(specs.len(), expected.len());
        assert_eq!(
            specs.iter().map(ModeSpec::id).collect::<BTreeSet<_>>(),
            expected
        );

        // Both entry points keep their established order.
        assert_eq!(
            ids.iter().map(ModeId::as_str).collect::<Vec<_>>(),
            [
                "idle",
                "normal",
                "text_input",
                "quick_switch",
                "grid",
                "recursive_grid",
                "bisect",
                "ui_hint",
                "text_select",
                "window",
                "window_quick",
                "window_editor",
                "window_restore",
                "window_tab",
            ]
        );
        assert_eq!(
            specs
                .iter()
                .map(|spec| spec.id().as_str().to_owned())
                .collect::<Vec<_>>(),
            [
                "quick_switch",
                "text_select",
                "text_input",
                "idle",
                "normal",
                "grid",
                "recursive_grid",
                "bisect",
                "ui_hint",
                "window",
                "window_quick",
                "window_editor",
                "window_restore",
                "window_tab",
            ]
        );
    }

    #[test]
    fn instance_enumeration_does_not_compile_routes() {
        let mut config = Config::default();
        config.grid.inherits = vec!["not a valid mode id".into()];
        assert!(built_in_specs(&config).is_err());
        assert_eq!(
            built_in(&config)
                .iter()
                .map(|mode| mode.id())
                .collect::<Vec<_>>(),
            built_in(&Config::default())
                .iter()
                .map(|mode| mode.id())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn runtime_catalog_stops_before_constructing_later_modes_on_route_error() {
        let mut config = Config::default();
        config.quick_switch.inherits = vec!["invalid quick switch".into()];
        config.text_select.inherits = vec!["invalid text select".into()];
        // Later Text Select and window constructors would panic on these settings.
        config.ui_hint.search_copy_keys.push("invalid+".into());
        config.window_quick.split_ratios.clear();
        assert_eq!(
            built_in_specs(&config).err().as_deref(),
            Some("invalid mode id: \"invalid quick switch\"")
        );
        let _ = config.ui_hint.search_copy_keys.pop();
        config.quick_switch.enabled = false;
        assert_eq!(
            built_in_specs(&config).err().as_deref(),
            Some("invalid mode id: \"invalid text select\"")
        );
    }

    #[test]
    fn enable_flags_only_remove_optional_modes() {
        let defaults = Config::default();
        let expected_instances: Vec<_> = built_in(&defaults).iter().map(|mode| mode.id()).collect();
        let expected_specs: Vec<_> = built_in_specs(&defaults)
            .unwrap()
            .iter()
            .map(ModeSpec::id)
            .collect();
        let document = serde_json::to_value(defaults).unwrap();
        for (name, section) in document.as_object().unwrap() {
            if section.get("bindings").is_none() || section.get("enabled").is_none() {
                continue;
            }
            for enabled in [false, true] {
                let mut source = format!("[{name}]\nenabled = {enabled}");
                if !enabled {
                    // Disabled modes must not compile their routes.
                    source.push_str("\ninherits = ['invalid disabled mode']");
                }
                let config = Config::parse(&source).unwrap();
                let specs = built_in_specs(&config).unwrap();
                assert_eq!(
                    specs.iter().map(ModeSpec::id).collect::<Vec<_>>(),
                    expected_specs
                        .iter()
                        .filter(|id| enabled || id.as_str() != name)
                        .cloned()
                        .collect::<Vec<_>>(),
                    "{name}"
                );
                assert_eq!(
                    built_in(&config)
                        .iter()
                        .map(|mode| mode.id())
                        .collect::<Vec<_>>(),
                    expected_instances
                        .iter()
                        .filter(|id| enabled || id.as_str() != name)
                        .cloned()
                        .collect::<Vec<_>>(),
                    "{name}"
                );
            }
        }
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
        assert_eq!(
            built_in_specs(&config)
                .unwrap()
                .iter()
                .map(ModeSpec::id)
                .collect::<Vec<_>>(),
            vec![ModeId::text_input(), ModeId::idle(), ModeId::normal()]
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
