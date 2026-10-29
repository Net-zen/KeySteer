// Plain configs: declare each field and its default together. Ordinary serde
// attributes and Rust expressions remain available for exceptional behavior.
macro_rules! config_struct {
    ($(#[$attr:meta])* $vis:vis struct $name:ident { $($fields:tt)* }) => {
        $crate::config::macros::config_struct!(@define [
            #[derive(serde::Deserialize)]
            #[serde(default, deny_unknown_fields)]
            $(#[$attr])*
        ] $vis struct $name { $($fields)* });
    };
    (@define [$($attr:tt)*] $vis:vis struct $name:ident {
        $($(#[$field_attr:meta])* $field:ident: $ty:ty $(= $default:expr)?),* $(,)?
    }) => {
        #[derive(Debug, Clone, PartialEq, serde::Serialize)]
        $($attr)*
        $vis struct $name { $($(#[$field_attr])* pub $field: $ty,)* }
        impl Default for $name {
            fn default() -> Self {
                Self { $($field: $crate::config::macros::config_struct!(@value [Default::default()] $($default)?),)* }
            }
        }
    };
    (@value [$fallback:expr] $default:expr) => { $default };
    (@value [$fallback:expr]) => { $fallback };
}

// Modes add one routing group in place. Its defaults match the targeting modes;
// other modes override only what differs. `omit_empty` affects serialization,
// never defaults. The internal callback also feeds the flat window document.
macro_rules! mode_config {
    ($(#[$attr:meta])* $vis:vis struct $name:ident { $($fields:tt)* }) => {
        $crate::config::macros::mode_config!(@expand
            [config_struct ($(#[$attr])* $vis struct $name)] { $($fields)* });
        $crate::config::macros::mode_config!(@view $name);
    };
    (@view $name:ident) => {
        impl $name {
            pub(crate) fn routing(&self) -> $crate::config::Routing<'_> {
                $crate::config::Routing {
                    inherits: &self.inherits,
                    temporary_mode: self.temporary_mode.as_deref(),
                    temporary_mode_keys: &self.temporary_mode_keys,
                    temporary_mode_passthrough_keys: &self.temporary_mode_passthrough_keys,
                }
            }
        }
    };
    (@expand $output:tt {
        $($(#[$attr:meta])* $field:ident: $ty:ty $(= $default:expr)?,)*
        ..routing { $($routing:tt)* }
        $($rest:tt)*
    }) => {
        $crate::config::macros::mode_config!(@routing { $($routing)* } [$output {
            $($(#[$attr])* $field: $ty $(= $default)?,)*
        } { $($rest)* }]);
    };
    (@routing { omit_empty: all, $($defaults:tt)* } $context:tt) => {
        $crate::config::macros::mode_config!(@routing {
            @skip_inherits "Vec::is_empty";
            @skip_temporary "Option::is_none", "Vec::is_empty";
            $($defaults)*
        } $context);
    };
    (@routing { omit_empty: temporary, $($defaults:tt)* } $context:tt) => {
        $crate::config::macros::mode_config!(@routing {
            @skip_temporary "Option::is_none", "Vec::is_empty";
            $($defaults)*
        } $context);
    };
    (@routing {
        $(@skip_inherits $skip_inherits:literal;)?
        $(@skip_temporary $skip_mode:literal, $skip_keys:literal;)?
        $(inherits: $inherits:expr,)?
        $(temporary_mode: $temporary:expr,)?
        $(temporary_mode_keys: $keys:expr,)?
        $(temporary_mode_passthrough_keys: $passthrough:expr,)?
    } [[$emit:ident ($($header:tt)*)] {$($before:tt)*} {$($after:tt)*}]) => {
        $crate::config::macros::$emit! { $($header)* {
            $($before)*
            $(#[serde(skip_serializing_if = $skip_inherits)])?
            inherits: Vec<String> = $crate::config::macros::config_struct!(
                @value [vec!["hotkeys".into(), "normal".into()]] $($inherits)?),
            $(#[serde(skip_serializing_if = $skip_mode)])?
            temporary_mode: Option<String> = $crate::config::macros::config_struct!(
                @value [Some("normal".into())] $($temporary)?),
            $(#[serde(skip_serializing_if = $skip_keys)])?
            temporary_mode_keys: Vec<String> $(= $keys)?,
            $(#[serde(skip_serializing_if = $skip_keys)])?
            temporary_mode_passthrough_keys: Vec<String> $(= $passthrough)?,
            $($after)*
        } }
    };
}

// One window schema feeds both the shared struct and each strict, flat document.
// Window modes keep their own defaults, including partial lifecycle tables.
macro_rules! window_config {
    (common $vis:vis struct $name:ident;) => {
        $crate::config::macros::window_config!(@schema []
            [config_struct (@define [#[derive(serde::Deserialize)] #[serde(default)]] $vis struct $name)]);
        $crate::config::macros::mode_config!(@view $name);
    };
    ($vis:vis struct $name:ident { common: $common:expr, $($extra:tt)* }) => {
        $crate::config::macros::window_config!(@schema [#[serde(deserialize_with = "read_lifecycle")]]
            [window_config (@mode $vis struct $name [$common] { $($extra)* })]);
    };
    (@schema [$($lifecycle_attr:tt)*] $output:tt) => {
        $crate::config::macros::mode_config!(@expand $output {
            target: Option<$crate::api::window::WindowTarget>,
            screens: $crate::config::window::WindowScreens = $crate::config::window::WindowScreens::Current,
            include_minimized: bool = false,
            enabled: bool = true,
            ..routing { inherits: vec!["hotkeys".into()], }
            number_timeout_ms: u64 = 250,
            border_width: f64 = 3.0,
            ui: $crate::api::LabelUi = $crate::api::LabelUi { font_size: 28, ..Default::default() },
            $($lifecycle_attr)*
            lifecycle: $crate::api::TargetingLifecycle,
            bindings: $crate::config::Bindings,
            app_configs: Vec<$crate::config::AppOverride>,
        });
    };
    (@mode $vis:vis struct $name:ident [$common:expr] {
        $($(#[$extra_attr:meta])* $extra:ident: $extra_ty:ty $(= $extra_default:expr)?),* $(,)?
    } { $($(#[$attr:meta])* $field:ident: $ty:ty $(= $default:expr)?),* $(,)? }) => {
        $crate::config::macros::config_struct!(@define [] $vis struct $name {
            #[serde(flatten)]
            common: $crate::config::window::WindowModeConfig = $common,
            $($(#[$extra_attr])* $extra: $extra_ty $(= $extra_default)?,)*
        });
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                #[derive(serde::Deserialize)]
                #[serde(default, deny_unknown_fields)]
                struct Document {
                    $($(#[$attr])* $field: $ty,)*
                    $($(#[$extra_attr])* $extra: $extra_ty,)*
                }
                fn read_lifecycle<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<$crate::api::TargetingLifecycle, D::Error> {
                    $crate::config::window::read_lifecycle(deserializer, $name::default().common.lifecycle)
                }
                impl Default for Document {
                    fn default() -> Self {
                        let mode = $name::default();
                        Self { $($field: mode.common.$field,)* $($extra: mode.$extra,)* }
                    }
                }
                let doc = <Document as serde::Deserialize>::deserialize(deserializer)?;
                Ok(Self {
                    common: $crate::config::window::WindowModeConfig { $($field: doc.$field,)* },
                    $($extra: doc.$extra,)*
                })
            }
        }
        impl std::ops::Deref for $name {
            type Target = $crate::config::window::WindowModeConfig;
            fn deref(&self) -> &Self::Target { &self.common }
        }
        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Self::Target { &mut self.common }
        }
    };
}

// One inventory for read-only routing, mutable key normalization and validation.
// The projection borrows fields directly, including window modes' common data.
macro_rules! builtin_mode_fields {
    ($config:ident, $borrow:tt, |$mode:ident| $value:expr) => {
        $crate::config::macros::builtin_mode_fields!(@fields $config, $borrow, |$mode| $value;
            normal, quick_switch, text_input, text_select, grid, bisect,
            recursive_grid, ui_hint, window.common, window_quick.common,
            window_editor.common, window_restore.common, window_tab.common)
    };
    (@fields $config:ident, $borrow:tt, |$mode:ident| $value:expr;
        $($name:ident $(.$field:ident)*),+ $(,)?) => {
        [$({
            let $mode = $crate::config::macros::builtin_mode_fields!(
                @borrow $borrow $config.$name$(.$field)*);
            (stringify!($name), $value)
        }),+]
    };
    (@borrow [$($borrow:tt)*] $place:expr) => { $($borrow)* $place };
}

pub(super) use {builtin_mode_fields, config_struct, mode_config, window_config};
