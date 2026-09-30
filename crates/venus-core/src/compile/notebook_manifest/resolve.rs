use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::compile::ExternalDependency;
use crate::error::Result;

use super::context::{NotebookContext, ParentManifest, manifest_error};
use super::spec::{dependency_table, notebook_document, rebase_path, string_array};

#[derive(Debug, Clone, Default)]
pub(in crate::compile) struct ResolvedManifest {
    pub dependencies: Table,
    pub targets: Table,
    pub features: Table,
    pub patches: Table,
}

impl ResolvedManifest {
    pub fn resolve(context: &NotebookContext, source: &str) -> Result<Self> {
        let parent = context.read()?;
        let mut resolved = Self::default();
        if let Some(parent) = &parent {
            let directory = parent.path.parent().unwrap_or(Path::new("."));
            let virtual_root = !parent.document.contains_key("package")
                && parent.document.contains_key("workspace");
            let dependencies = if virtual_root {
                parent
                    .document
                    .get("workspace")
                    .and_then(|workspace| workspace.get("dependencies"))
            } else {
                parent.document.get("dependencies")
            };
            resolved.dependencies =
                resolve_dependencies(dependencies, directory, &parent.path, Some(parent))?;
            resolved.targets = resolve_targets(
                parent.document.get("target"),
                directory,
                &parent.path,
                Some(parent),
            )?;
            resolved.features = resolve_features(parent.document.get("features"), &parent.path)?;
            let (patch_document, patch_origin) = parent
                .workspace
                .as_ref()
                .map(|(path, document)| (document, path))
                .unwrap_or((&parent.document, &parent.path));
            resolved.patches = resolve_patches(patch_document.get("patch"), patch_origin)?;
        }
        let previous_optional = resolved.optional_aliases();
        let notebook = notebook_document(source, &context.directory)?;
        let dependencies = resolve_dependencies(
            notebook.get("dependencies"),
            &context.directory,
            &context.directory,
            parent.as_ref(),
        )?;
        // A notebook alias replaces its parent declaration across target sections.
        for alias in dependencies.keys() {
            for target in resolved
                .targets
                .iter_mut()
                .map(|(_, value)| value)
                .filter_map(Value::as_table_mut)
            {
                if let Some(dependencies) =
                    target.get_mut("dependencies").and_then(Value::as_table_mut)
                {
                    dependencies.remove(alias);
                }
            }
        }
        resolved.dependencies.extend(dependencies);
        let targets = resolve_targets(
            notebook.get("target"),
            &context.directory,
            &context.directory,
            parent.as_ref(),
        )?;
        for (target, mut entries) in targets {
            let entries = entries
                .as_table_mut()
                .and_then(|entries| entries.remove("dependencies"))
                .and_then(|value| match value {
                    Value::Table(table) => Some(table),
                    _ => None,
                })
                .unwrap_or_default();
            let existing = resolved
                .targets
                .entry(target)
                .or_insert_with(|| Value::Table(Table::new()));
            if let Some(existing) = existing.as_table_mut() {
                let dependencies = existing
                    .entry("dependencies")
                    .or_insert_with(|| Value::Table(Table::new()));
                if let Some(dependencies) = dependencies.as_table_mut() {
                    dependencies.extend(entries);
                }
            }
        }
        resolved.features.extend(resolve_features(
            notebook.get("features"),
            &context.directory,
        )?);
        for (registry, patches) in resolve_patches(notebook.get("patch"), &context.directory)? {
            let existing = resolved
                .patches
                .entry(registry)
                .or_insert_with(|| Value::Table(Table::new()));
            if let (Some(existing), Value::Table(patches)) = (existing.as_table_mut(), patches) {
                existing.extend(patches);
            }
        }
        let remaining_optional = resolved.optional_aliases();
        let declared = resolved
            .dependencies
            .keys()
            .chain(
                resolved
                    .targets
                    .values()
                    .filter_map(|target| target.get("dependencies").and_then(Value::as_table))
                    .flat_map(Table::keys),
            )
            .cloned()
            .collect::<BTreeSet<_>>();
        for alias in previous_optional {
            if declared.contains(&alias) && !remaining_optional.contains(&alias) {
                resolved.normalize_required_feature(&alias, true);
            }
        }
        if let Some(parent) = &parent {
            resolved.prune_omitted_features(&parent.document);
        }
        resolved.check_sources(&context.directory)?;
        Ok(resolved)
    }

    pub fn external_dependencies(&self) -> Vec<ExternalDependency> {
        let mut dependencies = self.dependencies.iter().collect::<BTreeMap<_, _>>();
        for target in self.targets.values().filter_map(Value::as_table) {
            if let Some(target_deps) = target.get("dependencies").and_then(Value::as_table) {
                for (alias, value) in target_deps {
                    dependencies.entry(alias).or_insert(value);
                }
            }
        }
        dependencies
            .into_iter()
            .map(|(name, value)| ExternalDependency {
                name: name.clone(),
                version: value
                    .get("version")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                features: value
                    .get("features")
                    .and_then(Value::as_array)
                    .map(|features| {
                        features
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                path: value.get("path").and_then(Value::as_str).map(PathBuf::from),
            })
            .collect()
    }

    pub fn has_local_paths(&self) -> bool {
        fn contains_path(value: &Value) -> bool {
            match value {
                Value::Table(table) => {
                    table.contains_key("path") || table.values().any(contains_path)
                }
                _ => false,
            }
        }
        [&self.dependencies, &self.targets, &self.patches]
            .iter()
            .any(|table| table.values().any(contains_path))
    }
}

fn resolve_dependencies(
    value: Option<&Value>,
    directory: &Path,
    origin: &Path,
    parent: Option<&ParentManifest>,
) -> Result<Table> {
    let Some(value) = value else {
        return Ok(Table::new());
    };
    let dependencies = value
        .as_table()
        .ok_or_else(|| manifest_error(origin, "dependencies must be a table"))?;
    dependencies.iter().map(|(alias, value)| {
        let mut dependency = dependency_table(value, alias, origin)?;
        let mut path_directory = directory;
        if dependency.remove("workspace").is_some() {
            let (workspace_path, workspace) = parent.and_then(|parent| parent.workspace.as_ref())
                .ok_or_else(|| manifest_error(origin, &format!("Dependency '{alias}' inherits without an owning workspace")))?;
            let base = workspace.get("workspace").and_then(|workspace| workspace.get("dependencies")).and_then(|dependencies| dependencies.get(alias))
                .ok_or_else(|| manifest_error(origin, &format!("Dependency '{alias}' is missing from workspace.dependencies in '{}'", workspace_path.display())))?;
            let mut inherited = dependency_table(base, alias, workspace_path)?;
            if inherited.contains_key("workspace") || inherited.get("optional").and_then(Value::as_bool) == Some(true) {
                return Err(manifest_error(workspace_path, &format!("workspace.dependencies.{alias} cannot inherit or be optional")));
            }
            for key in dependency.keys() {
                if !matches!(key.as_str(), "features" | "optional" | "default-features" | "default_features") {
                    return Err(manifest_error(origin, &format!("Inherited dependency '{alias}' cannot override '{key}'")));
                }
            }
            if let Some(extra) = dependency.remove("features") {
                let features = inherited.entry("features").or_insert_with(|| Value::Array(Vec::new()));
                if let Some(features) = features.as_array_mut() {
                    features.extend(string_array(&extra, origin, alias)?.iter().cloned());
                }
            }
            let defaults = inherited.get("default-features").or_else(|| inherited.get("default_features")).and_then(Value::as_bool).unwrap_or(true);
            if defaults && dependency.get("default-features").or_else(|| dependency.get("default_features")).and_then(Value::as_bool) == Some(false) {
                let edition = parent.and_then(|parent| parent.document.get("package"))
                    .and_then(|package| package.get("edition"));
                let edition = edition.and_then(Value::as_str).or_else(|| {
                    if edition.and_then(|edition| edition.get("workspace")).and_then(Value::as_bool) == Some(true) {
                        workspace.get("workspace").and_then(|workspace| workspace.get("package")).and_then(|package| package.get("edition")).and_then(Value::as_str)
                    } else { None }
                });
                if edition == Some("2024") {
                    return Err(manifest_error(origin, &format!("Inherited dependency '{alias}' cannot disable workspace default features in edition 2024. Disable defaults in workspace.dependencies")));
                }
                dependency.remove("default-features");
                dependency.remove("default_features");
            }
            inherited.extend(dependency);
            dependency = inherited;
            path_directory = workspace_path.parent().unwrap_or(directory);
        }
        if !["version", "path", "git"].iter().any(|key| dependency.contains_key(*key)) {
            return Err(manifest_error(origin, &format!("Dependency '{alias}' has no version, path, or Git source")));
        }
        if dependency.contains_key("git") && dependency.contains_key("path") {
            return Err(manifest_error(origin, &format!("Dependency '{alias}' selects both Git and path sources")));
        }
        rebase_path(&mut dependency, path_directory, alias, origin)?;
        // Feature order does not change Cargo semantics.
        if let Some(features) = dependency.get_mut("features").and_then(Value::as_array_mut) { features.sort_by(|a, b| a.as_str().cmp(&b.as_str())); features.dedup(); }
        Ok((alias.clone(), Value::Table(dependency)))
    }).collect()
}

fn resolve_targets(
    value: Option<&Value>,
    directory: &Path,
    origin: &Path,
    parent: Option<&ParentManifest>,
) -> Result<Table> {
    let Some(value) = value else {
        return Ok(Table::new());
    };
    let targets = value
        .as_table()
        .ok_or_else(|| manifest_error(origin, "target must be a table"))?;
    targets
        .iter()
        .map(|(target, value)| {
            if target.starts_with("cfg(") {
                syn::parse_str::<syn::Meta>(target).map_err(|error| {
                    manifest_error(
                        origin,
                        &format!("Target condition '{target}' is malformed: {error}"),
                    )
                })?;
            }
            let table = value.as_table().ok_or_else(|| {
                manifest_error(origin, &format!("target.{target} must be a table"))
            })?;
            let dependencies =
                resolve_dependencies(table.get("dependencies"), directory, origin, parent)?;
            Ok((
                target.clone(),
                Value::Table(Table::from_iter([(
                    "dependencies".into(),
                    Value::Table(dependencies),
                )])),
            ))
        })
        .collect()
}

fn resolve_features(value: Option<&Value>, origin: &Path) -> Result<Table> {
    let Some(value) = value else {
        return Ok(Table::new());
    };
    let features = value
        .as_table()
        .ok_or_else(|| manifest_error(origin, "features must be a table"))?;
    let mut features = features.clone();
    for (name, value) in &mut features {
        string_array(value, origin, &format!("Feature '{name}'"))?;
        if let Some(values) = value.as_array_mut() {
            values.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
            values.dedup();
        }
    }
    Ok(features)
}

fn resolve_patches(value: Option<&Value>, origin: &Path) -> Result<Table> {
    let Some(value) = value else {
        return Ok(Table::new());
    };
    let patches = value
        .as_table()
        .ok_or_else(|| manifest_error(origin, "patch must be a table"))?;
    patches
        .iter()
        .map(|(registry, entries)| {
            let directory = if origin.is_dir() {
                origin
            } else {
                origin.parent().unwrap_or(Path::new("."))
            };
            let entries = resolve_dependencies(Some(entries), directory, origin, None)?;
            Ok((registry.clone(), Value::Table(entries)))
        })
        .collect()
}
