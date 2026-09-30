use std::path::Path;

use toml::{Table, Value};

use crate::error::Result;

use super::context::manifest_error;

pub(in crate::compile) fn notebook_document(source: &str, origin: &Path) -> Result<Table> {
    let mut content = String::new();
    let mut in_block = false;
    for line in source.lines() {
        let Some(line) = line.trim().strip_prefix("//!") else {
            continue;
        };
        let line = line.trim_start();
        if line.trim() == "```cargo" {
            if in_block {
                return Err(manifest_error(origin, "Nested cargo fence"));
            }
            in_block = true;
        } else if line.trim() == "```" && in_block {
            in_block = false;
            content.push('\n');
        } else if in_block {
            content.push_str(line);
            content.push('\n');
        }
    }
    if in_block {
        return Err(manifest_error(origin, "Unclosed cargo fence"));
    }
    toml::from_str(&content).map_err(|error| {
        manifest_error(
            origin,
            &format!("Cannot parse notebook cargo block: {error}"),
        )
    })
}

pub(super) fn dependency_table(value: &Value, alias: &str, origin: &Path) -> Result<Table> {
    let table = match value {
        Value::String(version) => {
            Table::from_iter([("version".into(), Value::String(version.clone()))])
        }
        Value::Table(table) => table.clone(),
        _ => {
            return Err(manifest_error(
                origin,
                &format!("Dependency '{alias}' must be a version string or table"),
            ));
        }
    };
    for key in [
        "version",
        "path",
        "package",
        "git",
        "branch",
        "tag",
        "rev",
        "registry",
        "registry-index",
    ] {
        if table.get(key).is_some_and(|value| !value.is_str()) {
            return Err(manifest_error(
                origin,
                &format!("Dependency '{alias}'.{key} must be a string"),
            ));
        }
    }
    for key in [
        "workspace",
        "optional",
        "default-features",
        "default_features",
    ] {
        if table.get(key).is_some_and(|value| !value.is_bool()) {
            return Err(manifest_error(
                origin,
                &format!("Dependency '{alias}'.{key} must be a boolean"),
            ));
        }
    }
    if let Some(features) = table.get("features") {
        string_array(features, origin, &format!("Dependency '{alias}'.features"))?;
    }
    if let Some(version) = table.get("version").and_then(Value::as_str) {
        semver::VersionReq::parse(version).map_err(|error| {
            manifest_error(
                origin,
                &format!("Dependency '{alias}' has invalid version '{version}': {error}"),
            )
        })?;
    }
    if table
        .get("workspace")
        .is_some_and(|value| value.as_bool() != Some(true))
    {
        return Err(manifest_error(
            origin,
            &format!("Dependency '{alias}'.workspace must be true"),
        ));
    }
    Ok(table)
}

pub(super) fn string_array<'a>(
    value: &'a Value,
    origin: &Path,
    label: &str,
) -> Result<&'a Vec<Value>> {
    value
        .as_array()
        .filter(|values| values.iter().all(Value::is_str))
        .ok_or_else(|| manifest_error(origin, &format!("{label} must be an array of strings")))
}

pub(super) fn rebase_path(
    table: &mut Table,
    directory: &Path,
    alias: &str,
    origin: &Path,
) -> Result<()> {
    if let Some(path) = table.get("path").and_then(Value::as_str) {
        let path = directory.join(path);
        let path = path.canonicalize().map_err(|error| {
            manifest_error(
                origin,
                &format!(
                    "Cannot resolve dependency '{alias}' path '{}': {error}",
                    path.display()
                ),
            )
        })?;
        table.insert(
            "path".into(),
            Value::String(path.to_string_lossy().into_owned()),
        );
    }
    Ok(())
}
