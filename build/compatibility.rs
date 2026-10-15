//! Generate borrowed Rust rules from the JSON also imported by the web editor.
use std::{collections::BTreeMap, fmt::Write, path::PathBuf};

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    sections: Vec<String>,
    #[serde(default)]
    ignore: Vec<String>,
    #[serde(default)]
    reject: BTreeMap<String, String>,
    #[serde(default)]
    copy: BTreeMap<String, CopyField>,
    #[serde(default)]
    reject_values: BTreeMap<String, String>,
    transform: Option<Transform>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyField {
    value_type: ValueType,
    targets: Vec<String>,
    #[serde(default)]
    existing_parent_only: bool,
    warning: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ValueType {
    Boolean,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Transform {
    SearchEditKeys,
    SearchCopyKeys,
}

pub(super) fn generate() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build/compatibility.rs");
    println!("cargo:rerun-if-changed=src/config/compatibility.json");
    let source = std::fs::read_to_string("src/config/compatibility.json")?;
    let rules: Vec<Rule> = serde_json::from_str(&source)?;
    let mut output = String::from(
        "// Generated from src/config/compatibility.json. Do not edit.\nstatic RULES: &[Rule] = &[\n",
    );
    for rule in rules {
        if rule.sections.is_empty()
            || rule
                .sections
                .iter()
                .any(|path| path.split('.').any(str::is_empty))
        {
            return Err("compatibility rules require non-empty section paths".into());
        }
        output.push_str("Rule { sections: &[\n");
        for section in rule.sections {
            writeln!(output, "&{:?},", section.split('.').collect::<Vec<_>>())?;
        }
        writeln!(output, "], ignore: &{:?},", rule.ignore)?;
        writeln!(output, "reject: {},", pairs(&rule.reject))?;
        output.push_str("copy: &[\n");
        for (field, copy) in rule.copy {
            if copy.targets.is_empty()
                || copy
                    .targets
                    .iter()
                    .any(|path| path.split('.').any(str::is_empty))
            {
                return Err("compatibility copies require non-empty target paths".into());
            }
            let kind = match copy.value_type {
                ValueType::Boolean => "ValueType::Boolean",
            };
            writeln!(
                output,
                "({field:?}, CopyField {{ value_type: {kind}, targets: &{:?}, existing_parent_only: {}, warning: {:?} }}),",
                copy.targets, copy.existing_parent_only, copy.warning
            )?;
        }
        let transform = match rule.transform {
            Some(Transform::SearchEditKeys) => "Some(Transform::SearchEditKeys)",
            Some(Transform::SearchCopyKeys) => "Some(Transform::SearchCopyKeys)",
            None => "None",
        };
        writeln!(
            output,
            "], reject_values: {}, transform: {transform} }},",
            pairs(&rule.reject_values)
        )?;
    }
    output.push_str("];\n");
    std::fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?)
            .join("compatibility_rules.rs"),
        output,
    )?;
    Ok(())
}

fn pairs(values: &BTreeMap<String, String>) -> String {
    let items = values
        .iter()
        .map(|(key, value)| format!("({key:?}, {value:?})"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("&[{items}]")
}
