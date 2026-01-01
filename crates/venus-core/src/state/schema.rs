//! Schema evolution detection for Venus notebooks.
//!
//! Detects changes to struct definitions that may require cache invalidation.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// A fingerprint of a type's schema for detecting breaking changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeFingerprint {
    /// Type name (fully qualified)
    pub type_name: String,

    /// Hash of the type's structure
    pub structure_hash: u64,

    /// Field names in order (for structs)
    pub fields: Vec<String>,

    /// Field type names in order
    pub field_types: Vec<String>,
}

impl TypeFingerprint {
    /// Create a fingerprint from field information.
    pub fn new(type_name: &str, fields: Vec<(String, String)>) -> Self {
        let mut hasher = DefaultHasher::new();
        type_name.hash(&mut hasher);
        for (name, ty) in &fields {
            name.hash(&mut hasher);
            ty.hash(&mut hasher);
        }

        let (field_names, field_types): (Vec<_>, Vec<_>) = fields.into_iter().unzip();

        Self {
            type_name: type_name.to_string(),
            structure_hash: hasher.finish(),
            fields: field_names,
            field_types,
        }
    }

    /// Create a fingerprint for a primitive type.
    pub fn primitive(type_name: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        type_name.hash(&mut hasher);

        Self {
            type_name: type_name.to_string(),
            structure_hash: hasher.finish(),
            fields: Vec::new(),
            field_types: Vec::new(),
        }
    }

    /// Compare with another fingerprint and detect the type of change.
    pub fn compare(&self, other: &TypeFingerprint) -> SchemaChange {
        if self.type_name != other.type_name {
            return SchemaChange::TypeRenamed {
                old: self.type_name.clone(),
                new: other.type_name.clone(),
            };
        }

        if self.structure_hash == other.structure_hash {
            return SchemaChange::None;
        }

        // Detect specific changes
        let old_fields: std::collections::HashSet<_> = self.fields.iter().collect();
        let new_fields: std::collections::HashSet<_> = other.fields.iter().collect();

        let added: Vec<_> = new_fields
            .difference(&old_fields)
            .cloned()
            .cloned()
            .collect();
        let removed: Vec<_> = old_fields
            .difference(&new_fields)
            .cloned()
            .cloned()
            .collect();

        // Check for type changes in existing fields
        let mut type_changes = Vec::new();
        for (i, field) in self.fields.iter().enumerate() {
            if let Some(new_idx) = other.fields.iter().position(|f| f == field)
                && self.field_types.get(i) != other.field_types.get(new_idx)
            {
                type_changes.push((
                    field.clone(),
                    self.field_types.get(i).cloned().unwrap_or_default(),
                    other.field_types.get(new_idx).cloned().unwrap_or_default(),
                ));
            }
        }

        // Determine if change is breaking
        if !removed.is_empty() || !type_changes.is_empty() {
            SchemaChange::Breaking {
                added,
                removed,
                type_changes,
            }
        } else if !added.is_empty() {
            SchemaChange::Additive { added }
        } else {
            // Some other structural change (e.g., field reordering)
            SchemaChange::Breaking {
                added: Vec::new(),
                removed: Vec::new(),
                type_changes: Vec::new(),
            }
        }
    }
}

/// Describes a change between two schema versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaChange {
    /// No change detected.
    None,

    /// Fields were added (non-breaking if optional).
    Additive { added: Vec<String> },

    /// Breaking change that requires cache invalidation.
    Breaking {
        added: Vec<String>,
        removed: Vec<String>,
        type_changes: Vec<(String, String, String)>, // (field, old_type, new_type)
    },

    /// Type was renamed (breaking).
    TypeRenamed { old: String, new: String },
}

impl SchemaChange {
    /// Check if this change is breaking (requires cache invalidation).
    pub fn is_breaking(&self) -> bool {
        matches!(
            self,
            SchemaChange::Breaking { .. } | SchemaChange::TypeRenamed { .. }
        )
    }

    /// Get a human-readable description of the change.
    pub fn description(&self) -> String {
        match self {
            SchemaChange::None => "No changes".to_string(),
            SchemaChange::Additive { added } => {
                format!("Added fields: {}", added.join(", "))
            }
            SchemaChange::Breaking {
                added,
                removed,
                type_changes,
            } => {
                let mut parts = Vec::new();
                if !added.is_empty() {
                    parts.push(format!("added: {}", added.join(", ")));
                }
                if !removed.is_empty() {
                    parts.push(format!("removed: {}", removed.join(", ")));
                }
                for (field, old, new) in type_changes {
                    parts.push(format!("{}: {} -> {}", field, old, new));
                }
                format!("Breaking changes: {}", parts.join("; "))
            }
            SchemaChange::TypeRenamed { old, new } => {
                format!("Type renamed: {} -> {}", old, new)
            }
        }
    }
}

/// Extract type fingerprint from a syn ItemStruct.
///
/// TODO(proc-macro): This function will be used by the venus proc-macro crate
/// to generate compile-time type fingerprints for schema evolution detection.
/// It enables automatic cache invalidation when struct definitions change.
#[allow(dead_code)]
pub fn fingerprint_from_struct(item: &syn::ItemStruct) -> TypeFingerprint {
    let type_name = item.ident.to_string();

    let fields: Vec<(String, String)> = match &item.fields {
        syn::Fields::Named(named) => named
            .named
            .iter()
            .map(|f| {
                let name = f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();
                let ty = quote::quote!(#f.ty).to_string();
                (name, ty)
            })
            .collect(),
        syn::Fields::Unnamed(unnamed) => unnamed
            .unnamed
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let ty = quote::quote!(#f.ty).to_string();
                (format!("{}", i), ty)
            })
            .collect(),
        syn::Fields::Unit => Vec::new(),
    };

    TypeFingerprint::new(&type_name, fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_equality() {
        let fp1 = TypeFingerprint::new(
            "TestStruct",
            vec![
                ("x".to_string(), "i32".to_string()),
                ("y".to_string(), "String".to_string()),
            ],
        );

        let fp2 = TypeFingerprint::new(
            "TestStruct",
            vec![
                ("x".to_string(), "i32".to_string()),
                ("y".to_string(), "String".to_string()),
            ],
        );

        assert_eq!(fp1, fp2);
        assert_eq!(fp1.compare(&fp2), SchemaChange::None);
    }

    #[test]
    fn test_additive_change() {
        let fp1 = TypeFingerprint::new("TestStruct", vec![("x".to_string(), "i32".to_string())]);

        let fp2 = TypeFingerprint::new(
            "TestStruct",
            vec![
                ("x".to_string(), "i32".to_string()),
                ("y".to_string(), "String".to_string()),
            ],
        );

        let change = fp1.compare(&fp2);
        assert!(!change.is_breaking());
        match change {
            SchemaChange::Additive { added } => {
                assert_eq!(added, vec!["y".to_string()]);
            }
            _ => panic!("Expected Additive change"),
        }
    }

    #[test]
    fn test_breaking_removal() {
        let fp1 = TypeFingerprint::new(
            "TestStruct",
            vec![
                ("x".to_string(), "i32".to_string()),
                ("y".to_string(), "String".to_string()),
            ],
        );

        let fp2 = TypeFingerprint::new("TestStruct", vec![("x".to_string(), "i32".to_string())]);

        let change = fp1.compare(&fp2);
        assert!(change.is_breaking());
    }

    #[test]
    fn test_breaking_type_change() {
        let fp1 = TypeFingerprint::new("TestStruct", vec![("x".to_string(), "i32".to_string())]);

        let fp2 = TypeFingerprint::new("TestStruct", vec![("x".to_string(), "i64".to_string())]);

        let change = fp1.compare(&fp2);
        assert!(change.is_breaking());
        match change {
            SchemaChange::Breaking { type_changes, .. } => {
                assert_eq!(type_changes.len(), 1);
                assert_eq!(type_changes[0].0, "x");
            }
            _ => panic!("Expected Breaking change"),
        }
    }

    #[test]
    fn test_type_renamed() {
        let fp1 = TypeFingerprint::new("OldName", vec![]);
        let fp2 = TypeFingerprint::new("NewName", vec![]);

        let change = fp1.compare(&fp2);
        assert!(change.is_breaking());
        match change {
            SchemaChange::TypeRenamed { old, new } => {
                assert_eq!(old, "OldName");
                assert_eq!(new, "NewName");
            }
            _ => panic!("Expected TypeRenamed"),
        }
    }

    #[test]
    fn test_primitive_fingerprint() {
        let fp1 = TypeFingerprint::primitive("i32");
        let fp2 = TypeFingerprint::primitive("i32");
        let fp3 = TypeFingerprint::primitive("i64");

        assert_eq!(fp1.compare(&fp2), SchemaChange::None);
        assert!(fp1.compare(&fp3).is_breaking());
    }

    #[test]
    fn test_fingerprint_from_syn() {
        let code = "struct Point { x: f64, y: f64 }";
        let item: syn::ItemStruct = syn::parse_str(code).unwrap();

        let fp = fingerprint_from_struct(&item);
        assert_eq!(fp.type_name, "Point");
        assert_eq!(fp.fields, vec!["x", "y"]);
    }
}
