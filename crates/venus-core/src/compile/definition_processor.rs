//! Processing of notebook definition cells for the universe crate.
//!
//! Definition cells hold the notebook's imports, type definitions, and helper
//! items. They are compiled into the shared `venus_universe` crate, and every
//! cell links that crate and glob-imports it (`use venus_universe::*;`). For a
//! definition-cell item to be visible to cells it must therefore be a **public**
//! item of the universe crate.
//!
//! This processor parses each definition cell with `syn` and:
//! - Re-exports every `use` statement as `pub use` (so notebook imports reach
//!   cells instead of staying private to the universe crate).
//! - Promotes every top-level definition (`struct`, `enum`, `type`, `fn`,
//!   `trait`, `const`, `static`, `union`, `mod`) to `pub` visibility.
//! - Rewrites `#[derive(...)]` on structs/enums to carry rkyv's `Archive`,
//!   `Serialize`, and `Deserialize` derives (re-exported by the universe as
//!   `Archive`/`RkyvSerialize`/`RkyvDeserialize`) so cell return values can be
//!   zero-copy serialized.
//!
//! Line-based rewriting is deliberately avoided: it silently mishandles braces
//! in strings/comments, nested items, and multi-line declarations. Everything
//! here goes through `syn`.

use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::{Attribute, Item, Visibility, parse_file, parse_quote};

/// The result of processing a notebook's definition cells.
#[derive(Debug, Default)]
pub struct ProcessedDefinitions {
    /// `pub use` re-exports, one per line, in source order.
    pub imports: String,
    /// Public type/helper definitions (with rkyv derives applied).
    pub type_definitions: String,
}

/// Process the contents of all definition cells into re-exportable imports and
/// public type definitions for the universe crate.
///
/// Each cell's content is parsed independently. If a cell fails to parse as
/// valid Rust (which should not happen for parser-extracted definition blocks),
/// its content is preserved verbatim as a type definition so nothing is lost.
pub fn process_definitions(contents: &[String]) -> ProcessedDefinitions {
    let mut imports: Vec<String> = Vec::new();
    let mut type_definitions: Vec<String> = Vec::new();

    for content in contents {
        match parse_file(content) {
            Ok(file) => {
                for item in file.items {
                    match classify_item(item) {
                        ProcessedItem::Import(text) => imports.push(text),
                        ProcessedItem::Definition(text) => type_definitions.push(text),
                    }
                }
            }
            Err(_) => {
                // Fall back to preserving the raw content so a parse failure
                // never drops user code. Such content stays private, but that
                // matches the pre-existing behavior for unparseable blocks.
                type_definitions.push(content.clone());
            }
        }
    }

    ProcessedDefinitions {
        imports: imports.join("\n"),
        type_definitions: type_definitions.join("\n\n"),
    }
}

enum ProcessedItem {
    Import(String),
    Definition(String),
}

/// Force `pub` visibility and re-export imports; render the item to source text.
fn classify_item(item: Item) -> ProcessedItem {
    let public: Visibility = parse_quote!(pub);

    match item {
        Item::Use(mut item_use) => {
            item_use.vis = public;
            ProcessedItem::Import(render(&item_use))
        }
        Item::Struct(mut s) => {
            s.vis = public;
            apply_rkyv_derives(&mut s.attrs);
            ProcessedItem::Definition(render(&s))
        }
        Item::Enum(mut e) => {
            e.vis = public;
            apply_rkyv_derives(&mut e.attrs);
            ProcessedItem::Definition(render(&e))
        }
        Item::Type(mut t) => {
            t.vis = public;
            ProcessedItem::Definition(render(&t))
        }
        Item::Fn(mut f) => {
            f.vis = public;
            ProcessedItem::Definition(render(&f))
        }
        Item::Trait(mut t) => {
            t.vis = public;
            ProcessedItem::Definition(render(&t))
        }
        Item::TraitAlias(mut t) => {
            t.vis = public;
            ProcessedItem::Definition(render(&t))
        }
        Item::Const(mut c) => {
            c.vis = public;
            ProcessedItem::Definition(render(&c))
        }
        Item::Static(mut s) => {
            s.vis = public;
            ProcessedItem::Definition(render(&s))
        }
        Item::Union(mut u) => {
            u.vis = public;
            apply_rkyv_derives(&mut u.attrs);
            ProcessedItem::Definition(render(&u))
        }
        Item::Mod(mut m) => {
            m.vis = public;
            ProcessedItem::Definition(render(&m))
        }
        // Impl blocks, macro invocations, extern crates, etc. carry no
        // visibility of their own; keep them as-is.
        other => ProcessedItem::Definition(render(&other)),
    }
}

/// Render a `syn` item back to source text.
fn render<T: ToTokens>(item: &T) -> String {
    let tokens: TokenStream = item.to_token_stream();
    tokens.to_string()
}

/// Rewrite the `#[derive(...)]` attribute of a struct/enum/union so it carries
/// rkyv's derives, dropping serde's `Serialize`/`Deserialize` (which the
/// notebook cannot use for its zero-copy value protocol).
///
/// Only types that already carry a `#[derive(...)]` are modified, matching the
/// notebook convention where serializable types opt in with a derive list.
fn apply_rkyv_derives(attrs: &mut Vec<Attribute>) {
    // Drop any pre-existing `#[rkyv(...)]` to avoid duplicating the attribute;
    // a canonical one is re-added below when a derive list is present.
    attrs.retain(|attr| !attr.path().is_ident("rkyv"));

    let mut has_derive = false;

    for attr in attrs.iter_mut() {
        if !attr.path().is_ident("derive") {
            continue;
        }
        has_derive = true;

        // Collect the existing derive paths.
        let mut existing: Vec<syn::Path> = Vec::new();
        let _ = attr.parse_nested_meta(|meta| {
            existing.push(meta.path.clone());
            Ok(())
        });

        let mut derives: Vec<syn::Path> = Vec::new();
        let mut has_rkyv = false;
        for path in existing {
            let name = path
                .segments
                .last()
                .map(|seg| seg.ident.to_string())
                .unwrap_or_default();
            match name.as_str() {
                // Serde derives are re-exported as rkyv aliases; drop the serde
                // ones and rely on the rkyv derives added below.
                "Serialize" | "Deserialize" => {}
                "Archive" | "RkyvSerialize" | "RkyvDeserialize" => {
                    has_rkyv = true;
                    derives.push(path);
                }
                _ => derives.push(path),
            }
        }

        if !has_rkyv {
            derives.push(parse_quote!(Archive));
            derives.push(parse_quote!(RkyvSerialize));
            derives.push(parse_quote!(RkyvDeserialize));
        }

        *attr = parse_quote!(#[derive(#(#derives),*)]);
    }

    if has_derive {
        attrs.push(parse_quote!(#[rkyv(derive(Debug))]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process_one(content: &str) -> ProcessedDefinitions {
        process_definitions(&[content.to_string()])
    }

    /// Normalize whitespace so assertions are robust to `syn`'s token spacing.
    fn squish(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn named_import_becomes_pub_use() {
        let out = process_one("use plotlars::polars::prelude::DataFrame;");
        assert_eq!(
            squish(&out.imports),
            "pub use plotlars :: polars :: prelude :: DataFrame ;"
        );
        assert!(out.type_definitions.is_empty());
    }

    #[test]
    fn glob_import_becomes_pub_use() {
        let out = process_one("use venus::prelude::*;");
        assert_eq!(squish(&out.imports), "pub use venus :: prelude :: * ;");
    }

    #[test]
    fn aliased_import_becomes_pub_use() {
        let out = process_one("use std::collections::HashMap as Map;");
        assert!(squish(&out.imports).starts_with("pub use std :: collections :: HashMap as Map"));
    }

    #[test]
    fn already_public_import_stays_single_pub() {
        let out = process_one("pub use std::collections::HashMap;");
        let squished = squish(&out.imports);
        assert!(squished.contains("pub use std :: collections :: HashMap"));
        assert!(!squished.contains("pub pub"));
    }

    #[test]
    fn multiple_imports_all_reexported() {
        let out = process_one("use std::collections::HashMap;\nuse std::collections::BTreeMap;");
        let squished = squish(&out.imports);
        assert!(squished.contains("pub use std :: collections :: HashMap"));
        assert!(squished.contains("pub use std :: collections :: BTreeMap"));
    }

    #[test]
    fn mixed_cell_splits_import_and_promotes_type() {
        let out = process_one(
            "use std::collections::HashMap;\n\nstruct Config {\n    map: HashMap<String, i32>,\n}",
        );
        assert!(squish(&out.imports).contains("pub use std :: collections :: HashMap"));
        assert!(squish(&out.type_definitions).contains("pub struct Config"));
    }

    #[test]
    fn non_pub_struct_is_promoted_to_pub() {
        let out = process_one("struct Foo { x: i32 }");
        assert!(squish(&out.type_definitions).starts_with("pub struct Foo"));
    }

    #[test]
    fn non_pub_helper_fn_is_promoted_to_pub() {
        let out = process_one("fn helper(x: i32) -> i32 { x + 1 }");
        assert!(squish(&out.type_definitions).starts_with("pub fn helper"));
    }

    #[test]
    fn non_pub_enum_and_type_alias_promoted() {
        let out = process_one("enum Color { Red, Green }\ntype Id = u64;");
        let squished = squish(&out.type_definitions);
        assert!(squished.contains("pub enum Color"));
        assert!(squished.contains("pub type Id"));
    }

    #[test]
    fn already_pub_struct_not_double_pub() {
        let out = process_one("pub struct Foo { pub x: i32 }");
        let squished = squish(&out.type_definitions);
        assert!(squished.contains("pub struct Foo"));
        assert!(!squished.contains("pub pub struct"));
    }

    #[test]
    fn derive_gains_rkyv_and_drops_serde() {
        let out =
            process_one("#[derive(Debug, Clone, Serialize, Deserialize)]\nstruct Point { x: i32 }");
        let squished = squish(&out.type_definitions);
        assert!(squished.contains("Archive"));
        assert!(squished.contains("RkyvSerialize"));
        assert!(squished.contains("RkyvDeserialize"));
        // Serde derives must be gone (only the rkyv aliases remain).
        assert!(!squished.contains("derive (Debug , Clone , Serialize"));
        assert!(squished.contains("rkyv (derive (Debug))"));
    }

    #[test]
    fn existing_rkyv_derives_not_duplicated() {
        let out = process_one(
            "#[derive(Debug, Archive, RkyvSerialize, RkyvDeserialize)]\nstruct Point { x: i32 }",
        );
        let squished = squish(&out.type_definitions);
        assert_eq!(squished.matches("Archive").count(), 1);
        assert_eq!(squished.matches("RkyvSerialize").count(), 1);
        assert_eq!(squished.matches("rkyv (derive (Debug))").count(), 1);
    }

    #[test]
    fn struct_without_derive_is_left_alone() {
        let out = process_one("struct Bare { x: i32 }");
        let squished = squish(&out.type_definitions);
        assert!(!squished.contains("Archive"));
        assert!(!squished.contains("rkyv"));
    }

    #[test]
    fn impl_block_preserved_without_visibility() {
        let out = process_one("impl Foo {\n    fn bar(&self) -> i32 { 0 }\n}");
        let squished = squish(&out.type_definitions);
        assert!(squished.contains("impl Foo"));
        assert!(!squished.contains("pub impl"));
    }

    #[test]
    fn unparseable_content_is_preserved() {
        let out = process_one("this is not valid rust @#$");
        assert!(out.type_definitions.contains("this is not valid rust"));
    }
}
