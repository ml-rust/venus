use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use toml::{Table, Value};

use crate::error::Result;

use super::context::manifest_error;
use super::resolve::ResolvedManifest;

impl ResolvedManifest {
    pub fn optional_aliases(&self) -> BTreeSet<String> {
        self.dependencies
            .iter()
            .chain(
                self.targets
                    .values()
                    .filter_map(|target| target.get("dependencies").and_then(Value::as_table))
                    .flat_map(Table::iter),
            )
            .filter(|(_, value)| value.get("optional").and_then(Value::as_bool) == Some(true))
            .map(|(alias, _)| alias.clone())
            .collect()
    }

    pub fn normalize_required_feature(&mut self, alias: &str, was_optional: bool) {
        let activation = format!("dep:{alias}");
        let weak_forward = format!("{alias}?/");
        let explicit_activation = self
            .features
            .values()
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(Value::as_str)
            .any(|member| member == activation);
        if was_optional && !explicit_activation {
            self.features
                .entry(alias)
                .or_insert_with(|| Value::Array(Vec::new()));
        }
        for feature in self
            .features
            .iter_mut()
            .map(|(_, value)| value)
            .filter_map(Value::as_array_mut)
        {
            feature.retain(|member| member.as_str() != Some(activation.as_str()));
            for member in feature {
                if let Some(forward) = member
                    .as_str()
                    .and_then(|member| member.strip_prefix(weak_forward.as_str()))
                {
                    *member = Value::String(format!("{alias}/{forward}"));
                }
            }
        }
    }

    pub fn prune_omitted_features(&mut self, parent: &Table) {
        let mut omitted = BTreeSet::new();
        for table in std::iter::once(parent).chain(
            parent
                .get("target")
                .and_then(Value::as_table)
                .into_iter()
                .flat_map(Table::values)
                .filter_map(Value::as_table),
        ) {
            for section in ["build-dependencies", "dev-dependencies"] {
                if let Some(dependencies) = table.get(section).and_then(Value::as_table) {
                    omitted.extend(dependencies.keys().cloned());
                }
            }
        }
        for (alias, _) in self.dependencies.iter().chain(
            self.targets
                .values()
                .filter_map(|target| target.get("dependencies").and_then(Value::as_table))
                .flat_map(Table::iter),
        ) {
            omitted.remove(alias);
        }
        let nodes = self.features.keys().cloned().collect::<BTreeSet<_>>();
        for feature in self
            .features
            .iter_mut()
            .map(|(_, value)| value)
            .filter_map(Value::as_array_mut)
        {
            feature.retain(|member| {
                let Some(member) = member.as_str() else {
                    return true;
                };
                let alias = member
                    .strip_prefix("dep:")
                    .unwrap_or(member)
                    .split('/')
                    .next()
                    .unwrap_or(member)
                    .trim_end_matches('?');
                !omitted.contains(alias)
                    || (!member.starts_with("dep:")
                        && !member.contains('/')
                        && nodes.contains(member))
            });
        }
    }

    pub fn check_sources(&self, origin: &Path) -> Result<()> {
        let mut sources = BTreeMap::new();
        for (alias, dependency) in self.dependencies.iter().chain(
            self.targets
                .values()
                .filter_map(|target| target.get("dependencies").and_then(Value::as_table))
                .flat_map(Table::iter),
        ) {
            let source = [
                "package",
                "path",
                "git",
                "rev",
                "branch",
                "tag",
                "registry",
                "registry-index",
            ]
            .iter()
            .map(|key| {
                (
                    *key,
                    if *key == "package" {
                        Some(Value::String(
                            dependency
                                .get(*key)
                                .and_then(Value::as_str)
                                .unwrap_or(alias)
                                .into(),
                        ))
                    } else {
                        dependency.get(*key).cloned()
                    },
                )
            })
            .collect::<Vec<_>>();
            if sources
                .get(alias)
                .is_some_and(|existing| existing != &source)
            {
                return Err(manifest_error(
                    origin,
                    &format!(
                        "Dependency alias '{alias}' selects different packages or sources across targets. Use distinct aliases or matching sources"
                    ),
                ));
            }
            sources.insert(alias, source);
        }
        Ok(())
    }
}
