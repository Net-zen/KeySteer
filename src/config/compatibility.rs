//! Retired configuration fields, handled once before typed deserialization.
//!
//! The browser imports the same rules. Retired fields, binding names and old
//! table shapes disappear here; typed settings and runtime code use current data.

use serde::Deserialize;

use super::ConfigError;

struct Rule {
    sections: &'static [&'static [&'static str]],
    ignore: &'static [&'static str],
    reject: &'static [(&'static str, &'static str)],
    copy: &'static [(&'static str, CopyField)],
    reject_values: &'static [(&'static str, &'static str)],
    transform: Option<Transform>,
}

struct CopyField {
    value_type: ValueType,
    targets: &'static [&'static str],
    existing_parent_only: bool,
    warning: Option<&'static str>,
}

enum ValueType {
    Boolean,
}

enum Transform {
    SearchEditKeys,
    SearchCopyKeys,
}

#[derive(Default)]
struct Pending {
    copies: Vec<(&'static str, toml::Value, bool)>,
    warnings: Vec<String>,
}

include!(concat!(env!("OUT_DIR"), "/compatibility_rules.rs"));

// Stack-only path segments; format a complete path only for an actual error.
struct ConfigPath<'a> {
    parent: Option<&'a ConfigPath<'a>>,
    segment: &'a dyn std::fmt::Display,
}
impl std::fmt::Display for ConfigPath<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(parent) = self.parent {
            write!(f, "{parent}.")?;
        }
        self.segment.fmt(f)
    }
}

pub(super) fn normalize(value: &mut toml::Value) -> Result<Vec<String>, ConfigError> {
    let mut pending = Pending::default();
    for rule in RULES {
        for section in rule.sections {
            visit(value, section, None, rule, &mut pending)?;
        }
    }
    // Apply copies in rule order, preserving explicit current values and giving
    // the newer all-axis setting priority over the older global setting.
    for (path, source, existing_parent_only) in pending.copies {
        copy_if_missing(value, path, source, existing_parent_only)?;
    }
    Ok(pending.warnings)
}

fn visit(
    value: &mut toml::Value,
    remaining: &[&str],
    section: Option<&ConfigPath<'_>>,
    rule: &Rule,
    pending: &mut Pending,
) -> Result<(), ConfigError> {
    if remaining.is_empty() {
        match rule.transform {
            Some(Transform::SearchEditKeys) => normalize_edit_keys(value)?,
            Some(Transform::SearchCopyKeys) => {
                if let Some(keys) = value.as_array_mut()
                    && keys.len() == 5
                {
                    keys.truncate(4);
                }
            }
            None => {}
        }
        let Some(table) = value.as_table_mut() else {
            return Ok(());
        };
        for &(field, replacement) in rule.reject {
            if table.contains_key(field) {
                return Err(ConfigError::Invalid(format!(
                    "{}.{field} was removed; use {replacement}",
                    section.map_or_else(String::new, ToString::to_string)
                )));
            }
        }
        for &field in rule.ignore {
            table.remove(field);
        }
        for &(field, ref migration) in rule.copy {
            if let Some(source) = table.remove(field) {
                let valid = match migration.value_type {
                    ValueType::Boolean => source.is_bool(),
                };
                if !valid {
                    return Err(ConfigError::Invalid(format!(
                        "{}.{field} must be a boolean",
                        section.map_or_else(String::new, ToString::to_string)
                    )));
                }
                for &target in migration.targets {
                    pending
                        .copies
                        .push((target, source.clone(), migration.existing_parent_only));
                }
                pending.warnings.extend(migration.warning.map(String::from));
            }
        }
        if !rule.reject_values.is_empty() {
            for (key, value) in table {
                check_binding(value, section, key, rule.reject_values)?;
            }
        }
        return Ok(());
    }
    let (segment, rest) = remaining.split_first().unwrap_or((&"", &[]));
    match value {
        toml::Value::Table(table) => {
            for (key, child) in table {
                if *segment == "*" || *segment == key {
                    let path = ConfigPath {
                        parent: section,
                        segment: key,
                    };
                    visit(child, rest, Some(&path), rule, pending)?;
                }
            }
        }
        toml::Value::Array(items) if *segment == "*" => {
            for (index, child) in items.iter_mut().enumerate() {
                let path = ConfigPath {
                    parent: section,
                    segment: &index,
                };
                visit(child, rest, Some(&path), rule, pending)?;
            }
        }
        _ => {} // Leave malformed sections for the normal schema validation.
    }
    Ok(())
}

fn copy_if_missing(
    value: &mut toml::Value,
    path: &str,
    source: toml::Value,
    existing_parent_only: bool,
) -> Result<(), ConfigError> {
    let Some(table) = value.as_table_mut() else {
        return Err(ConfigError::Invalid(format!(
            "parent of {path} must be a table"
        )));
    };
    if let Some((segment, rest)) = path.split_once('.') {
        if existing_parent_only && !table.contains_key(segment) {
            return Ok(());
        }
        let child = table
            .entry(segment)
            .or_insert_with(|| toml::Value::Table(Default::default()));
        copy_if_missing(child, rest, source, existing_parent_only)
    } else {
        table.entry(path).or_insert(source);
        Ok(())
    }
}

fn normalize_edit_keys(value: &mut toml::Value) -> Result<(), ConfigError> {
    let Some(table) = value.as_table_mut() else {
        return Ok(());
    };
    let action = |value: &str| {
        crate::api::text_edit::EditAction::deserialize(serde::de::value::StrDeserializer::<
            serde::de::value::Error,
        >::new(value))
        .is_ok()
    };
    if table
        .values()
        .all(|value| value.as_str().is_some_and(action))
        || !table
            .iter()
            .all(|(key, value)| action(key) && value.is_str())
    {
        return Ok(());
    }
    let mut bindings = toml::Table::new();
    for (action, value) in std::mem::take(table) {
        let mut keys = value.as_str().unwrap_or_default().to_string();
        while bindings.contains_key(&keys) {
            if !keys.trim().is_empty() {
                return Err(ConfigError::Invalid(
                    "duplicate search editing key group".into(),
                ));
            }
            keys.push(' ');
        }
        bindings.insert(keys, toml::Value::String(action));
    }
    *table = bindings;
    Ok(())
}

fn check_binding(
    value: &toml::Value,
    section: Option<&ConfigPath<'_>>,
    key: &str,
    removed: &[(&str, &str)],
) -> Result<(), ConfigError> {
    if let Some(values) = value.as_array() {
        for value in values {
            check_binding(value, section, key, removed)?;
        }
    } else if let Some(head) = value
        .as_str()
        .and_then(|text| text.split_whitespace().next())
        && let Some((_, replacement)) = removed.iter().find(|(name, _)| *name == head)
    {
        return Err(ConfigError::Invalid(format!(
            "{}.{key}: {head} was removed; use {replacement}",
            section.map_or_else(String::new, ToString::to_string)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[derive(Deserialize)]
    struct Case {
        name: String,
        source: String,
        normalized: Option<String>,
        error: Option<String>,
        warnings: Option<Vec<String>>,
    }

    #[test]
    fn native_and_browser_share_retired_field_cases() {
        let cases: Vec<Case> = serde_json::from_str(include_str!(
            "../../tests/fixtures/config-compatibility.json"
        ))
        .unwrap();
        for case in cases {
            let mut value = toml::from_str(&case.source).unwrap();
            if let Some(message) = case.error {
                let ConfigError::Invalid(error) = normalize(&mut value).unwrap_err() else {
                    panic!("{}: expected a compatibility error", case.name);
                };
                assert_eq!(error, message, "{}", case.name);
                assert!(
                    Config::parse(&case.source)
                        .unwrap_err()
                        .to_string()
                        .contains(&message)
                );
            } else {
                let expected: toml::Value =
                    toml::from_str(case.normalized.as_ref().unwrap()).unwrap();
                let warnings = normalize(&mut value).unwrap();
                assert_eq!(warnings, case.warnings.unwrap_or_default(), "{}", case.name);
                assert_eq!(value, expected, "{}", case.name);
                assert!(normalize(&mut value).unwrap().is_empty());
                assert_eq!(value, expected, "{}: idempotent", case.name);
                let config = Config::parse(&case.source).unwrap();
                config.validate().unwrap();
                assert_eq!(
                    config,
                    Config::parse(case.normalized.as_ref().unwrap()).unwrap()
                );
                let exported = config.to_toml().unwrap();
                assert_eq!(config, Config::parse(&exported).unwrap());
            }
        }
    }

    #[test]
    fn compatibility_does_not_swallow_unknown_fields_or_malformed_sections() {
        for source in [
            "[ui_hint.search_input_ui]\nx_offest = 4",
            "[window.card]\nx_offset = 4",
            "[ui_hint]\nsearch_input_ui = 7",
            "window = 'invalid'",
        ] {
            assert!(Config::parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn current_configuration_uses_static_rules_without_allocating() {
        let mut value = toml::Value::try_from(Config::default()).unwrap();
        let region = stats_alloc::Region::new(crate::TEST_ALLOCATOR);
        for _ in 0..100 {
            std::hint::black_box(normalize(&mut value).unwrap());
        }
        assert_eq!(region.change().allocations, 0);
        assert_eq!(region.change().reallocations, 0);
    }
}
