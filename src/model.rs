//! Data model for a single top-level Rust item and helpers to classify it.

use quote::ToTokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Fn,
    Struct,
    Enum,
    Union,
    Trait,
    Impl,
    Const,
    Static,
    TypeAlias,
    Mod,
    MacroDef,
    MacroCall,
    Use,
    ExternCrate,
    Other,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Fn => "fn",
            ItemKind::Struct => "struct",
            ItemKind::Enum => "enum",
            ItemKind::Union => "union",
            ItemKind::Trait => "trait",
            ItemKind::Impl => "impl",
            ItemKind::Const => "const",
            ItemKind::Static => "static",
            ItemKind::TypeAlias => "type",
            ItemKind::Mod => "mod",
            ItemKind::MacroDef => "macro",
            ItemKind::MacroCall => "macro-call",
            ItemKind::Use => "use",
            ItemKind::ExternCrate => "extern crate",
            ItemKind::Other => "other",
        }
    }
}

/// A single top-level item carved out of the source file.
#[derive(Debug, Clone)]
pub struct Item {
    pub kind: ItemKind,
    /// The name this item introduces into the namespace, if any.
    pub name: Option<String>,
    /// Human-readable label used in summaries and graphs.
    pub display: String,
    /// Comment lines attached directly above the item (no blank line between).
    pub lead: String,
    /// Verbatim source text of the item (attributes + body), trailing ws trimmed.
    pub body: String,
    /// Indices (into the reorderable list) of items this one depends on.
    pub deps: Vec<usize>,
    /// Whether this item is pinned at the top (use / extern crate).
    pub pinned: bool,
}

fn path_str(path: &syn::Path) -> String {
    path.to_token_stream()
        .to_string()
        .replace(" :: ", "::")
        .replace(" ::", "::")
        .replace(":: ", "::")
}

fn type_str(ty: &syn::Type) -> String {
    let s = ty.to_token_stream().to_string();
    // Light cleanup so labels read nicely.
    s.replace(" < ", "<")
        .replace(" >", ">")
        .replace("< ", "<")
        .replace(" ,", ",")
        .replace(" :: ", "::")
}

/// Returns (kind, defined-name, display-label, pinned) for a syn item.
pub fn classify(item: &syn::Item) -> (ItemKind, Option<String>, String, bool) {
    use syn::Item::*;
    match item {
        Fn(f) => {
            let n = f.sig.ident.to_string();
            (ItemKind::Fn, Some(n.clone()), format!("fn {n}"), false)
        }
        Struct(s) => {
            let n = s.ident.to_string();
            (ItemKind::Struct, Some(n.clone()), format!("struct {n}"), false)
        }
        Enum(e) => {
            let n = e.ident.to_string();
            (ItemKind::Enum, Some(n.clone()), format!("enum {n}"), false)
        }
        Union(u) => {
            let n = u.ident.to_string();
            (ItemKind::Union, Some(n.clone()), format!("union {n}"), false)
        }
        Trait(t) => {
            let n = t.ident.to_string();
            (ItemKind::Trait, Some(n.clone()), format!("trait {n}"), false)
        }
        TraitAlias(t) => {
            let n = t.ident.to_string();
            (ItemKind::Trait, Some(n.clone()), format!("trait {n}"), false)
        }
        Type(t) => {
            let n = t.ident.to_string();
            (ItemKind::TypeAlias, Some(n.clone()), format!("type {n}"), false)
        }
        Const(c) => {
            let n = c.ident.to_string();
            (ItemKind::Const, Some(n.clone()), format!("const {n}"), false)
        }
        Static(s) => {
            let n = s.ident.to_string();
            (ItemKind::Static, Some(n.clone()), format!("static {n}"), false)
        }
        Mod(m) => {
            let n = m.ident.to_string();
            (ItemKind::Mod, Some(n.clone()), format!("mod {n}"), false)
        }
        Macro(m) => {
            if let Some(id) = &m.ident {
                let n = id.to_string();
                (ItemKind::MacroDef, Some(n.clone()), format!("macro {n}!"), false)
            } else {
                let p = path_str(&m.mac.path);
                (ItemKind::MacroCall, None, format!("{p}!(…)"), false)
            }
        }
        Impl(i) => {
            let self_ty = type_str(&i.self_ty);
            let disp = if let Some((_, tp, _)) = &i.trait_ {
                format!("impl {} for {}", path_str(tp), self_ty)
            } else {
                format!("impl {self_ty}")
            };
            (ItemKind::Impl, None, disp, false)
        }
        Use(_) => (ItemKind::Use, None, "use …".to_string(), true),
        ExternCrate(e) => {
            let n = e.ident.to_string();
            (ItemKind::ExternCrate, None, format!("extern crate {n}"), true)
        }
        ForeignMod(_) => (ItemKind::Other, None, "extern { … }".to_string(), true),
        Verbatim(_) => (ItemKind::Other, None, "<verbatim>".to_string(), true),
        _ => (ItemKind::Other, None, "<item>".to_string(), true),
    }
}
