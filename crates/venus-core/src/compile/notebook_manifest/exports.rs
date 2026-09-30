use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{Hash, Hasher};

use toml::Value;

use super::resolve::ResolvedManifest;

impl ResolvedManifest {
    pub fn reexports(&self) -> String {
        let mut aliases: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        for (alias, dependency) in &self.dependencies {
            aliases
                .entry(alias)
                .or_default()
                .insert(self.dependency_gate(alias, dependency, None));
        }
        for (target, table) in &self.targets {
            if let Some(dependencies) = table.get("dependencies").and_then(Value::as_table) {
                for (alias, dependency) in dependencies {
                    aliases
                        .entry(alias)
                        .or_default()
                        .insert(self.dependency_gate(alias, dependency, Some(target)));
                }
            }
        }
        let mut source = String::new();
        for (alias, gates) in aliases {
            if matches!(alias, "rkyv" | "serde_json" | "venus") {
                continue;
            }
            if !gates.contains("") {
                source.push_str(&format!(
                    "#[cfg(any({}))]\n",
                    gates.into_iter().collect::<Vec<_>>().join(", ")
                ));
            }
            source.push_str(&format!("pub use {};\n", alias.replace('-', "_")));
        }
        source
    }

    pub fn build_script(&self) -> String {
        let mut source = String::from(
            "fn main() {\n    let target = std::env::var(\"TARGET\").unwrap_or_default();\n",
        );
        for target in self
            .targets
            .keys()
            .filter(|target| !target.starts_with("cfg("))
        {
            let cfg = target_cfg(target);
            source.push_str(&format!("    println!(\"cargo:rustc-check-cfg=cfg({cfg})\");\n    if target == {target:?} {{ println!(\"cargo:rustc-cfg={cfg}\"); }}\n"));
        }
        source.push_str("}\n");
        source
    }

    fn dependency_gate(&self, alias: &str, dependency: &Value, target: Option<&str>) -> String {
        let mut gates = Vec::new();
        if let Some(target) = target {
            gates.push(
                if let Some(expression) = target
                    .strip_prefix("cfg(")
                    .and_then(|target| target.strip_suffix(')'))
                {
                    expression.into()
                } else {
                    target_cfg(target)
                },
            );
        }
        if dependency.get("optional").and_then(Value::as_bool) == Some(true) {
            let implicit = !self.features.contains_key(alias)
                && !self
                    .features
                    .values()
                    .filter_map(Value::as_array)
                    .flatten()
                    .filter_map(Value::as_str)
                    .any(|feature| feature.strip_prefix("dep:") == Some(alias));
            let mut activators = BTreeSet::new();
            if implicit {
                activators.insert(alias.to_string());
            }
            for feature in self.features.keys() {
                if self.activates(feature, alias, implicit, &mut HashSet::new()) {
                    activators.insert(feature.clone());
                }
            }
            gates.push(format!(
                "any({})",
                activators
                    .iter()
                    .map(|feature| format!("feature = {feature:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        match gates.len() {
            0 => String::new(),
            1 => gates.remove(0),
            _ => format!("all({})", gates.join(", ")),
        }
    }

    fn activates(
        &self,
        feature: &str,
        alias: &str,
        implicit: bool,
        visited: &mut HashSet<String>,
    ) -> bool {
        if !visited.insert(feature.into()) {
            return false;
        }
        self.features
            .get(feature)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|member| {
                member.strip_prefix("dep:") == Some(alias)
                    || (implicit && member == alias)
                    || member
                        .strip_prefix(alias)
                        .is_some_and(|suffix| suffix.starts_with('/'))
                    || self.activates(member, alias, implicit, visited)
            })
    }
}

fn target_cfg(target: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    target.hash(&mut hasher);
    format!("venus_target_{:x}", hasher.finish())
}
