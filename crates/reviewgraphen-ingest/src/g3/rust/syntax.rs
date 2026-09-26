//! Identity-free Rust AST observations. The caller of this private parser supplies
//! syntax context only; the root ingest pipeline alone binds its result.
use super::types::{
    RawKey, RawRustFileV1, RawRustParseObstructionV1, RustExclusionV1,
    RustInclusiveLineColumnRange as Range, RustLhsKindV1, RustLimitationV1,
};
use proc_macro2::Span;
use std::collections::BTreeMap;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, Item, ItemFn};

fn span(start: Span, end: Span) -> Result<Range, RawRustParseObstructionV1> {
    let start = start.start();
    let end = end.end();
    Range::new(
        u64::try_from(start.line).map_err(|_| RawRustParseObstructionV1::InvalidRange)?,
        u64::try_from(start.column)
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or(RawRustParseObstructionV1::InvalidRange)?,
        u64::try_from(end.line).map_err(|_| RawRustParseObstructionV1::InvalidRange)?,
        u64::try_from(end.column).map_err(|_| RawRustParseObstructionV1::InvalidRange)?,
    )
    .ok_or(RawRustParseObstructionV1::InvalidRange)
}
fn range(value: Span) -> Result<Range, RawRustParseObstructionV1> {
    span(value, value)
}

pub(super) fn module_label(path: &str) -> String {
    let trimmed = path.trim_end_matches(".rs");
    let segments = trimmed.split('/').collect::<Vec<_>>();
    let parts: Vec<String> = match segments.as_slice() {
        ["src", "lib" | "main"] => Vec::new(),
        ["src", rest @ ..] => rest
            .iter()
            .filter(|s| **s != "mod")
            .map(|s| (*s).to_owned())
            .collect(),
        ["tests", rest @ ..] => std::iter::once("tests".to_owned())
            .chain(rest.iter().map(|s| (*s).to_owned()))
            .collect(),
        _ => segments.iter().map(|s| (*s).to_owned()).collect(),
    };
    if parts.is_empty() {
        "crate".to_owned()
    } else {
        format!("crate::{}", parts.join("::"))
    }
}

fn keys_for_items(
    items: &[Item],
    label: &str,
    output: &mut Vec<RawKey>,
    exclusions: &mut Vec<RustExclusionV1>,
    accepted_locations: &mut BTreeMap<RawKey, Range>,
    path: &str,
    omit_declarations: bool,
) -> Result<(), RawRustParseObstructionV1> {
    for item in items {
        match item {
            Item::Fn(function) => {
                let logical_name = format!("{label}::{}", function.sig.ident);
                let syntax = span(function.sig.fn_token.span, function.span())?;
                let declaration = RawKey::Declaration {
                    occurrence: syntax,
                    logical_name: logical_name.clone(),
                };
                accepted_locations.insert(declaration.clone(), range(function.span())?);
                if !omit_declarations {
                    output.push(declaration);
                }
                for attribute in &function.attrs {
                    if attribute.path().is_ident("test") {
                        let marker = RawKey::TestAttribute {
                            occurrence: range(attribute.span())?,
                            function_syntax: syntax,
                            owner_logical_name: logical_name.clone(),
                        };
                        accepted_locations.insert(marker.clone(), range(function.span())?);
                        output.push(marker);
                    } else {
                        exclusions.push(RustExclusionV1 {
                            path: path.to_owned(),
                            range: range(attribute.span())?,
                            kind: "non_exact_test_attribute",
                        });
                    }
                }
            }
            Item::Mod(module) => {
                if let Some((_, children)) = &module.content {
                    let child_label = format!("{label}::{}", module.ident);
                    let parent = span(module.mod_token.span, module.span())?;
                    for child in children {
                        if let Item::Fn(function) = child {
                            let pair = RawKey::Containment {
                                parent,
                                child: span(function.sig.fn_token.span, function.span())?,
                                parent_logical_name: child_label.clone(),
                                child_logical_name: format!(
                                    "{child_label}::{}",
                                    function.sig.ident
                                ),
                            };
                            accepted_locations.insert(pair.clone(), range(function.span())?);
                            output.push(pair);
                        }
                    }
                    keys_for_items(
                        children,
                        &child_label,
                        output,
                        exclusions,
                        accepted_locations,
                        path,
                        omit_declarations,
                    )?;
                } else {
                    exclusions.push(RustExclusionV1 {
                        path: path.to_owned(),
                        range: span(module.mod_token.span, module.span())?,
                        kind: "out_of_line_module",
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn lhs(expr: &Expr) -> (RustLhsKindV1, Option<String>) {
    match expr {
        Expr::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 => (
            RustLhsKindV1::Identifier,
            Some(path.path.segments[0].ident.to_string()),
        ),
        Expr::Path(_) => (RustLhsKindV1::Other, None),
        Expr::Field(_) => (RustLhsKindV1::Field, None),
        Expr::Index(_) => (RustLhsKindV1::Index, None),
        Expr::Unary(_) => (RustLhsKindV1::Dereference, None),
        Expr::Tuple(_) => (RustLhsKindV1::Tuple, None),
        Expr::Assign(_) => (RustLhsKindV1::Other, None),
        _ => (RustLhsKindV1::Pattern, None),
    }
}

fn compound(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::AddAssign(_)
            | syn::BinOp::SubAssign(_)
            | syn::BinOp::MulAssign(_)
            | syn::BinOp::DivAssign(_)
            | syn::BinOp::RemAssign(_)
            | syn::BinOp::BitXorAssign(_)
            | syn::BinOp::BitAndAssign(_)
            | syn::BinOp::BitOrAssign(_)
            | syn::BinOp::ShlAssign(_)
            | syn::BinOp::ShrAssign(_)
    )
}

struct AssignVisitor<'a> {
    path: &'a str,
    modules: Vec<String>,
    owner: Option<(Range, String, Range)>,
    unsupported_depth: usize,
    // The protected producer can record a state write from a method or a
    // closure even though neither has a supported G3 free-function owner.
    // Keep its first-location witness independent of G3's ownership policy.
    legacy_source_active: bool,
    keys: Vec<RawKey>,
    exclusions: Vec<RustExclusionV1>,
    limitations: Vec<RustLimitationV1>,
    first: BTreeMap<String, Range>,
    accepted_locations: BTreeMap<RawKey, Range>,
    error: Option<RawRustParseObstructionV1>,
}
impl AssignVisitor<'_> {
    fn push(&mut self, key: Result<RawKey, RawRustParseObstructionV1>) {
        match key {
            Ok(key) => self.keys.push(key),
            Err(err) => self.error = Some(err),
        }
    }
    fn excluded(&mut self, value: Span, kind: &'static str) {
        match range(value) {
            Ok(range) => self.exclusions.push(RustExclusionV1 {
                path: self.path.to_owned(),
                range,
                kind,
            }),
            Err(err) => self.error = Some(err),
        }
    }
    fn first_write(&mut self, expr: &Expr, s: Span) {
        if !self.legacy_source_active {
            return;
        }
        let Some(label) = legacy_label(expr) else {
            return;
        };
        match range(s) {
            Ok(r) => {
                self.first
                    .entry(label)
                    .and_modify(|previous| {
                        if (r.start_line(), r.start_column())
                            < (previous.start_line(), previous.start_column())
                        {
                            *previous = r;
                        }
                    })
                    .or_insert(r);
            }
            Err(err) => self.error = Some(err),
        }
    }
}

// Mirrors only the old producer's label grammar for first state location checking.
fn legacy_label(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(p) if p.qself.is_none() => Some(
            p.path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::"),
        ),
        Expr::Field(f) => legacy_label(&f.base).map(|base| match &f.member {
            syn::Member::Named(member) => format!("{base}.{member}"),
            syn::Member::Unnamed(member) => format!("{base}.{}", member.index),
        }),
        Expr::Index(i) => legacy_label(&i.expr).map(|base| format!("{base}[..]")),
        _ => None,
    }
}

impl<'ast> Visit<'ast> for AssignVisitor<'_> {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if let Some((_, items)) = &module.content {
            let block_local = self.owner.is_some() || self.unsupported_depth > 0;
            if block_local {
                self.excluded(module.span(), "block_local_module");
                self.unsupported_depth += 1;
            }
            self.modules.push(module.ident.to_string());
            for item in items {
                self.visit_item(item);
            }
            self.modules.pop();
            if block_local {
                self.unsupported_depth -= 1;
            }
        }
    }
    fn visit_item_fn(&mut self, function: &'ast ItemFn) {
        if self.owner.is_some() || self.unsupported_depth > 0 {
            self.excluded(function.span(), "block_local_function");
            self.unsupported_depth += 1;
            let previous = std::mem::replace(&mut self.legacy_source_active, false);
            visit::visit_item_fn(self, function);
            self.legacy_source_active = previous;
            self.unsupported_depth -= 1;
            return;
        }
        match span(function.sig.fn_token.span, function.span()) {
            Ok(syntax) => {
                let label = format!("{}::{}", self.modules.join("::"), function.sig.ident);
                let Ok(full) = range(function.span()) else {
                    self.error = Some(RawRustParseObstructionV1::InvalidRange);
                    return;
                };
                self.owner = Some((syntax, label, full));
                let previous = std::mem::replace(&mut self.legacy_source_active, true);
                visit::visit_item_fn(self, function);
                self.legacy_source_active = previous;
                self.owner = None;
            }
            Err(err) => self.error = Some(err),
        }
    }
    fn visit_impl_item_fn(&mut self, method: &'ast syn::ImplItemFn) {
        self.excluded(method.span(), "method");
        let accepted_method = self.owner.is_none() && self.unsupported_depth == 0;
        self.unsupported_depth += 1;
        let previous = std::mem::replace(&mut self.legacy_source_active, accepted_method);
        visit::visit_impl_item_fn(self, method);
        self.legacy_source_active = previous;
        self.unsupported_depth -= 1;
    }
    fn visit_trait_item_fn(&mut self, method: &'ast syn::TraitItemFn) {
        // A default method inside a block-local trait must not inherit the
        // enclosing free function's accepted write origin. A signature with
        // no body still receives a typed exclusion witness.
        self.excluded(method.span(), "trait_method");
        self.unsupported_depth += 1;
        visit::visit_trait_item_fn(self, method);
        self.unsupported_depth -= 1;
    }
    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        self.excluded(closure.span(), "closure");
        self.unsupported_depth += 1;
        visit::visit_expr_closure(self, closure);
        self.unsupported_depth -= 1;
    }
    fn visit_expr_assign(&mut self, assign: &'ast syn::ExprAssign) {
        let key = (|| {
            let occurrence = range(assign.span())?;
            let lhs_range = range(assign.left.span())?;
            let (lhs_kind, lhs_name) = lhs(&assign.left);
            let owner = self.owner.as_ref().filter(|_| self.unsupported_depth == 0);
            Ok(RawKey::Assignment {
                occurrence,
                lhs: lhs_range,
                lhs_kind,
                lhs_name,
                owner_syntax: owner.map(|(syntax, _, _)| *syntax),
                owner_logical_name: owner.map(|(_, name, _)| name.clone()),
            })
        })();
        if let (Ok(key), Some((_, _, full))) = (&key, &self.owner)
            && self.unsupported_depth == 0
        {
            self.accepted_locations.insert(key.clone(), *full);
        }
        self.push(key);
        self.first_write(&assign.left, assign.span());
        visit::visit_expr_assign(self, assign);
    }
    fn visit_expr_binary(&mut self, binary: &'ast syn::ExprBinary) {
        if compound(&binary.op) {
            self.excluded(binary.span(), "CompoundAssignment");
            self.first_write(&binary.left, binary.span());
        }
        visit::visit_expr_binary(self, binary);
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        match range(mac.span()) {
            Ok(range) => self.limitations.push(RustLimitationV1 {
                path: self.path.to_owned(),
                range,
                kind: "opaque_macro_expansion",
                latent_occurrence_count: "unknown",
            }),
            Err(err) => self.error = Some(err),
        }
    }
}

fn assignments<'a>(
    file: &syn::File,
    path: &'a str,
    root: &str,
) -> Result<AssignVisitor<'a>, RawRustParseObstructionV1> {
    let mut visitor = AssignVisitor {
        path,
        modules: vec![root.to_owned()],
        owner: None,
        unsupported_depth: 0,
        legacy_source_active: false,
        keys: Vec::new(),
        exclusions: Vec::new(),
        limitations: Vec::new(),
        first: BTreeMap::new(),
        accepted_locations: BTreeMap::new(),
        error: None,
    };
    visitor.visit_file(file);
    if let Some(err) = visitor.error {
        return Err(err);
    }
    Ok(visitor)
}

// Independent walk over parsed AST items. In particular, this census never
// invokes keys_for_items or AssignVisitor: dropping a push/visit in either raw
// emitter retains the occurrence here and fails the full-key multiset check.
fn census_items(
    items: &[Item],
    module_name: &str,
    keys: &mut Vec<RawKey>,
) -> Result<(), RawRustParseObstructionV1> {
    for item in items {
        if let Item::Fn(function) = item {
            let logical_name = format!("{module_name}::{}", function.sig.ident);
            let occurrence = span(function.sig.fn_token.span, function.span())?;
            keys.push(RawKey::Declaration {
                occurrence,
                logical_name: logical_name.clone(),
            });
            for attr in &function.attrs {
                if attr.path().is_ident("test") {
                    keys.push(RawKey::TestAttribute {
                        occurrence: range(attr.span())?,
                        function_syntax: occurrence,
                        owner_logical_name: logical_name.clone(),
                    });
                }
            }
        } else if let Item::Mod(module) = item
            && let Some((_, children)) = &module.content
        {
            let nested_name = format!("{module_name}::{}", module.ident);
            let module_range = span(module.mod_token.span, module.span())?;
            for child in children {
                if let Item::Fn(function) = child {
                    keys.push(RawKey::Containment {
                        parent: module_range,
                        child: span(function.sig.fn_token.span, function.span())?,
                        parent_logical_name: nested_name.clone(),
                        child_logical_name: format!("{nested_name}::{}", function.sig.ident),
                    });
                }
            }
            census_items(children, &nested_name, keys)?;
        }
    }
    Ok(())
}

fn census_lhs(expr: &Expr) -> (RustLhsKindV1, Option<String>) {
    match expr {
        Expr::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 => (
            RustLhsKindV1::Identifier,
            Some(path.path.segments[0].ident.to_string()),
        ),
        Expr::Path(_) => (RustLhsKindV1::Other, None),
        Expr::Field(_) => (RustLhsKindV1::Field, None),
        Expr::Index(_) => (RustLhsKindV1::Index, None),
        Expr::Unary(_) => (RustLhsKindV1::Dereference, None),
        Expr::Tuple(_) => (RustLhsKindV1::Tuple, None),
        Expr::Assign(_) => (RustLhsKindV1::Other, None),
        _ => (RustLhsKindV1::Pattern, None),
    }
}

struct CensusAssignments {
    module_names: Vec<String>,
    owner: Option<(Range, String)>,
    unsupported_depth: usize,
    keys: Vec<RawKey>,
    error: Option<RawRustParseObstructionV1>,
}

impl<'ast> Visit<'ast> for CensusAssignments {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if let Some((_, children)) = &module.content {
            let unsupported = self.owner.is_some() || self.unsupported_depth > 0;
            self.unsupported_depth += usize::from(unsupported);
            self.module_names.push(module.ident.to_string());
            for child in children {
                self.visit_item(child);
            }
            self.module_names.pop();
            self.unsupported_depth -= usize::from(unsupported);
        }
    }
    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        if self.owner.is_some() || self.unsupported_depth > 0 {
            self.unsupported_depth += 1;
            visit::visit_item_fn(self, function);
            self.unsupported_depth -= 1;
            return;
        }
        match span(function.sig.fn_token.span, function.span()) {
            Ok(range) => {
                self.owner = Some((
                    range,
                    format!("{}::{}", self.module_names.join("::"), function.sig.ident),
                ));
                visit::visit_item_fn(self, function);
                self.owner = None;
            }
            Err(err) => self.error = Some(err),
        }
    }
    fn visit_impl_item_fn(&mut self, method: &'ast syn::ImplItemFn) {
        self.unsupported_depth += 1;
        visit::visit_impl_item_fn(self, method);
        self.unsupported_depth -= 1;
    }
    fn visit_trait_item_fn(&mut self, method: &'ast syn::TraitItemFn) {
        self.unsupported_depth += 1;
        visit::visit_trait_item_fn(self, method);
        self.unsupported_depth -= 1;
    }
    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        self.unsupported_depth += 1;
        visit::visit_expr_closure(self, closure);
        self.unsupported_depth -= 1;
    }
    fn visit_expr_assign(&mut self, assign: &'ast syn::ExprAssign) {
        let observed = (|| {
            let occurrence = range(assign.span())?;
            let lhs = range(assign.left.span())?;
            let (lhs_kind, lhs_name) = census_lhs(&assign.left);
            let owner = self.owner.as_ref().filter(|_| self.unsupported_depth == 0);
            Ok(RawKey::Assignment {
                occurrence,
                lhs,
                lhs_kind,
                lhs_name,
                owner_syntax: owner.map(|(syntax, _)| *syntax),
                owner_logical_name: owner.map(|(_, name)| name.clone()),
            })
        })();
        match observed {
            Ok(key) => self.keys.push(key),
            Err(err) => self.error = Some(err),
        }
        visit::visit_expr_assign(self, assign);
    }
}

fn independent_census(
    file: &syn::File,
    root: &str,
) -> Result<Vec<RawKey>, RawRustParseObstructionV1> {
    let mut keys = Vec::new();
    census_items(&file.items, root, &mut keys)?;
    let mut assignments = CensusAssignments {
        module_names: vec![root.to_owned()],
        owner: None,
        unsupported_depth: 0,
        keys: Vec::new(),
        error: None,
    };
    assignments.visit_file(file);
    if let Some(err) = assignments.error {
        return Err(err);
    }
    keys.extend(assignments.keys);
    Ok(keys)
}

pub(crate) fn parse_rust_g3(
    bytes: &[u8],
    canonical_path: &str,
) -> Result<RawRustFileV1, RawRustParseObstructionV1> {
    parse_rust_g3_with_policy(bytes, canonical_path, false)
}

// An omission inside the raw emitter must be caught by the independently
// enumerated AST census. This raw-only fault hook cannot bind accepted facts.
fn parse_rust_g3_with_policy(
    bytes: &[u8],
    canonical_path: &str,
    omit_declarations: bool,
) -> Result<RawRustFileV1, RawRustParseObstructionV1> {
    let text = std::str::from_utf8(bytes).map_err(|_| RawRustParseObstructionV1::InvalidUtf8)?;
    let ast = syn::parse_file(text).map_err(|_| RawRustParseObstructionV1::ParseFailed)?;
    let root = module_label(canonical_path);
    let mut output = Vec::new();
    let mut exclusions = Vec::new();
    let mut accepted_locations = BTreeMap::new();
    keys_for_items(
        &ast.items,
        &root,
        &mut output,
        &mut exclusions,
        &mut accepted_locations,
        canonical_path,
        omit_declarations,
    )?;
    let visited = assignments(&ast, canonical_path, &root)?;
    output.extend(visited.keys.iter().cloned());
    accepted_locations.extend(
        visited
            .accepted_locations
            .iter()
            .map(|(k, v)| (k.clone(), *v)),
    );
    exclusions.extend(visited.exclusions);
    // This AST census has its own visitors and cannot inherit an omission in
    // keys_for_items/AssignVisitor. The raw-only omission injection above is
    // therefore detectable as MissingOccurrence, including inside modules.
    let census = independent_census(&ast, &root)?;
    Ok(RawRustFileV1 {
        declarations: output
            .iter()
            .filter(|key| matches!(key, RawKey::Declaration { .. }))
            .cloned()
            .collect(),
        containment: output
            .iter()
            .filter(|key| matches!(key, RawKey::Containment { .. }))
            .cloned()
            .collect(),
        assignments: output
            .iter()
            .filter(|key| matches!(key, RawKey::Assignment { .. }))
            .cloned()
            .collect(),
        test_attributes: output
            .iter()
            .filter(|key| matches!(key, RawKey::TestAttribute { .. }))
            .cloned()
            .collect(),
        census,
        exclusions,
        limitations: visited.limitations,
        legacy_first_writes: visited.first,
        accepted_locations,
    })
}

#[cfg(test)]
mod owner_tests {
    use super::super::partition::test_fixture;
    use super::super::types::{RustG3OutcomeV1, RustG3ReasonV1};

    #[test]
    fn trait_default_method_cannot_inherit_outer_free_function_write_owner() {
        let source = "trait Top { fn signature(&self); }\nfn outer() {\n    trait Local {\n        fn inner() {\n            let mut value = 0;\n            value = 1;\n        }\n    }\n}\n";
        let result = test_fixture::committed(source).ingest();
        let entry = &result.source_bundle.entries()[0];
        assert_eq!(entry.bytes(), source.as_bytes(), "real committed Git bytes");
        let accepted_outer = result
            .program_space
            .artifacts()
            .iter()
            .find(|a| a.kind == "function" && a.label == "crate::structural::outer")
            .expect("legacy producer accepted the outer free function");
        assert!(
            result
                .program_space
                .relations()
                .iter()
                .any(|r| r.kind == "writes" && r.source_id == accepted_outer.id),
            "legacy producer has an outer write eligible for erroneous joining"
        );
        let batch = result.g3_observations().expect("Git-bound G3 batch");
        assert_eq!(
            batch.writes().len(),
            1,
            "trait default method assignment is counted"
        );
        let row = &batch.writes()[0];
        assert_eq!(row.source().file_id(), entry.artifact_id());
        assert!(
            matches!(
                row.outcome(),
                RustG3OutcomeV1::Obstructed {
                    reason: RustG3ReasonV1::UnsupportedOwner,
                    ..
                }
            ),
            "trait body cannot reuse enclosing outer's accepted writes relation"
        );
        assert!(
            batch
                .construct_partition()
                .exclusions()
                .iter()
                .filter(|x| x.kind == "trait_method")
                .count()
                >= 2,
            "both top-level signature and nested default method have explicit exclusions"
        );
    }
}

#[cfg(test)]
mod census_tests {
    use super::super::partition::check_multiset;
    use super::super::types::G3AccountingError;
    use super::*;

    #[test]
    fn omitted_declaration_emission_remains_in_independent_ast_census() {
        let raw = parse_rust_g3_with_policy(b"fn observed() {}\n", "src/lib.rs", true)
            .expect("valid syntax despite fault-injected dropped emitter row");
        let emitted = raw
            .declarations
            .iter()
            .chain(&raw.containment)
            .chain(&raw.assignments)
            .chain(&raw.test_attributes)
            .cloned()
            .collect::<Vec<_>>();
        assert!(
            raw.declarations.is_empty(),
            "fault omission actually occurred"
        );
        assert!(
            matches!(
                check_multiset(&raw.census, &emitted),
                Err(G3AccountingError::MissingOccurrence { .. })
            ),
            "census must retain declaration when its independent output emitter loses it"
        );
    }
}
