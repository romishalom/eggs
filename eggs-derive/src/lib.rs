//! Derive macro for the [`eggs`](https://docs.rs/eggs) crate.
//!
//! See the `eggs` crate documentation for the attribute reference. This crate is not meant to
//! be used directly; enable the `derive` feature of `eggs` (on by default).

use std::collections::HashMap;

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{
    parse_macro_input, parse_quote, Data, DeriveInput, Error, Fields, Index, LitInt, LitStr, Member,
    Path, Result, Type,
};

/// Derives `eggs::Egg` for a struct.
///
/// # Attributes
///
/// Field attributes (`#[egg(...)]`):
///
/// * `skip` — exclude the field from capture, restore and diff.
/// * `rename = "name"` — use a custom field path label.
/// * `default` — only on `skip` fields: build the field with `Default` in `Egg::from_snapshot`.
/// * `ignore_diff` — capture and restore, but omit from diffs.
/// * `ignore_restore` — capture and diff, but never restore (patches still apply it).
/// * `with = "module"` — custom adapter module providing `type Snapshot`,
///   `fn snapshot(&T) -> Snapshot`, `fn restore(&mut T, &Snapshot) -> EggResult<()>`,
///   `fn from_snapshot(&Snapshot) -> EggResult<T>` and
///   `fn diff(&T, &T, &EggPath, &mut Vec<Change>)`.
/// * `compare_with = "path"` — `fn(&T, &T) -> bool` equality; a difference is reported as one
///   `Modified` change at the field.
/// * `strategy = "clone"` — select the snapshot strategy (only `"clone"` exists).
///
/// Struct attribute: `#[egg(version = N)]` declares the schema version.
#[proc_macro_derive(Egg, attributes(egg))]
pub fn derive_egg(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

#[derive(Default)]
struct FieldAttrs {
    skip: bool,
    default: bool,
    ignore_diff: bool,
    ignore_restore: bool,
    rename: Option<LitStr>,
    with: Option<Path>,
    compare_with: Option<Path>,
    strategy: Option<LitStr>,
}

fn set_flag(flag: &mut bool, meta: &syn::meta::ParseNestedMeta<'_>, name: &str) -> Result<()> {
    if *flag {
        return Err(meta.error(format!("duplicate `{name}` attribute")));
    }
    *flag = true;
    Ok(())
}

fn set_once<V>(slot: &mut Option<V>, value: V, meta: &syn::meta::ParseNestedMeta<'_>, name: &str) -> Result<()> {
    if slot.is_some() {
        return Err(meta.error(format!("duplicate `{name}` attribute")));
    }
    *slot = Some(value);
    Ok(())
}

fn parse_field_attrs(attrs: &[syn::Attribute]) -> Result<FieldAttrs> {
    let mut out = FieldAttrs::default();
    for attr in attrs.iter().filter(|a| a.path().is_ident("egg")) {
        attr.parse_nested_meta(|meta| {
            let name = meta.path.get_ident().map(ToString::to_string).unwrap_or_default();
            match name.as_str() {
                "skip" => set_flag(&mut out.skip, &meta, "skip"),
                "default" => set_flag(&mut out.default, &meta, "default"),
                "ignore_diff" => set_flag(&mut out.ignore_diff, &meta, "ignore_diff"),
                "ignore_restore" => set_flag(&mut out.ignore_restore, &meta, "ignore_restore"),
                "rename" => {
                    let lit: LitStr = meta.value()?.parse()?;
                    set_once(&mut out.rename, lit, &meta, "rename")
                }
                "strategy" => {
                    let lit: LitStr = meta.value()?.parse()?;
                    set_once(&mut out.strategy, lit, &meta, "strategy")
                }
                "with" => {
                    let lit: LitStr = meta.value()?.parse()?;
                    let path = lit.parse::<Path>()?;
                    set_once(&mut out.with, path, &meta, "with")
                }
                "compare_with" => {
                    let lit: LitStr = meta.value()?.parse()?;
                    let path = lit.parse::<Path>()?;
                    set_once(&mut out.compare_with, path, &meta, "compare_with")
                }
                "version" => Err(meta.error(
                    "`version` is a struct-level attribute: put `#[egg(version = N)]` on the struct, not on a field",
                )),
                other => Err(meta.error(format!(
                    "unknown egg attribute `{other}`; expected one of: skip, rename, default, ignore_diff, \
                     ignore_restore, with, compare_with, strategy"
                ))),
            }
        })?;
    }
    Ok(out)
}

fn validate_field(span: Span, a: &FieldAttrs) -> Result<()> {
    let err = |msg: &str| Err(Error::new(span, msg));
    if a.skip {
        let conflicts = [
            (a.ignore_diff, "ignore_diff"),
            (a.ignore_restore, "ignore_restore"),
            (a.rename.is_some(), "rename"),
            (a.with.is_some(), "with"),
            (a.compare_with.is_some(), "compare_with"),
            (a.strategy.is_some(), "strategy"),
        ];
        if let Some((_, name)) = conflicts.iter().find(|(present, _)| *present) {
            return err(&format!(
                "`#[egg(skip)]` conflicts with `{name}`: a skipped field is already excluded entirely"
            ));
        }
    } else if a.default {
        return err(
            "`#[egg(default)]` is only meaningful on `#[egg(skip)]` fields (it supplies the value \
             when a struct is rebuilt from a snapshot)",
        );
    }
    if a.ignore_diff && a.compare_with.is_some() {
        return err("`#[egg(ignore_diff)]` conflicts with `compare_with`: the field is never compared");
    }
    if a.with.is_some() && a.compare_with.is_some() {
        return err("`#[egg(with = ...)]` conflicts with `compare_with`: the adapter's `diff` does the comparing");
    }
    if a.with.is_some() && a.strategy.is_some() {
        return err("`#[egg(with = ...)]` conflicts with `strategy`: the adapter chooses how the field is captured");
    }
    if let Some(s) = &a.strategy {
        if s.value() != "clone" {
            return Err(Error::new(
                s.span(),
                format!("unknown snapshot strategy `{}`; the only supported strategy is \"clone\"", s.value()),
            ));
        }
    }
    if let Some(r) = &a.rename {
        if r.value().is_empty() {
            return Err(Error::new(r.span(), "`rename` must not be empty"));
        }
    }
    Ok(())
}

fn parse_container_attrs(attrs: &[syn::Attribute]) -> Result<Option<u32>> {
    let mut version = None;
    for attr in attrs.iter().filter(|a| a.path().is_ident("egg")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("version") {
                let lit: LitInt = meta.value()?.parse()?;
                set_once(&mut version, lit.base10_parse::<u32>()?, &meta, "version")
            } else {
                Err(meta.error(
                    "unknown struct-level egg attribute; the only one is `version = N` \
                     (the other attributes belong on fields)",
                ))
            }
        })?;
    }
    Ok(version)
}

struct FieldInfo<'a> {
    member: Member,
    ty: &'a Type,
    label: String,
    attrs: FieldAttrs,
    /// Name of the corresponding field in the generated snapshot struct (captured fields only).
    snap_field: Option<syn::Ident>,
}

fn expand(input: &DeriveInput) -> Result<TokenStream2> {
    let data = match &input.data {
        Data::Struct(s) => s,
        Data::Enum(e) => {
            return Err(Error::new(
                e.enum_token.span,
                "`#[derive(Egg)]` does not support enums yet: their variant-change semantics are not \
                 defined. Derive on a struct, or implement `eggs::Egg` by hand",
            ))
        }
        Data::Union(u) => {
            return Err(Error::new(u.union_token.span, "`#[derive(Egg)]` does not support unions"))
        }
    };
    let version = parse_container_attrs(&input.attrs)?;
    let name = &input.ident;
    let vis = &input.vis;

    let raw_fields: Vec<&syn::Field> = match &data.fields {
        Fields::Named(f) => f.named.iter().collect(),
        Fields::Unnamed(f) => f.unnamed.iter().collect(),
        Fields::Unit => Vec::new(),
    };
    let named = matches!(data.fields, Fields::Named(_));
    let unit = matches!(data.fields, Fields::Unit);

    let mut fields: Vec<FieldInfo<'_>> = Vec::new();
    let mut labels: HashMap<String, ()> = HashMap::new();
    let mut captured_count = 0usize;
    for (i, f) in raw_fields.iter().enumerate() {
        let attrs = parse_field_attrs(&f.attrs)?;
        validate_field(f.span(), &attrs)?;
        let member = match &f.ident {
            Some(id) => Member::Named(id.clone()),
            None => Member::Unnamed(Index::from(i)),
        };
        let default_label = match &f.ident {
            Some(id) => id.unraw().to_string(),
            None => i.to_string(),
        };
        let label = attrs.rename.as_ref().map_or(default_label, LitStr::value);
        let snap_field = if attrs.skip {
            None
        } else {
            if labels.insert(label.clone(), ()).is_some() {
                return Err(Error::new(
                    f.span(),
                    format!("duplicate field label `{label}`; give one of the fields a distinct `#[egg(rename = \"...\")]`"),
                ));
            }
            captured_count += 1;
            Some(format_ident!("f{}", captured_count - 1))
        };
        fields.push(FieldInfo { member, ty: &f.ty, label, attrs, snap_field });
    }

    let captured: Vec<&FieldInfo<'_>> = fields.iter().filter(|f| !f.attrs.skip).collect();
    let snap_ident = format_ident!("{}EggSnapshot", name);
    let params: Vec<syn::Ident> = (0..captured.len()).map(|i| format_ident!("__EggSnap{}", i)).collect();
    let snap_field_idents: Vec<&syn::Ident> = captured.iter().map(|f| f.snap_field.as_ref().unwrap()).collect();
    let snap_args: Vec<TokenStream2> = captured
        .iter()
        .map(|f| match &f.attrs.with {
            Some(w) => quote!(#w::Snapshot),
            None => {
                let ty = f.ty;
                quote!(<#ty as ::eggs::Egg>::Snapshot)
            }
        })
        .collect();

    let seg = |label: &str| quote!(::eggs::PathSegment::Field(::std::string::String::from(#label)));

    // ---- snapshot / restore / from_snapshot -------------------------------------------------
    let snapshot_exprs = captured.iter().map(|f| {
        let m = &f.member;
        match &f.attrs.with {
            Some(w) => quote!(#w::snapshot(&self.#m)),
            None => quote!(::eggs::Egg::snapshot(&self.#m)),
        }
    });

    let restore_stmts = captured.iter().filter(|f| !f.attrs.ignore_restore).map(|f| {
        let m = &f.member;
        let fi = f.snap_field.as_ref().unwrap();
        let s = seg(&f.label);
        match &f.attrs.with {
            Some(w) => quote!(#w::restore(&mut self.#m, &snapshot.#fi).map_err(|e| e.within(&#s))?;),
            None => quote!(::eggs::Egg::restore_in_place(&mut self.#m, &snapshot.#fi).map_err(|e| e.within(&#s))?;),
        }
    });

    let missing_default = fields.iter().find(|f| f.attrs.skip && !f.attrs.default);
    let from_snapshot_body = if let Some(missing) = missing_default {
        let msg = format!(
            "field `{}` of `{}` is #[egg(skip)] without #[egg(default)], so a value cannot be built from a \
             snapshot alone",
            missing.label, name
        );
        quote! {
            let _ = snapshot;
            ::std::result::Result::Err(::eggs::EggError::unsupported("from_snapshot", #msg))
        }
    } else {
        let exprs: Vec<TokenStream2> = fields
            .iter()
            .map(|f| match (&f.snap_field, &f.attrs.with) {
                (None, _) => quote!(::std::default::Default::default()),
                (Some(fi), Some(w)) => quote!(#w::from_snapshot(&snapshot.#fi)?),
                (Some(fi), None) => {
                    let s = seg(&f.label);
                    quote!(::eggs::Egg::from_snapshot(&snapshot.#fi).map_err(|e| e.within(&#s))?)
                }
            })
            .collect();
        let members: Vec<&Member> = fields.iter().map(|f| &f.member).collect();
        let build = if unit {
            quote!(Self)
        } else if named {
            quote!(Self { #(#members: #exprs),* })
        } else {
            quote!(Self(#(#exprs),*))
        };
        quote!(::std::result::Result::Ok(#build))
    };

    // ---- diff -------------------------------------------------------------------------------
    let diff_stmts = captured.iter().filter(|f| !f.attrs.ignore_diff).map(|f| {
        let m = &f.member;
        let label = &f.label;
        if let Some(w) = &f.attrs.with {
            quote!(#w::diff(&self.#m, &other.#m, &path.child_field(#label), out);)
        } else if let Some(cmp) = &f.attrs.compare_with {
            quote! {
                if !#cmp(&self.#m, &other.#m) {
                    out.push(::eggs::Change::modified(
                        path.child_field(#label),
                        ::eggs::Value::new(::eggs::Egg::snapshot(&self.#m)),
                        ::eggs::Value::new(::eggs::Egg::snapshot(&other.#m)),
                    ));
                }
            }
        } else {
            quote!(::eggs::Egg::diff_into(&self.#m, &other.#m, &path.child_field(#label), out);)
        }
    });

    // ---- path-addressed operations ----------------------------------------------------------
    let type_name = name.to_string();
    let apply_arms = captured.iter().map(|f| {
        let m = &f.member;
        let label = &f.label;
        let body = if f.attrs.with.is_some() {
            quote! {
                ::std::result::Result::Err(::eggs::EggError::unsupported(
                    "apply_change",
                    ::std::format!("field `{}` uses a custom `with` adapter; patches cannot traverse it", #label),
                ))
            }
        } else {
            quote!(::eggs::Egg::apply_change(&mut self.#m, rest, change))
        };
        quote!(if name == #label { return #body; })
    });

    let restore_path_arms = captured.iter().map(|f| {
        let m = &f.member;
        let fi = f.snap_field.as_ref().unwrap();
        let label = &f.label;
        let body = if f.attrs.ignore_restore {
            quote! {
                ::std::result::Result::Err(::eggs::EggError::invalid_path(
                    ::eggs::EggPath::from_segments(path.to_vec()),
                    "field is excluded from restore (`#[egg(ignore_restore)]`)",
                ))
            }
        } else if let Some(w) = &f.attrs.with {
            quote! {
                if rest.is_empty() {
                    #w::restore(&mut self.#m, &snapshot.#fi)
                } else {
                    ::std::result::Result::Err(::eggs::EggError::invalid_path(
                        ::eggs::EggPath::from_segments(path.to_vec()),
                        "cannot address inside a field that uses a `with` adapter",
                    ))
                }
            }
        } else {
            quote!(::eggs::Egg::restore_path(&mut self.#m, rest, &snapshot.#fi).map_err(|e| e.within(first)))
        };
        quote!(if name == #label { return #body; })
    });

    let labels_list: Vec<&String> = captured.iter().map(|f| &f.label).collect();
    let heap_terms = captured
        .iter()
        .filter(|f| f.attrs.with.is_none())
        .map(|f| {
            let fi = f.snap_field.as_ref().unwrap();
            let ty = f.ty;
            quote!(+ <#ty as ::eggs::Egg>::snapshot_heap_size(&snapshot.#fi))
        });
    let is_empty_fn = if captured.is_empty() {
        quote! {
            fn snapshot_is_empty(_snapshot: &Self::Snapshot) -> bool { true }
        }
    } else {
        quote!()
    };
    let version_fn = version.map(|v| quote!(fn schema_version() -> u32 { #v }));

    // ---- generics ---------------------------------------------------------------------------
    let mut generics = input.generics.clone();
    {
        let where_clause = generics.make_where_clause();
        for f in captured.iter().filter(|f| f.attrs.with.is_none()) {
            let ty = f.ty;
            where_clause.predicates.push(parse_quote!(#ty: ::eggs::Egg));
        }
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let serde_attrs = if cfg!(feature = "serde") {
        quote! {
            #[derive(::eggs::serde::Serialize, ::eggs::serde::Deserialize)]
            #[serde(crate = "::eggs::serde")]
        }
    } else {
        quote!()
    };
    let snap_doc = format!("Captured state of `{name}`. Generated by `#[derive(Egg)]`; treat as opaque.");

    Ok(quote! {
        #serde_attrs
        #[derive(::std::clone::Clone, ::std::fmt::Debug, ::std::cmp::PartialEq)]
        #[allow(dead_code)]
        #[doc = #snap_doc]
        #vis struct #snap_ident<#(#params),*> {
            #(#snap_field_idents: #params,)*
        }

        impl #impl_generics ::eggs::Egg for #name #ty_generics #where_clause {
            type Snapshot = #snap_ident<#(#snap_args),*>;

            fn snapshot(&self) -> Self::Snapshot {
                #snap_ident { #(#snap_field_idents: #snapshot_exprs,)* }
            }

            #[allow(unused_variables)]
            fn restore_in_place(&mut self, snapshot: &Self::Snapshot) -> ::eggs::EggResult<()> {
                #(#restore_stmts)*
                ::std::result::Result::Ok(())
            }

            #[allow(unused_variables)]
            fn from_snapshot(snapshot: &Self::Snapshot) -> ::eggs::EggResult<Self> {
                #from_snapshot_body
            }

            #[allow(unused_variables)]
            fn diff_into(
                &self,
                other: &Self,
                path: &::eggs::EggPath,
                out: &mut ::std::vec::Vec<::eggs::Change>,
            ) {
                #(#diff_stmts)*
            }

            fn fields() -> &'static [&'static str] {
                &[#(#labels_list),*]
            }

            #version_fn

            #[allow(unused_variables)]
            fn restore_path(
                &mut self,
                path: &[::eggs::PathSegment],
                snapshot: &Self::Snapshot,
            ) -> ::eggs::EggResult<()> {
                let ::std::option::Option::Some((first, rest)) = path.split_first() else {
                    return ::eggs::Egg::restore_in_place(self, snapshot);
                };
                if let ::eggs::PathSegment::Field(name) = first {
                    #(#restore_path_arms)*
                }
                ::std::result::Result::Err(::eggs::EggError::invalid_path(
                    ::eggs::EggPath::from_segments(path.to_vec()),
                    ::std::format!("no restorable field `{}` in `{}`", first, #type_name),
                ))
            }

            #[allow(unused_variables)]
            fn apply_change(
                &mut self,
                path: &[::eggs::PathSegment],
                change: &::eggs::Change,
            ) -> ::eggs::EggResult<()> {
                let ::std::option::Option::Some((first, rest)) = path.split_first() else {
                    return ::eggs::apply_whole(self, change);
                };
                if let ::eggs::PathSegment::Field(name) = first {
                    #(#apply_arms)*
                }
                ::std::result::Result::Err(::eggs::EggError::invalid_path(
                    &change.path,
                    ::std::format!("no field `{}` in `{}`", first, #type_name),
                ))
            }

            #is_empty_fn

            #[allow(unused_variables)]
            fn snapshot_heap_size(snapshot: &Self::Snapshot) -> usize {
                0usize #(#heap_terms)*
            }
        }
    })
}
