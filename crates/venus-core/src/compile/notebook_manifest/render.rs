use toml::{Table, Value};

use crate::compile::ManifestConfig;
use crate::error::Result;

use super::context::manifest_error;
use super::resolve::ResolvedManifest;

impl ResolvedManifest {
    pub fn render(&self, config: &ManifestConfig<'_>) -> Result<String> {
        let mut manifest = self.sections();
        manifest.insert(
            "package".into(),
            Value::Table(Table::from_iter([
                ("name".into(), Value::String(config.name.into())),
                ("version".into(), Value::String(config.version.into())),
                ("edition".into(), Value::String(config.edition.into())),
            ])),
        );
        if let Some(types) = config.lib_crate_types {
            manifest.insert(
                "lib".into(),
                Value::Table(Table::from_iter([(
                    "crate-type".into(),
                    Value::Array(
                        types
                            .iter()
                            .map(|name| Value::String((*name).into()))
                            .collect(),
                    ),
                )])),
            );
        }
        if let Some(profile) = &config.release_profile {
            manifest.insert(
                "profile".into(),
                Value::Table(Table::from_iter([(
                    "release".into(),
                    Value::Table(Table::from_iter([
                        ("opt-level".into(), Value::Integer(profile.opt_level.into())),
                        ("lto".into(), Value::Boolean(profile.lto)),
                        (
                            "codegen-units".into(),
                            Value::Integer(profile.codegen_units.into()),
                        ),
                        ("panic".into(), Value::String(profile.panic.into())),
                    ])),
                )])),
            );
        }
        if config.standalone_workspace {
            manifest.insert("workspace".into(), Value::Table(Table::new()));
        }
        toml::to_string(&manifest).map_err(|error| {
            manifest_error(
                std::path::Path::new(config.name),
                &format!("Cannot render generated manifest: {error}"),
            )
        })
    }

    pub fn canonical(&self) -> String {
        Value::Table(self.sections()).to_string()
    }

    fn sections(&self) -> Table {
        let mut sections = Table::from_iter([(
            "dependencies".into(),
            Value::Table(self.dependencies.clone()),
        )]);
        for (name, table) in [
            ("target", &self.targets),
            ("features", &self.features),
            ("patch", &self.patches),
        ] {
            if !table.is_empty() {
                sections.insert(name.into(), Value::Table(table.clone()));
            }
        }
        sections
    }
}
