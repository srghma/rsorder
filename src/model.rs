//! Item model and classification of `syn` items.

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
        use ItemKind::*;
        match self {
            Fn => "fn",
            Struct => "struct",
            Enum => "enum",
            Union => "union",
            Trait => "trait",
            Impl => "impl",
            Const => "const",
            Static => "static",
            TypeAlias => "type",
            Mod => "mod",
            MacroDef => "macro",
            MacroCall => "macro-call",
            Use => "use",
            ExternCrate => "extern crate",
            Other => "other",
        }
    }
}

/// A single top-level item carved out of the source file (in source order).
#[derive(Debug, Clone)]
pub struct Item {
    pub kind: ItemKind,
    /// Name introduced into the namespace, if any.
    pub name: Option<String>,
    /// Human-readable label for summaries / graphs.
    pub display: String,
    /// Comment lines attached directly above (no blank line between).
    pub lead: String,
    /// Comment block sitting above but separated by a blank line.
    pub pre_floating: String,
    /// Verbatim source of the item (attrs + body), trailing whitespace trimmed.
    pub body: String,
    /// Byte offset of the item's start in the original source.
    pub byte_start: usize,
    /// Whether the original gap before this item contained a blank line.
    pub blank_before: bool,
    /// Global indices (into `FileModel::items`) this item depends on.
    pub deps: Vec<usize>,
    /// `use` / `extern crate` / opaque blocks are pinned (never hoisted).
    pub pinned: bool,
}

fn clean(s: String) -> String {
    s.replace(" :: ", "::")
        .replace(":: ", "::")
        .replace(" ::", "::")
        .replace(" < ", "<")
        .replace("< ", "<")
        .replace(" >", ">")
        .replace(" ,", ",")
}

fn path_str(p: &syn::Path) -> String {
    clean(p.to_token_stream().to_string())
}
fn type_str(t: &syn::Type) -> String {
    clean(t.to_token_stream().to_string())
}

/// (kind, defined-name, display-label, pinned)
pub fn classify(item: &syn::Item) -> (ItemKind, Option<String>, String, bool) {
    use syn::Item::*;
    let named = |k: ItemKind, id: &syn::Ident, kw: &str| {
        let n = id.to_string();
        (k, Some(n.clone()), format!("{kw} {n}"), false)
    };
    match item {
        Fn(f) => named(ItemKind::Fn, &f.sig.ident, "fn"),
        Struct(s) => named(ItemKind::Struct, &s.ident, "struct"),
        Enum(e) => named(ItemKind::Enum, &e.ident, "enum"),
        Union(u) => named(ItemKind::Union, &u.ident, "union"),
        Trait(t) => named(ItemKind::Trait, &t.ident, "trait"),
        TraitAlias(t) => named(ItemKind::Trait, &t.ident, "trait"),
        Type(t) => named(ItemKind::TypeAlias, &t.ident, "type"),
        Const(c) => named(ItemKind::Const, &c.ident, "const"),
        Static(s) => named(ItemKind::Static, &s.ident, "static"),
        Mod(m) => named(ItemKind::Mod, &m.ident, "mod"),
        Macro(m) => match &m.ident {
            Some(id) => {
                let n = id.to_string();
                (
                    ItemKind::MacroDef,
                    Some(n.clone()),
                    format!("macro {n}!"),
                    false,
                )
            }
            None => (
                ItemKind::MacroCall,
                None,
                format!("{}!(…)", path_str(&m.mac.path)),
                false,
            ),
        },
        Impl(i) => {
            let s = type_str(&i.self_ty);
            let disp = match &i.trait_ {
                Some((_, tp, _)) => format!("impl {} for {}", path_str(tp), s),
                None => format!("impl {s}"),
            };
            (ItemKind::Impl, None, disp, false)
        }
        Use(_) => (ItemKind::Use, None, "use …".to_string(), true),
        ExternCrate(e) => (
            ItemKind::ExternCrate,
            None,
            format!("extern crate {}", e.ident),
            true,
        ),
        ForeignMod(_) => (ItemKind::Other, None, "extern { … }".to_string(), true),
        _ => (ItemKind::Other, None, "<item>".to_string(), true),
    }
}
