use super::Bindings;
use super::macros::{config_struct, window_config};
use crate::api::window::WindowAction as W;
use crate::api::{Binding, LifecycleAction, ModeId, TargetingLifecycle};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SplitRatio {
    Fraction(String),
    Decimal(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowScreens {
    #[default]
    Current,
    All,
}

// Shared schema, not binding inheritance. Each mode owns a separate value.
window_config!(common pub struct WindowModeConfig;);

// Sparse window lifecycles inherit the selected mode's finish/click defaults.
pub(super) fn read_lifecycle<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
    defaults: TargetingLifecycle,
) -> Result<TargetingLifecycle, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Partial {
        after_finish: Option<LifecycleAction>,
        after_click: Option<LifecycleAction>,
    }
    let partial = Partial::deserialize(deserializer)?;
    Ok(TargetingLifecycle {
        after_finish: partial.after_finish.unwrap_or(defaults.after_finish),
        after_click: partial.after_click.unwrap_or(defaults.after_click),
    })
}

config_struct! {
    pub struct MultiSelect {
        bindings: Bindings = {
            let mut bindings = multi_select_control_defaults();
            bindings.extend(super::builtin_bindings(&[
                ("ctrl enter", "window_multi_confirm"),
                ("primary+h", "arrow_left"),
                ("primary+l", "arrow_right"),
            ]));
            bindings
        },
    }
}

pub(super) fn multi_select_control_defaults() -> Bindings {
    super::builtin_bindings(&[
        ("esc", "window_multi_cancel"),
        ("backspace", "send backspace"),
        ("delete", "send delete"),
        ("arrow_left", "send arrow_left"),
        ("arrow_right", "send arrow_right"),
        ("home", "send home"),
        ("end", "send end"),
    ])
}

window_config!(pub struct Window {
    common: {
        let mut common = editing_common(
            ModeId::idle(),
            &[
                ("h", W::Left),
                ("j", W::Down),
                ("k", W::Up),
                ("l", W::Right),
                ("s", W::Size),
                ("d", W::NextScreen),
                ("f", W::ToggleMaximize),
                ("shift+f", W::ToggleMinimize),
                ("c", W::Center),
                ("x", W::Close),
            ],
            &[
                ("g", ModeId::grid()),
                ("primary+f", ModeId::recursive_grid()),
                ("a", ModeId::window_quick()),
                ("e", ModeId::window_editor()),
                ("t", ModeId::window_tab()),
                ("r", ModeId::window_restore()),
            ],
        );
        common.screens = WindowScreens::All;
        common.temporary_mode_passthrough_keys = vec!["ctrl".into(), "ctrl+x".into()];
        common.bindings.extend([
            ("ctrl".into(), Binding::Window(W::MultiSelect)),
            ("ctrl+x".into(), Binding::Window(W::ClearMulti)),
        ]);

        common
    },
    multi_select: MultiSelect,
    card: crate::api::style::WindowCardUi,
    move_step: f64 = 20.0,
    move_speed: f64 = 600.0,
    resize_step: f64 = 20.0,
    resize_speed: f64 = 500.0,
});
window_config!(pub struct WindowQuick {
    common: {
        use crate::api::Direction::*;
        editing_common(
            ModeId::window(),
            &[
                ("h", W::Navigate(Left)),
                ("j", W::Navigate(Down)),
                ("k", W::Navigate(Up)),
                ("l", W::Navigate(Right)),
            ],
            &[
                ("e", ModeId::window_editor()),
                ("r", ModeId::window_restore()),
            ],
        )
    },
    split_ratios: Vec<SplitRatio> = ["1/4", "1/3", "1/2", "2/3", "3/4"]
        .map(|v| SplitRatio::Fraction(v.into()))
        .to_vec(),
    gap: f64 = 0.0,
});
config_struct! {
    pub struct EditorCard {
        position: [String; 4] = ["0%", "50%", "100%", "50%"].map(String::from),
        position_mode: crate::api::style::WindowCardPositionMode =
            crate::api::style::WindowCardPositionMode::Window,
    }
}

impl EditorCard {
    pub fn apply(
        &self,
        shared: &crate::api::style::WindowCardUi,
    ) -> crate::api::style::WindowCardUi {
        crate::api::style::WindowCardUi {
            position: self.position.clone(),
            position_mode: self.position_mode,
            ..shared.clone()
        }
    }
}
window_config!(pub struct WindowEditor {
    common: {
        use crate::api::Direction::*;
        let mut common = editing_common(
            ModeId::window(),
            &[
                ("h", W::Navigate(Left)),
                ("j", W::Navigate(Down)),
                ("k", W::Navigate(Up)),
                ("l", W::Navigate(Right)),
                ("shift+h", W::Split(Left)),
                ("shift+j", W::Split(Down)),
                ("shift+k", W::Split(Up)),
                ("shift+l", W::Split(Right)),
                ("ctrl+h", W::Ratio(Left)),
                ("ctrl+j", W::Ratio(Down)),
                ("ctrl+k", W::Ratio(Up)),
                ("ctrl+l", W::Ratio(Right)),
                ("x", W::RemoveRegion),
                ("ctrl+s", W::SaveLayout),
            ],
            &[("r", ModeId::window_restore())],
        );
        common.bindings.extend(editor_control_defaults());
        common
    },
    card: EditorCard,
    resize_step: f64 = 20.0,
    resize_speed: f64 = 500.0,
    gap: f64 = 0.0,
});
pub(super) fn editor_control_defaults() -> Bindings {
    Bindings::from([("`".into(), Binding::Window(W::AreaNumber))])
}

window_config!(pub struct WindowRestore {
    common: {
        let mut common = common(
            ModeId::window(),
            [
                ("enter", W::Confirm),
                ("x", W::DeletePreset),
            ],
            &[],
        );
        common.bindings.extend(restore_control_defaults());
        common.lifecycle.after_finish = LifecycleAction::Mode(ModeId::window_editor());
        common
    },
    gap: f64 = 0.0,
});
pub(super) fn restore_control_defaults() -> Bindings {
    Bindings::from([
        ("page_up".into(), Binding::Window(W::PreviousPage)),
        ("page_down".into(), Binding::Window(W::NextPage)),
    ])
}

window_config!(pub struct WindowTab {
    common: {
        use crate::api::Direction::{Down, Left, Right, Up};
        let mut common = common(
            ModeId::window(),
            [
                ("t", W::TabEnd),
                ("~", W::TabPrefix),
                ("space", W::TabSeparator),
                ("d", W::TabRemove),
                ("x", W::TabDissolve),
                ("tab", W::TabNext),
                ("shift+tab", W::TabPrevious),
                ("z", W::Undo),
                ("shift+z", W::Redo),
                ("ctrl+s", W::SaveLayout),
            ],
            &[("r", ModeId::window_restore())],
        );
        common
            .bindings
            .insert("esc".into(), Binding::Mode(ModeId::idle()));
        for (key, direction) in [("h", Left), ("j", Down), ("k", Up), ("l", Right)] {
            common.bindings.insert(key.into(), Binding::Move(direction));
        }
        common
    },
});

// Movement/layout modes share selection, audio, undo and reset controls.
fn editing_common(
    back: ModeId,
    actions: &[(&str, W)],
    modes: &[(&str, ModeId)],
) -> WindowModeConfig {
    common(
        back,
        actions.iter().copied().chain([
            ("tab", W::Select),
            ("shift+tab", W::SelectPrevious),
            ("v+j", W::VolumeDown),
            ("v+k", W::VolumeUp),
            ("v+m", W::VolumeMute),
            ("shift+v+m", W::SystemVolumeMute),
            ("v+h", W::AudioPrevious),
            ("v+l", W::AudioNext),
            ("shift+v+j", W::SystemVolumeDown),
            ("shift+v+k", W::SystemVolumeUp),
            ("shift+v+h", W::SystemAudioPrevious),
            ("shift+v+l", W::SystemAudioNext),
            ("z", W::Undo),
            ("shift+z", W::Redo),
            ("shift+c", W::ResetInitial),
        ]),
        modes,
    )
}

fn common<'a>(
    back: ModeId,
    actions: impl IntoIterator<Item = (&'a str, W)>,
    modes: &[(&str, ModeId)],
) -> WindowModeConfig {
    let mut common = WindowModeConfig::default();
    common.bindings.insert("?".into(), Binding::KeyHelp);
    common.bindings.extend(
        actions
            .into_iter()
            .map(|(key, action)| (key.into(), Binding::Window(action))),
    );
    common.bindings.extend(
        modes
            .iter()
            .cloned()
            .chain([("q", back), ("primary+q", ModeId::idle())])
            .map(|(key, mode)| (key.into(), Binding::Mode(mode))),
    );
    common
}

impl WindowQuick {
    pub(crate) fn split_ratio_ticks(&self) -> Vec<crate::api::window_layout::RatioTick> {
        self.parsed_split_ratios()
            .unwrap_or_else(|error| {
                panic!("window settings require validated configuration: {error}")
            })
            .into_iter()
            .map(|value| {
                let labels = self
                    .split_ratios
                    .iter()
                    .filter_map(|source| {
                        let (number, label) = match source {
                            SplitRatio::Decimal(number) => (*number, number.to_string()),
                            SplitRatio::Fraction(label) => {
                                let (a, b) =
                                    label.trim().split_once('/').unwrap_or((label.trim(), "1"));
                                (
                                    a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?,
                                    label.clone(),
                                )
                            }
                        };
                        (number == value).then_some(label)
                    })
                    .collect::<Vec<_>>();
                crate::api::window_layout::RatioTick {
                    value,
                    label: labels.join(" / "),
                }
            })
            .chain([crate::api::window_layout::RatioTick {
                value: 1.0,
                label: "1".into(),
            }])
            .collect()
    }

    pub(crate) fn parsed_split_ratios(&self) -> Result<Vec<f64>, &'static str> {
        const ERROR: &str = "window_quick.split_ratios must be a non-empty array of fractions such as \"1/4\" or decimal numbers, each finite, greater than 0 and less than 1";
        let mut values = Vec::with_capacity(self.split_ratios.len());
        for ratio in &self.split_ratios {
            let value = match ratio {
                SplitRatio::Decimal(value) => *value,
                SplitRatio::Fraction(text) => match text.trim().split_once('/') {
                    Some((numerator, denominator)) => {
                        let numerator = numerator.trim().parse::<u32>().map_err(|_| ERROR)?;
                        let denominator = denominator.trim().parse::<u32>().map_err(|_| ERROR)?;
                        if numerator == 0 || numerator >= denominator {
                            return Err(ERROR);
                        }
                        f64::from(numerator) / f64::from(denominator)
                    }
                    None => text.trim().parse::<f64>().map_err(|_| ERROR)?,
                },
            };
            if !value.is_finite() || value <= 0.0 || value >= 1.0 {
                return Err(ERROR);
            }
            values.push(value);
        }
        if values.is_empty() {
            return Err(ERROR);
        }
        values.sort_unstable_by(f64::total_cmp);
        values.dedup();
        Ok(values)
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    #[test]
    fn window_scope_defaults_and_flat_mode_configuration() {
        let default = Window::default();
        assert_eq!(default.screens, WindowScreens::All);
        assert!(!default.include_minimized);
        let window: Window = toml::from_str("").unwrap();
        assert_eq!(window.screens, WindowScreens::All);
        for common in [
            WindowQuick::default().common,
            WindowEditor::default().common,
            WindowRestore::default().common,
            WindowTab::default().common,
            toml::from_str::<WindowQuick>("").unwrap().common,
            toml::from_str::<WindowEditor>("").unwrap().common,
            toml::from_str::<WindowRestore>("").unwrap().common,
            toml::from_str::<WindowTab>("").unwrap().common,
        ] {
            assert_eq!(common.screens, WindowScreens::Current);
            assert!(!common.include_minimized);
        }
        let editor: WindowEditor =
            toml::from_str("screens = 'all'\ninclude_minimized = true").unwrap();
        assert_eq!(editor.screens, WindowScreens::All);
        assert!(editor.include_minimized);
        assert!(toml::from_str::<WindowTab>("screens = 'unknown'").is_err());
        let tabs: WindowTab = toml::from_str("screens = 'all'").unwrap();
        assert_eq!(tabs.screens, WindowScreens::All);
        assert!(!tabs.include_minimized);
    }
}
