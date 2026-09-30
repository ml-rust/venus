use std::path::Path;

use semver::{Version, VersionReq};
use toml::{Table, Value};

use crate::compile::CompilerConfig;
use crate::error::Result;

use super::context::{absolute, manifest_error};
use super::resolve::ResolvedManifest;

impl ResolvedManifest {
    pub fn with_runtime(&self, config: &CompilerConfig, origin: &Path) -> Result<Self> {
        let mut resolved = self.clone();
        let mut venus = Table::new();
        if let Some(path) = &config.venus_crate_path {
            venus.insert(
                "path".into(),
                Value::String(absolute(path)?.to_string_lossy().into_owned()),
            );
        } else {
            venus.insert("version".into(), Value::String("0.1".into()));
        }
        let runtime = [
            ("venus", venus, "0.1", Vec::<&str>::new()),
            (
                "rkyv",
                Table::from_iter([("version".into(), Value::String("0.8".into()))]),
                "0.8",
                vec!["std", "bytecheck"],
            ),
            (
                "serde_json",
                Table::from_iter([("version".into(), Value::String("1.0".into()))]),
                "1.0",
                Vec::new(),
            ),
        ];
        for (alias, mut required, version, required_features) in runtime {
            let mut features = required_features
                .into_iter()
                .map(|feature| Value::String(feature.into()))
                .collect::<Vec<_>>();
            if let Some(user) = self.dependencies.get(alias) {
                check_reserved(alias, user, &required, version, origin)?;
                append_features(user, &mut features);
                retain_version(user, &mut required);
            }
            let mut was_optional = self
                .dependencies
                .get(alias)
                .and_then(|user| user.get("optional"))
                .and_then(Value::as_bool)
                == Some(true);
            for target in resolved
                .targets
                .iter_mut()
                .map(|(_, value)| value)
                .filter_map(Value::as_table_mut)
            {
                if let Some(user) = target
                    .get_mut("dependencies")
                    .and_then(Value::as_table_mut)
                    .and_then(|dependencies| dependencies.get_mut(alias))
                {
                    check_reserved(alias, user, &required, version, origin)?;
                    was_optional |= user.get("optional").and_then(Value::as_bool) == Some(true);
                    let mut target_required = required.clone();
                    let mut target_features = Vec::new();
                    append_features(user, &mut target_features);
                    retain_version(user, &mut target_required);
                    if !target_features.is_empty() {
                        target_required.insert("features".into(), Value::Array(target_features));
                    }
                    *user = Value::Table(target_required);
                }
            }
            resolved.normalize_required_feature(alias, was_optional);
            features.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
            features.dedup();
            if !features.is_empty() {
                required.insert("features".into(), Value::Array(features));
            }
            resolved
                .dependencies
                .insert(alias.into(), Value::Table(required));
        }
        Ok(resolved)
    }
}

fn append_features(user: &Value, features: &mut Vec<Value>) {
    if let Some(extra) = user.get("features").and_then(Value::as_array) {
        features.extend(extra.iter().cloned());
    }
}

fn check_reserved(
    alias: &str,
    user: &Value,
    required: &Table,
    version: &str,
    origin: &Path,
) -> Result<()> {
    let incompatible = || {
        manifest_error(
            origin,
            &format!(
                "Dependency alias '{alias}' conflicts with the required Venus runtime dependency"
            ),
        )
    };
    if user
        .get("package")
        .and_then(Value::as_str)
        .is_some_and(|package| package != alias)
    {
        return Err(incompatible());
    }
    for key in ["path", "git", "registry", "registry-index"] {
        if user
            .get(key)
            .is_some_and(|value| key != "path" || required.get(key) != Some(value))
        {
            return Err(incompatible());
        }
    }
    if let Some(requirement) = user.get("version").and_then(Value::as_str) {
        let requirement = VersionReq::parse(requirement).map_err(|_| incompatible())?;
        let runtime = VersionReq::parse(version).map_err(|_| incompatible())?;
        let base = Version::parse(&format!("{version}.0")).map_err(|_| incompatible())?;
        let base_minor = base.minor;
        let mut candidates = vec![base];
        for comparator in &requirement.comparators {
            let candidate = Version::new(
                comparator.major,
                comparator.minor.unwrap_or(base_minor),
                comparator.patch.unwrap_or(0),
            );
            candidates.push(Version::new(
                candidate.major,
                candidate.minor,
                candidate.patch.saturating_add(1),
            ));
            candidates.push(Version::new(
                candidate.major,
                candidate.minor.saturating_add(1),
                0,
            ));
            candidates.push(candidate);
        }
        if !candidates
            .iter()
            .any(|candidate| runtime.matches(candidate) && requirement.matches(candidate))
        {
            return Err(incompatible());
        }
    }
    Ok(())
}

fn retain_version(user: &Value, required: &mut Table) {
    if let (Some(user), Some(runtime)) = (
        user.get("version").and_then(Value::as_str),
        required.get("version").and_then(Value::as_str),
    ) {
        required.insert(
            "version".into(),
            Value::String(format!("{runtime}, {user}")),
        );
    }
}
