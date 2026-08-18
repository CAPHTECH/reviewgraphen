use crate::git::GitSnapshot;
use crate::{
    AdapterReport, AdapterStatus, ArtifactDraft, CapabilityState, IngestionObstructionKind,
    IssueDraft, LocationDraft, ObstructionSeverity, RelationDraft,
};
use proc_macro2::Span;
use quote::ToTokens;
use reviewgraphen_core::{ContentHash, RustSymbolAnchorV1, RustSymbolKindV1, StableId};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    Arm, Attribute, Block, Expr, ExprAssign, ExprAwait, ExprBinary, ExprCall, ExprClosure,
    ExprForLoop, ExprIf, ExprMethodCall, ExprWhile, Fields, File, FnArg, ImplItem, Item, ItemFn,
    ItemImpl, ItemMacro, ItemMod, Local, Macro, Pat, Signature, Type, UseTree, Visibility,
};

/// Extracts only the M2 Rust AST subset from immutable Git file bytes.
pub(crate) fn extract(snapshot: &GitSnapshot, _snapshot_id: &StableId) -> RustExtraction {
    let rust_files = snapshot
        .files
        .iter()
        .filter(|file| file.path.ends_with(".rs"))
        .collect::<Vec<_>>();
    let mut parsed = Vec::new();
    let mut issues = Vec::new();
    for source in &rust_files {
        match std::str::from_utf8(&source.content)
            .map_err(|error| error.to_string())
            .and_then(|text| syn::parse_file(text).map_err(|error| error.to_string()))
        {
            Ok(ast) => parsed.push(ParsedFile::new(source, ast)),
            Err(error) => issues.push(IssueDraft {
                kind: IngestionObstructionKind::ParseFailure,
                severity: ObstructionSeverity::High,
                description: format!(
                    "syn could not parse Rust source at {}:{}:{}: {error}",
                    source.path, 1, 1
                ),
                source_keys: BTreeSet::from([format!("file:{}", source.path)]),
                paths: BTreeSet::from([source.path.clone()]),
                related_capabilities: BTreeSet::from([
                    "ast".to_owned(),
                    "containment".to_owned(),
                    // An unparsed file's declarations, `await` points, and
                    // concurrency-primitive occurrences are just as unread
                    // as its containment, so the same limitation justifies
                    // the same downgrade for all three.
                    "concurrency_model".to_owned(),
                ]),
            }),
        }
    }

    let mut artifacts = Vec::new();
    let mut relations = Vec::new();
    let mut resolver = BTreeMap::<String, Vec<String>>::new();
    for file in &mut parsed {
        collect_file_declarations(
            file,
            &mut artifacts,
            &mut relations,
            &mut issues,
            &mut resolver,
        );
    }
    let symbol_anchors = parsed
        .iter()
        .flat_map(file_symbol_anchors)
        .collect::<BTreeMap<_, _>>();
    for candidates in resolver.values_mut() {
        candidates.sort();
        candidates.dedup();
    }

    let mut state_artifacts = BTreeMap::<String, ArtifactDraft>::new();
    for file in &parsed {
        let mut visitor = FunctionBodyVisitor::new(&file.path, &file.function_sources, &resolver);
        visitor.visit_file(&file.ast);
        issues.extend(visitor.issues);
        for write in visitor.writes {
            let state_key = format!("state:{}:{}", file.path, write.label);
            state_artifacts
                .entry(state_key.clone())
                .or_insert_with(|| ArtifactDraft {
                    key: state_key.clone(),
                    id_kind: "state",
                    kind: "state",
                    label: write.label.clone(),
                    language: Some("rust"),
                    location: Some(write.location.clone()),
                    content_hash: Some(file.content_hash.clone()),
                    attributes: Map::from_iter([(
                        "syntactic_write_target".to_owned(),
                        Value::Bool(true),
                    )]),
                    source_path: Some(file.path.clone()),
                    extraction_method: "reviewgraphen.ingest.rust_syn.v1",
                });
            relations.push(RelationDraft {
                kind: "writes",
                source_key: write.source_key,
                target_keys: vec![state_key],
                attributes: Map::from_iter([(
                    "line".to_owned(),
                    Value::Number(write.location.start_line.into()),
                )]),
                source_path: Some(file.path.clone()),
                extraction_method: "reviewgraphen.ingest.rust_syn.v1",
            });
        }
        relations.extend(visitor.relations);
    }
    artifacts.extend(state_artifacts.into_values());
    let parse_failed = !issues
        .iter()
        .filter(|issue| issue.kind == IngestionObstructionKind::ParseFailure)
        .collect::<Vec<_>>()
        .is_empty();
    let mut capabilities = BTreeMap::new();
    capabilities.insert(
        "ast".to_owned(),
        if parse_failed {
            CapabilityState::Partial
        } else {
            CapabilityState::Complete
        },
    );
    capabilities.insert(
        "containment".to_owned(),
        if parse_failed {
            CapabilityState::Partial
        } else {
            CapabilityState::Complete
        },
    );
    // Scoped exactly like `ast`/`containment` above, and complete under the
    // same single condition, because it is read off the same unexpanded
    // syntax tree with no resolution step of its own: see
    // `ConcurrencyMarkers` for what this capability does and does not
    // claim. A parse failure is the only observed condition that leaves
    // part of that tree unread, so it is the only one that downgrades this
    // capability -- and when it does, the same `parse_failure` limitation
    // that justifies `ast`/`containment`'s downgrade names
    // `concurrency_model` too (ADR 0011 point 5 requires every
    // non-`complete` state to have a related, source-backed limitation).
    // Deliberately *not* one of the five permanently-`partial`,
    // resolution-bounded capabilities below: none of the facts behind this
    // one are claims about which item a name refers to.
    capabilities.insert(
        "concurrency_model".to_owned(),
        if parse_failed {
            CapabilityState::Partial
        } else {
            CapabilityState::Complete
        },
    );
    capabilities.insert("direct_calls".to_owned(), CapabilityState::Partial);
    capabilities.insert("imports".to_owned(), CapabilityState::Partial);
    capabilities.insert("module_dependencies".to_owned(), CapabilityState::Partial);
    capabilities.insert("test_mapping".to_owned(), CapabilityState::Partial);
    capabilities.insert("state_writes".to_owned(), CapabilityState::Partial);

    // These five capabilities are unconditionally `partial` (see module docs):
    // even a snapshot with no unresolved instance is still bounded to unique
    // local syntactic targets. v2 requires every `partial` declaration to
    // have a related, source-backed limitation, so retain this one
    // deterministically instead of only emitting it when a specific
    // unresolved call/import/write is observed.
    let bounded_capabilities = [
        "direct_calls".to_owned(),
        "imports".to_owned(),
        "module_dependencies".to_owned(),
        "test_mapping".to_owned(),
        "state_writes".to_owned(),
    ];
    let rust_source_keys = parsed
        .iter()
        .map(|file| format!("file:{}", file.path))
        .collect::<BTreeSet<_>>();
    issues.push(IssueDraft {
        kind: IngestionObstructionKind::RelationUnresolved,
        severity: ObstructionSeverity::Info,
        description: "M2 direct-call, import, module-dependency, test-coverage, and \
            state-write extraction is bounded to unique local syntactic targets; \
            cross-crate resolution and dynamic dispatch remain unresolved regardless \
            of whether this snapshot happens to exercise only resolved examples"
            .to_owned(),
        source_keys: rust_source_keys.clone(),
        paths: parsed.iter().map(|file| file.path.clone()).collect(),
        related_capabilities: bounded_capabilities.iter().cloned().collect(),
    });
    let mut capability_sources = BTreeMap::<String, BTreeSet<String>>::new();
    for capability in ["ast", "concurrency_model", "containment"]
        .into_iter()
        .map(ToOwned::to_owned)
        .chain(bounded_capabilities)
    {
        capability_sources.insert(capability, rust_source_keys.clone());
    }
    RustExtraction {
        artifacts,
        relations,
        symbol_anchors,
        issues,
        capabilities,
        capability_sources,
        adapter_report: AdapterReport {
            id: "reviewgraphen.ingest.rust_syn".to_owned(),
            version: "1".to_owned(),
            // Direct dispatch, macro expansion, and cross-crate resolution are
            // intentionally outside M2 even if this particular snapshot has no
            // such construct.
            status: AdapterStatus::Partial,
            parsed: Some(u64::try_from(parsed.len()).expect("usize fits u64")),
            total: Some(u64::try_from(rust_files.len()).expect("usize fits u64")),
            excluded: Some(0),
            failed: Some(u64::try_from(rust_files.len() - parsed.len()).expect("usize fits u64")),
        },
    }
}

pub(crate) struct RustExtraction {
    pub(crate) artifacts: Vec<ArtifactDraft>,
    pub(crate) relations: Vec<RelationDraft>,
    pub(crate) symbol_anchors: BTreeMap<String, RustSymbolAnchorV1>,
    pub(crate) issues: Vec<IssueDraft>,
    pub(crate) capabilities: BTreeMap<String, CapabilityState>,
    pub(crate) capability_sources: BTreeMap<String, BTreeSet<String>>,
    pub(crate) adapter_report: AdapterReport,
}

struct ParsedFile {
    path: String,
    content_hash: ContentHash,
    ast: File,
    root_module_key: String,
    root_module_label: String,
    function_sources: BTreeMap<SpanKey, FunctionSource>,
}

impl ParsedFile {
    fn new(source: &crate::git::SnapshotFile, ast: File) -> Self {
        let root_module_label = module_label(&source.path);
        Self {
            path: source.path.clone(),
            content_hash: source.content_hash.clone(),
            ast,
            root_module_key: format!("module:{}:{root_module_label}", source.path),
            root_module_label,
            function_sources: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SpanKey {
    line: usize,
    column: usize,
}

impl SpanKey {
    fn from(span: Span) -> Self {
        let start = span.start();
        Self {
            line: start.line,
            column: start.column,
        }
    }
}

#[derive(Clone)]
struct FunctionSource {
    source_key: String,
    /// The enclosing module's logical label (for example `crate::api`), used
    /// to normalize a `crate`/`self`/`super`-qualified call path at this
    /// call site before resolving it.
    module_label: String,
}

fn collect_file_declarations(
    file: &mut ParsedFile,
    artifacts: &mut Vec<ArtifactDraft>,
    relations: &mut Vec<RelationDraft>,
    issues: &mut Vec<IssueDraft>,
    resolver: &mut BTreeMap<String, Vec<String>>,
) {
    artifacts.push(module_artifact(
        file.root_module_key.clone(),
        file.root_module_label.clone(),
        &file.path,
        file.content_hash.clone(),
    ));
    relations.push(RelationDraft {
        kind: "contains",
        source_key: format!("file:{}", file.path),
        target_keys: vec![file.root_module_key.clone()],
        attributes: Map::new(),
        source_path: Some(file.path.clone()),
        extraction_method: "reviewgraphen.ingest.rust_syn.v1",
    });
    let context = DeclarationContext {
        module_key: file.root_module_key.clone(),
        module_label: file.root_module_label.clone(),
        test_scope: false,
    };
    let items = file.ast.items.clone();
    collect_items(
        &items, file, context, artifacts, relations, issues, resolver,
    );
}

#[derive(Clone)]
struct DeclarationContext {
    module_key: String,
    module_label: String,
    test_scope: bool,
}

/// Builds the complete v1 symbol-anchor set from the same parsed AST and the
/// same artifact-key derivation used by declaration extraction. `ToTokens`
/// drops source spans, comments, and formatting while retaining identifiers,
/// paths, visibility, qualifiers, ABI, generics, types, and parsed bodies.
fn file_symbol_anchors(file: &ParsedFile) -> BTreeMap<String, RustSymbolAnchorV1> {
    let mut anchors = BTreeMap::new();
    collect_item_anchors(&file.ast.items, file, &file.root_module_label, &mut anchors);
    anchors
}

fn collect_item_anchors(
    items: &[Item],
    file: &ParsedFile,
    module_label: &str,
    anchors: &mut BTreeMap<String, RustSymbolAnchorV1>,
) {
    for item in items {
        match item {
            Item::Fn(function) => {
                let logical_name = format!("{module_label}::{}", function.sig.ident);
                let location = location(&file.path, function.span());
                let key = symbol_key(&file.path, "function", &logical_name, &location);
                insert_anchor(
                    anchors,
                    key,
                    RustSymbolKindV1::Function,
                    format!(
                        "{} {} {}",
                        attribute_tokens(&function.attrs),
                        function.vis.to_token_stream(),
                        function.sig.to_token_stream()
                    ),
                    function.block.to_token_stream().to_string(),
                );
            }
            Item::Impl(implementation) => {
                let owner = impl_owner(&implementation.self_ty);
                let trait_path = implementation
                    .trait_
                    .as_ref()
                    .map(|(bang, path, _)| {
                        format!("{} {}", bang.to_token_stream(), path.to_token_stream())
                    })
                    .unwrap_or_default();
                for implementation_item in &implementation.items {
                    let ImplItem::Fn(method) = implementation_item else {
                        continue;
                    };
                    let logical_name = format!("{module_label}::{}::{owner}", method.sig.ident);
                    let location = location(&file.path, method.span());
                    let key = symbol_key(&file.path, "method", &logical_name, &location);
                    insert_anchor(
                        anchors,
                        key,
                        RustSymbolKindV1::Method,
                        format!(
                            "{} {} {} {} {} {} {} {} {}",
                            attribute_tokens(&implementation.attrs),
                            implementation.defaultness.to_token_stream(),
                            implementation.unsafety.to_token_stream(),
                            implementation.generics.to_token_stream(),
                            trait_path,
                            implementation.self_ty.to_token_stream(),
                            attribute_tokens(&method.attrs),
                            method.vis.to_token_stream(),
                            method.sig.to_token_stream(),
                        ),
                        method.block.to_token_stream().to_string(),
                    );
                }
            }
            Item::Struct(value) => insert_type_anchor(
                anchors,
                file,
                module_label,
                "struct",
                &value.ident.to_string(),
                value.span(),
                value,
                &value.vis,
                &value.generics,
            ),
            Item::Enum(value) => insert_type_anchor(
                anchors,
                file,
                module_label,
                "enum",
                &value.ident.to_string(),
                value.span(),
                value,
                &value.vis,
                &value.generics,
            ),
            Item::Trait(value) => insert_type_anchor(
                anchors,
                file,
                module_label,
                "trait",
                &value.ident.to_string(),
                value.span(),
                value,
                &value.vis,
                &value.generics,
            ),
            Item::Type(value) => insert_type_anchor(
                anchors,
                file,
                module_label,
                "type",
                &value.ident.to_string(),
                value.span(),
                value,
                &value.vis,
                &value.generics,
            ),
            Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    collect_item_anchors(
                        nested,
                        file,
                        &format!("{module_label}::{}", module.ident),
                        anchors,
                    );
                }
            }
            _ => {}
        }
    }
}

fn attribute_tokens(attributes: &[Attribute]) -> String {
    attributes
        .iter()
        .map(|attribute| attribute.to_token_stream().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(clippy::too_many_arguments)]
fn insert_type_anchor(
    anchors: &mut BTreeMap<String, RustSymbolAnchorV1>,
    file: &ParsedFile,
    module_label: &str,
    kind: &str,
    name: &str,
    span: Span,
    body: &impl ToTokens,
    visibility: &Visibility,
    generics: &syn::Generics,
) {
    let logical_name = format!("{module_label}::{name}");
    let location = location(&file.path, span);
    let key = symbol_key(&file.path, "type", &logical_name, &location);
    insert_anchor(
        anchors,
        key,
        RustSymbolKindV1::Type,
        format!(
            "{kind} {} {name} {}",
            visibility.to_token_stream(),
            generics.to_token_stream()
        ),
        body.to_token_stream().to_string(),
    );
}

fn insert_anchor(
    anchors: &mut BTreeMap<String, RustSymbolAnchorV1>,
    key: String,
    kind: RustSymbolKindV1,
    signature: String,
    body: String,
) {
    let anchor = RustSymbolAnchorV1::new(
        kind,
        ContentHash::sha256(signature.as_bytes()),
        ContentHash::sha256(body.as_bytes()),
    )
    .expect("SHA-256 Rust anchor hashes always satisfy the core contract");
    let previous = anchors.insert(key, anchor);
    assert!(
        previous.is_none(),
        "Rust artifact keys are unique within one AST"
    );
}

#[allow(clippy::too_many_arguments)]
fn collect_items(
    items: &[Item],
    file: &mut ParsedFile,
    context: DeclarationContext,
    artifacts: &mut Vec<ArtifactDraft>,
    relations: &mut Vec<RelationDraft>,
    issues: &mut Vec<IssueDraft>,
    resolver: &mut BTreeMap<String, Vec<String>>,
) {
    for item in items {
        match item {
            Item::Fn(function) => {
                collect_function(function, file, &context, artifacts, relations, resolver)
            }
            Item::Impl(implementation) => collect_impl(
                implementation,
                file,
                &context,
                artifacts,
                relations,
                resolver,
            ),
            Item::Struct(item) => collect_type(
                "struct",
                &item.ident.to_string(),
                item.vis.clone(),
                item.span(),
                file,
                &context,
                artifacts,
                relations,
            ),
            Item::Enum(item) => collect_type(
                "enum",
                &item.ident.to_string(),
                item.vis.clone(),
                item.span(),
                file,
                &context,
                artifacts,
                relations,
            ),
            Item::Trait(item) => collect_type(
                "trait",
                &item.ident.to_string(),
                item.vis.clone(),
                item.span(),
                file,
                &context,
                artifacts,
                relations,
            ),
            Item::Type(item) => collect_type(
                "type",
                &item.ident.to_string(),
                item.vis.clone(),
                item.span(),
                file,
                &context,
                artifacts,
                relations,
            ),
            Item::Mod(module) => collect_module(
                module, file, &context, artifacts, relations, issues, resolver,
            ),
            Item::Use(item) => {
                collect_import(item.tree.clone(), file, &context, artifacts, relations)
            }
            Item::Macro(item) => collect_item_macro(item, file, &context, issues),
            _ => {}
        }
    }
}

fn collect_module(
    module: &ItemMod,
    file: &mut ParsedFile,
    context: &DeclarationContext,
    artifacts: &mut Vec<ArtifactDraft>,
    relations: &mut Vec<RelationDraft>,
    issues: &mut Vec<IssueDraft>,
    resolver: &mut BTreeMap<String, Vec<String>>,
) {
    let module_label = format!("{}::{}", context.module_label, module.ident);
    let module_key = format!("module:{}:{module_label}", file.path);
    artifacts.push(module_artifact(
        module_key.clone(),
        module_label.clone(),
        &file.path,
        file.content_hash.clone(),
    ));
    relations.push(RelationDraft {
        kind: "contains",
        source_key: context.module_key.clone(),
        target_keys: vec![module_key.clone()],
        attributes: Map::new(),
        source_path: Some(file.path.clone()),
        extraction_method: "reviewgraphen.ingest.rust_syn.v1",
    });
    if let Some((_, nested)) = &module.content {
        collect_items(
            nested,
            file,
            DeclarationContext {
                module_key,
                module_label,
                test_scope: context.test_scope || is_cfg_test(&module.attrs),
            },
            artifacts,
            relations,
            issues,
            resolver,
        );
    } else {
        issues.push(IssueDraft {
            kind: IngestionObstructionKind::RelationUnresolved,
            severity: ObstructionSeverity::Low,
            description: format!(
                "out-of-line module `{}` was retained as containment but its file/module correspondence was not resolved",
                module.ident
            ),
            source_keys: BTreeSet::from([module_key]),
            paths: BTreeSet::from([file.path.clone()]),
            related_capabilities: BTreeSet::new(),
        });
    }
}

fn collect_function(
    function: &ItemFn,
    file: &mut ParsedFile,
    context: &DeclarationContext,
    artifacts: &mut Vec<ArtifactDraft>,
    relations: &mut Vec<RelationDraft>,
    resolver: &mut BTreeMap<String, Vec<String>>,
) {
    let logical_name = format!("{}::{}", context.module_label, function.sig.ident);
    let location = location(&file.path, function.span());
    let function_key = symbol_key(&file.path, "function", &logical_name, &location);
    let is_test = context.test_scope || is_test(&function.attrs);
    let markers = ConcurrencyMarkers::scan(&function.sig, &function.block);
    let mut attributes = function_attributes(&function.vis, &markers);
    attributes.insert("test_function".to_owned(), Value::Bool(is_test));
    artifacts.push(ArtifactDraft {
        key: function_key.clone(),
        id_kind: "function",
        kind: "function",
        label: logical_name.clone(),
        language: Some("rust"),
        location: Some(location.clone()),
        content_hash: Some(file.content_hash.clone()),
        attributes,
        source_path: Some(file.path.clone()),
        extraction_method: "reviewgraphen.ingest.rust_syn.v1",
    });
    relations.push(containment(&context.module_key, &function_key, &file.path));
    // Grounded on the function symbol itself, never the separate `test:`
    // record a `#[test]` function also mints below: an `await` point is a
    // property of the declared function's own body.
    relations.extend(markers.await_relations(&function_key, &file.path));
    // Scoped to the exact declaring module (see `same_module_call_key`): an
    // unqualified 1-segment call only ever resolves against this key, never
    // the bare short name, which used to match by crate-wide short-name
    // uniqueness regardless of which module declared it.
    resolver
        .entry(same_module_call_key(
            &context.module_label,
            &function.sig.ident.to_string(),
        ))
        .or_default()
        .push(function_key.clone());
    resolver
        .entry(logical_name)
        .or_default()
        .push(function_key.clone());
    let source_key = if is_test {
        let test_key = symbol_key(&file.path, "test", &function_key, &location);
        artifacts.push(ArtifactDraft {
            key: test_key.clone(),
            id_kind: "test",
            kind: "test",
            label: function.sig.ident.to_string(),
            language: Some("rust"),
            location: Some(location.clone()),
            content_hash: Some(file.content_hash.clone()),
            attributes: Map::from_iter([(
                "framework".to_owned(),
                Value::String("rust".to_owned()),
            )]),
            source_path: Some(file.path.clone()),
            extraction_method: "reviewgraphen.ingest.rust_syn.v1",
        });
        relations.push(containment(&context.module_key, &test_key, &file.path));
        test_key
    } else {
        function_key
    };
    file.function_sources.insert(
        SpanKey::from(function.span()),
        FunctionSource {
            source_key,
            module_label: context.module_label.clone(),
        },
    );
}

fn collect_impl(
    implementation: &ItemImpl,
    file: &mut ParsedFile,
    context: &DeclarationContext,
    artifacts: &mut Vec<ArtifactDraft>,
    relations: &mut Vec<RelationDraft>,
    resolver: &mut BTreeMap<String, Vec<String>>,
) {
    let owner = impl_owner(&implementation.self_ty);
    for item in &implementation.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };
        let logical_name = format!("{}::{}::{owner}", context.module_label, method.sig.ident);
        let location = location(&file.path, method.span());
        let key = symbol_key(&file.path, "method", &logical_name, &location);
        let markers = ConcurrencyMarkers::scan(&method.sig, &method.block);
        artifacts.push(ArtifactDraft {
            key: key.clone(),
            id_kind: "method",
            kind: "method",
            label: logical_name.clone(),
            language: Some("rust"),
            location: Some(location.clone()),
            content_hash: Some(file.content_hash.clone()),
            attributes: function_attributes(&method.vis, &markers),
            source_path: Some(file.path.clone()),
            extraction_method: "reviewgraphen.ingest.rust_syn.v1",
        });
        relations.push(containment(&context.module_key, &key, &file.path));
        relations.extend(markers.await_relations(&key, &file.path));
        // Deliberately never registered under `same_module_call_key`
        // (unlike a free function in `collect_function`): an `impl` method
        // is never callable via bare `name()` call syntax in real Rust --
        // only `self.name()`/method-call syntax (always unresolved; see
        // `visit_expr_method_call`) or a type-qualified path (`Type::name`
        // or UFCS, which needs type proof this crate does not do; see
        // `ufcs_style_call_is_not_matched_to_a_same_named_method_without_type_proof`)
        // reach it. Registering it here would let an unrelated bare call in
        // the same module -- an imported free function, or a genuinely
        // unresolvable name -- wrongly bind to a same-named method just
        // because it is the only same-module declaration sharing that
        // identifier.
        resolver.entry(logical_name).or_default().push(key.clone());
        file.function_sources.insert(
            SpanKey::from(method.span()),
            FunctionSource {
                source_key: key,
                module_label: context.module_label.clone(),
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_type(
    type_kind: &str,
    name: &str,
    visibility: Visibility,
    span: Span,
    file: &ParsedFile,
    context: &DeclarationContext,
    artifacts: &mut Vec<ArtifactDraft>,
    relations: &mut Vec<RelationDraft>,
) {
    let label = format!("{}::{name}", context.module_label);
    let location = location(&file.path, span);
    let key = symbol_key(&file.path, "type", &label, &location);
    let mut attributes =
        Map::from_iter([("rust_kind".to_owned(), Value::String(type_kind.to_owned()))]);
    attributes.insert(
        "public".to_owned(),
        Value::Bool(matches!(visibility, Visibility::Public(_))),
    );
    artifacts.push(ArtifactDraft {
        key: key.clone(),
        id_kind: "type",
        kind: "type",
        label,
        language: Some("rust"),
        location: Some(location),
        content_hash: Some(file.content_hash.clone()),
        attributes,
        source_path: Some(file.path.clone()),
        extraction_method: "reviewgraphen.ingest.rust_syn.v1",
    });
    relations.push(containment(&context.module_key, &key, &file.path));
}

fn collect_import(
    tree: UseTree,
    file: &ParsedFile,
    context: &DeclarationContext,
    artifacts: &mut Vec<ArtifactDraft>,
    relations: &mut Vec<RelationDraft>,
) {
    for path in use_paths(&tree, String::new()) {
        let key = format!("import-module:{}:{path}", context.module_key);
        artifacts.push(ArtifactDraft {
            key: key.clone(),
            id_kind: "module",
            kind: "module",
            label: path.clone(),
            language: Some("rust"),
            location: None,
            content_hash: None,
            attributes: Map::from_iter([("syntactic_reference".to_owned(), Value::Bool(true))]),
            source_path: Some(file.path.clone()),
            extraction_method: "reviewgraphen.ingest.rust_syn.v1",
        });
        relations.push(RelationDraft {
            kind: "imports",
            source_key: context.module_key.clone(),
            target_keys: vec![key],
            attributes: Map::from_iter([(
                "resolution".to_owned(),
                Value::String("syntactic_only".to_owned()),
            )]),
            source_path: Some(file.path.clone()),
            extraction_method: "reviewgraphen.ingest.rust_syn.v1",
        });
    }
}

fn collect_item_macro(
    item: &ItemMacro,
    file: &ParsedFile,
    context: &DeclarationContext,
    issues: &mut Vec<IssueDraft>,
) {
    let name = item
        .mac
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_else(|| "unknown_macro".to_owned());
    issues.push(IssueDraft {
        kind: IngestionObstructionKind::MacroExpansionUnresolved,
        severity: ObstructionSeverity::Medium,
        description: format!(
            "macro invocation `{name}!` was retained as an unresolved expansion at line {}",
            item.span().start().line
        ),
        source_keys: BTreeSet::from([context.module_key.clone()]),
        paths: BTreeSet::from([file.path.clone()]),
        related_capabilities: BTreeSet::new(),
    });
}

fn module_artifact(
    key: String,
    label: String,
    path: &str,
    content_hash: ContentHash,
) -> ArtifactDraft {
    ArtifactDraft {
        key,
        id_kind: "module",
        kind: "module",
        label,
        language: Some("rust"),
        location: Some(LocationDraft {
            path: path.to_owned(),
            start_line: 1,
            end_line: 1,
            start_column: 1,
            end_column: 1,
        }),
        content_hash: Some(content_hash),
        attributes: Map::new(),
        source_path: Some(path.to_owned()),
        extraction_method: "reviewgraphen.ingest.rust_syn.v1",
    }
}

fn containment(source_key: &str, target_key: &str, path: &str) -> RelationDraft {
    RelationDraft {
        kind: "contains",
        source_key: source_key.to_owned(),
        target_keys: vec![target_key.to_owned()],
        attributes: Map::new(),
        source_path: Some(path.to_owned()),
        extraction_method: "reviewgraphen.ingest.rust_syn.v1",
    }
}

fn function_attributes(
    visibility: &Visibility,
    markers: &ConcurrencyMarkers,
) -> Map<String, Value> {
    let mut attributes = Map::from_iter([(
        "public".to_owned(),
        Value::Bool(matches!(visibility, Visibility::Public(_))),
    )]);
    markers.write_attributes(&mut attributes);
    attributes
}

/// Shared-state and synchronization type names this adapter recognizes by
/// *syntactic name occurrence only*. A match records that the identifier
/// literally appears in the scanned signature or body -- never that it
/// refers to `std::sync::Mutex` (or `tokio::sync::Mutex`, or any other
/// specific item): proving that would need the cross-crate/type resolution
/// this adapter explicitly does not do (see the module's bounded
/// `direct_calls`/`imports` note). The names are the ones whose whole
/// purpose in any crate that defines them is cross-task/cross-thread
/// sharing, so a same-named local type is still a shared-state-shaped
/// occurrence worth showing a reviewer, not a silent mis-resolution.
const SHARED_STATE_TYPE_NAMES: &[&str] = &[
    "Arc",
    "Barrier",
    "Condvar",
    "LazyLock",
    "Mutex",
    "Notify",
    "OnceLock",
    "RwLock",
    "Semaphore",
];

/// Final call-path segments this adapter reads as spawning concurrent work.
/// Matched only on an `Expr::Call` with a path callee, never on method-call
/// syntax: `.spawn(..)` on an arbitrary receiver is exactly as unresolved
/// here as any other method call (see `visit_expr_method_call`), and
/// `std::process::Command::spawn` shares the name without sharing the
/// meaning.
const SPAWN_CALL_NAMES: &[&str] = &["spawn", "spawn_blocking", "spawn_local"];

/// Final call-path segments this adapter reads as constructing a channel.
/// Matched the same path-call-only way as [`SPAWN_CALL_NAMES`].
const CHANNEL_CALL_NAMES: &[&str] = &["channel", "sync_channel", "unbounded_channel"];

/// The local-syntactic concurrency surface of exactly one function or
/// method, read straight off the unexpanded `syn` tree the `ast` capability
/// already covers.
///
/// This is what the `concurrency_model` capability claims, and all it
/// claims: for every function/method this adapter accepted, whether it is
/// *declared* `async`, at which lines its body syntactically `.await`s,
/// which spawn-shaped and channel-constructing call paths it names, and
/// which shared-state/synchronization type names occur in its signature or
/// body. Every one of those is decided by the syntax tree alone, with no
/// resolution step, so -- unlike `direct_calls`/`imports`/
/// `module_dependencies`/`test_mapping`/`state_writes`, which are
/// permanently `partial` because they *do* claim which item a name refers
/// to -- there is no unresolved instance of this fact kind to report.
///
/// It is deliberately not a semantic concurrency model. It does not claim
/// which runtime a `spawn` belongs to, that a named `Mutex` is any
/// particular crate's, that two invocations can actually interleave, or
/// that an `.await` inside a nested `async` block belongs to the enclosing
/// function's own suspension points rather than the spawned future's. Like
/// every other fact read off this tree -- `ast` and `containment`
/// included -- it is bounded to the *unexpanded* source: a construct a
/// macro would have generated is not visible here, exactly as it is not
/// visible to `ast`, and is reported through the same
/// `macro_expansion_unresolved` obstruction rather than by silently
/// weakening a syntax-scoped capability that is complete with respect to
/// the tree the adapter actually has.
#[derive(Default)]
struct ConcurrencyMarkers {
    declared_async: bool,
    await_lines: BTreeSet<u64>,
    spawn_calls: BTreeSet<String>,
    primitives: BTreeSet<String>,
}

impl ConcurrencyMarkers {
    fn scan(signature: &Signature, block: &Block) -> Self {
        let mut scan = ConcurrencyScan::default();
        // The signature is scanned as well as the body: an `async fn
        // handler(State(state): State<Arc<Mutex<..>>>)` names its shared
        // state only there.
        scan.visit_signature(signature);
        scan.visit_block(block);
        let mut markers = scan.markers;
        markers.declared_async = signature.asyncness.is_some();
        markers
    }

    fn write_attributes(&self, attributes: &mut Map<String, Value>) {
        // Always written, for every accepted function and method, so an
        // absent marker is a recorded negative fact rather than an omission
        // a consumer has to guess about.
        attributes.insert("async".to_owned(), Value::Bool(self.declared_async));
        attributes.insert(
            "awaits".to_owned(),
            Value::Bool(!self.await_lines.is_empty()),
        );
        attributes.insert(
            "spawns".to_owned(),
            Value::Bool(!self.spawn_calls.is_empty()),
        );
        attributes.insert(
            "concurrency_primitives".to_owned(),
            Value::Array(
                self.primitives
                    .iter()
                    .chain(self.spawn_calls.iter())
                    .cloned()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }

    /// One `awaits` relation per syntactic `.await` point, as a self-edge on
    /// the awaiting symbol -- the same shape the reference ProgramSpace
    /// fixture uses (`examples/double-submit-payment/program-space.json`),
    /// and the relation kind `node.changed_public_symbol@1`'s own contract
    /// already names in its context cover.
    fn await_relations(&self, source_key: &str, path: &str) -> Vec<RelationDraft> {
        self.await_lines
            .iter()
            .map(|line| RelationDraft {
                kind: "awaits",
                source_key: source_key.to_owned(),
                target_keys: vec![source_key.to_owned()],
                attributes: Map::from_iter([("line".to_owned(), Value::Number((*line).into()))]),
                source_path: Some(path.to_owned()),
                extraction_method: "reviewgraphen.ingest.rust_syn.v1",
            })
            .collect()
    }
}

/// Walks exactly one function/method's signature and body collecting
/// [`ConcurrencyMarkers`]. Nested items are skipped rather than folded into
/// the enclosing symbol: a nested `fn`/`impl` in a block is its own
/// declaration, and `syn`'s default traversal would otherwise attribute its
/// `await`s and primitives to whatever function happens to enclose it.
/// Macro token streams are never descended into (`syn` does not parse
/// them), so an unexpanded macro leaves a `macro_expansion_unresolved`
/// obstruction and no marker, exactly as it does for `ast`.
#[derive(Default)]
struct ConcurrencyScan {
    markers: ConcurrencyMarkers,
}

impl ConcurrencyScan {
    fn record_call_path(&mut self, call: &ExprCall) {
        let Expr::Path(path) = call.func.as_ref() else {
            return;
        };
        let segments = path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        let Some(last) = segments.last() else {
            return;
        };
        let full = segments.join("::");
        if SPAWN_CALL_NAMES.contains(&last.as_str()) {
            self.markers.spawn_calls.insert(full);
        } else if CHANNEL_CALL_NAMES.contains(&last.as_str()) {
            self.markers.primitives.insert(full);
        }
    }
}

impl Visit<'_> for ConcurrencyScan {
    fn visit_ident(&mut self, ident: &proc_macro2::Ident) {
        let name = ident.to_string();
        if SHARED_STATE_TYPE_NAMES.contains(&name.as_str())
            || (name.starts_with("Atomic") && name.len() > "Atomic".len())
        {
            self.markers.primitives.insert(name);
        }
    }

    fn visit_expr_await(&mut self, expression: &ExprAwait) {
        self.markers.await_lines.insert(
            u64::try_from(expression.await_token.span.start().line).expect("usize fits u64"),
        );
        visit::visit_expr_await(self, expression);
    }

    fn visit_expr_call(&mut self, call: &ExprCall) {
        self.record_call_path(call);
        visit::visit_expr_call(self, call);
    }

    fn visit_item_fn(&mut self, _: &ItemFn) {}

    fn visit_impl_item_fn(&mut self, _: &syn::ImplItemFn) {}
}

fn symbol_key(path: &str, kind: &str, label: &str, location: &LocationDraft) -> String {
    format!(
        "{kind}:{path}:{}:{}:{label}",
        location.start_line, location.start_column
    )
}

fn location(path: &str, span: Span) -> LocationDraft {
    let start = span.start();
    let end = span.end();
    LocationDraft {
        path: path.to_owned(),
        start_line: u64::try_from(start.line.max(1)).expect("usize fits u64"),
        end_line: u64::try_from(end.line.max(start.line).max(1)).expect("usize fits u64"),
        start_column: u64::try_from(start.column + 1).expect("usize fits u64"),
        end_column: u64::try_from(end.column + 1).expect("usize fits u64"),
    }
}

fn module_label(path: &str) -> String {
    let trimmed = path.trim_end_matches(".rs");
    let segments = trimmed.split('/').collect::<Vec<_>>();
    let module_segments = match segments.as_slice() {
        ["src", "lib"] | ["src", "main"] => Vec::new(),
        ["src", rest @ ..] => rest
            .iter()
            .filter(|segment| **segment != "mod")
            .map(|segment| (*segment).to_owned())
            .collect(),
        ["tests", rest @ ..] => std::iter::once("tests".to_owned())
            .chain(rest.iter().map(|segment| (*segment).to_owned()))
            .collect(),
        _ => segments
            .iter()
            .map(|segment| (*segment).to_owned())
            .collect(),
    };
    if module_segments.is_empty() {
        "crate".to_owned()
    } else {
        format!("crate::{}", module_segments.join("::"))
    }
}

fn is_test(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute
            .path()
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "test")
    })
}

fn is_cfg_test(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && attribute
                .meta
                .require_list()
                .is_ok_and(|list| list.tokens.to_string().contains("test"))
    })
}

fn impl_owner(value: &Type) -> String {
    match value {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_else(|| "impl".to_owned()),
        _ => "impl".to_owned(),
    }
}

fn use_paths(tree: &UseTree, prefix: String) -> Vec<String> {
    match tree {
        UseTree::Path(path) => {
            let prefix = join_use_path(&prefix, &path.ident.to_string());
            use_paths(&path.tree, prefix)
        }
        UseTree::Name(name) => vec![join_use_path(&prefix, &name.ident.to_string())],
        UseTree::Rename(rename) => vec![join_use_path(&prefix, &rename.ident.to_string())],
        UseTree::Glob(_) => vec![format!("{prefix}::*")],
        UseTree::Group(group) => group
            .items
            .iter()
            .flat_map(|item| use_paths(item, prefix.clone()))
            .collect(),
    }
}

fn join_use_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_owned()
    } else {
        format!("{prefix}::{segment}")
    }
}

/// Resolver key for an unqualified (1-segment) call/method name declared
/// directly in `module_label`. Distinct from both the bare short-name key
/// (crate-wide, never used for call resolution -- see `record_call`) and
/// the full logical-name key (`module::name`, `::`-joined): the `#`
/// separator cannot collide with either, since neither a module label nor
/// a Rust identifier ever contains `#`.
fn same_module_call_key(module_label: &str, name: &str) -> String {
    format!("{module_label}#{name}")
}

/// Recursively collects every identifier a pattern binds -- a function or
/// closure parameter, a `let`/`let-else` target, a match arm, a `for` loop
/// variable, or an `if let`/`while let` target -- so `FunctionBodyVisitor`
/// can conservatively track local shadowing. `has_unknown` is set when the
/// pattern contains a `Pat::Macro` or `Pat::Verbatim` anywhere in its tree:
/// both are opaque (an unexpanded pattern macro, or tokens `syn` could not
/// parse into a known pattern shape) and may bind any name at all, so the
/// caller must treat the whole enclosing scope as conservatively unresolved
/// rather than trust the (necessarily incomplete) `bindings` collected
/// alongside it -- the same "can't prove it's safe" posture already used for
/// a glob `use` (see `in_conservative_unresolved_scope`).
///
/// `syn::Pat` is `#[non_exhaustive]`, so a future `syn` release can add a
/// variant this match has never seen; the catch-all arm below therefore
/// fails *closed*, not open. Every currently known no-binding shape
/// (`Pat::Const`, `Pat::Lit`, `Pat::Path`, `Pat::Range`, `Pat::Rest`,
/// `Pat::Wild`) is matched explicitly and contributes neither a binding nor
/// the conservative flag; the wildcard arm is reached only by a variant this
/// function has no explicit case for, which -- being unrecognized -- might
/// bind any name, so it is treated exactly like `Pat::Macro`/`Pat::Verbatim`
/// (`has_unknown = true`) rather than assumed safe the way the old blanket
/// `_ => {}` treated it. (Stable Rust's `non_exhaustive_omitted_patterns`
/// lint, which would flag this match at compile time when `syn` adds a
/// variant, is nightly-only -- `#[deny(non_exhaustive_omitted_patterns)]`
/// only ever produces an inert `unknown_lints` warning on a stable
/// toolchain -- so this fail-closed wildcard is the actual enforcement.)
///
/// Every opaque pattern this function encounters (`Pat::Macro`,
/// `Pat::Verbatim`, or an unrecognized future variant) is also recorded
/// exactly once as a typed, function-source-grounded obstruction --
/// `macro_expansion_unresolved` for `Pat::Macro`, `unknown` for the other
/// two -- via `FunctionBodyVisitor::unresolved`. This is required because no
/// caller of `pattern_bindings` ever runs `syn::visit::Visit`'s default
/// pattern traversal (each overrides its own `visit_local`/`visit_arm`/
/// `visit_expr_closure`/`visit_expr_for_loop` to control scope timing
/// instead of delegating to `visit::visit_*`), so a pattern-position macro
/// or unparsed pattern with no other call in its scope would otherwise leave
/// no trace at all -- `has_unknown` alone only ever reaches the *shadowing*
/// logic, never the obstruction ledger.
fn pattern_bindings(
    visitor: &mut FunctionBodyVisitor<'_>,
    pat: &Pat,
    bindings: &mut Vec<String>,
    has_unknown: &mut bool,
) {
    match pat {
        Pat::Ident(pat_ident) => {
            bindings.push(pat_ident.ident.to_string());
            if let Some((_, subpat)) = &pat_ident.subpat {
                pattern_bindings(visitor, subpat, bindings, has_unknown);
            }
        }
        Pat::Type(pat_type) => pattern_bindings(visitor, &pat_type.pat, bindings, has_unknown),
        Pat::Reference(pat_reference) => {
            pattern_bindings(visitor, &pat_reference.pat, bindings, has_unknown)
        }
        Pat::Paren(pat_paren) => pattern_bindings(visitor, &pat_paren.pat, bindings, has_unknown),
        Pat::Tuple(pat_tuple) => {
            for element in &pat_tuple.elems {
                pattern_bindings(visitor, element, bindings, has_unknown);
            }
        }
        Pat::TupleStruct(pat_tuple_struct) => {
            for element in &pat_tuple_struct.elems {
                pattern_bindings(visitor, element, bindings, has_unknown);
            }
        }
        Pat::Slice(pat_slice) => {
            for element in &pat_slice.elems {
                pattern_bindings(visitor, element, bindings, has_unknown);
            }
        }
        Pat::Struct(pat_struct) => {
            for field in &pat_struct.fields {
                pattern_bindings(visitor, &field.pat, bindings, has_unknown);
            }
        }
        Pat::Or(pat_or) => {
            for case in &pat_or.cases {
                pattern_bindings(visitor, case, bindings, has_unknown);
            }
        }
        Pat::Const(_)
        | Pat::Lit(_)
        | Pat::Path(_)
        | Pat::Range(_)
        | Pat::Rest(_)
        | Pat::Wild(_) => {}
        Pat::Macro(pat_macro) => {
            *has_unknown = true;
            let name = pat_macro
                .mac
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_else(|| "unknown_macro".to_owned());
            visitor.unresolved(
                IngestionObstructionKind::MacroExpansionUnresolved,
                format!(
                    "pattern-position macro invocation `{name}!` was not expanded at line {}",
                    pat.span().start().line
                ),
                pat.span(),
            );
        }
        Pat::Verbatim(_) => {
            *has_unknown = true;
            visitor.unresolved(
                IngestionObstructionKind::Unknown,
                format!(
                    "pattern at line {} could not be parsed into a known pattern shape",
                    pat.span().start().line
                ),
                pat.span(),
            );
        }
        _ => {
            *has_unknown = true;
            visitor.unresolved(
                IngestionObstructionKind::Unknown,
                format!(
                    "pattern at line {} used a pattern shape this adapter does not recognize",
                    pat.span().start().line
                ),
                pat.span(),
            );
        }
    }
}

/// The names a block-local `use` item shadows in its enclosing block's
/// lexical scope, and whether it also contains a glob whose actual
/// introduced names cannot be enumerated. Deliberately a separate walker
/// from `use_paths` (used for the crate's *import fact* extraction, which
/// records a renamed import's original target path, not its local alias):
/// here, `UseTree::Rename`'s alias -- not the name being renamed -- is the
/// identifier that actually becomes callable/referenceable in this scope,
/// so `use ext::target as renamed_target;` shadows `renamed_target`, never
/// `target`.
fn use_item_bindings(tree: &UseTree) -> (Vec<String>, bool) {
    let mut names = Vec::new();
    let mut has_glob = false;
    collect_use_item_bindings(tree, &mut names, &mut has_glob);
    (names, has_glob)
}

fn collect_use_item_bindings(tree: &UseTree, names: &mut Vec<String>, has_glob: &mut bool) {
    match tree {
        UseTree::Path(path) => collect_use_item_bindings(&path.tree, names, has_glob),
        UseTree::Name(name) => names.push(name.ident.to_string()),
        UseTree::Rename(rename) => names.push(rename.rename.to_string()),
        UseTree::Glob(_) => *has_glob = true,
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use_item_bindings(item, names, has_glob);
            }
        }
    }
}

/// Normalizes a `>= 2`-segment call path against the calling module's
/// logical label, using only the syntactic `crate`/`self`/`super` module
/// path grammar -- never `use`-alias or type resolution. Returns `None` when
/// the path cannot be normalized this way (an unqualified-but-external,
/// aliased, or otherwise unprovable qualifier), so the caller falls back to
/// an explicit unresolved relation rather than guessing.
fn normalize_call_path(segments: &[String], module_label: Option<&str>) -> Option<String> {
    if segments.len() < 2 {
        return None;
    }
    match segments[0].as_str() {
        "crate" => Some(segments.join("::")),
        "self" => {
            let module_label = module_label?;
            let rest = &segments[1..];
            if rest.is_empty() {
                return None;
            }
            Some(format!("{module_label}::{}", rest.join("::")))
        }
        "super" => {
            let mut module_segments = module_label?
                .split("::")
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            let mut index = 0;
            while index < segments.len() && segments[index] == "super" {
                // `module_segments[0]` is always the fixed `crate` root
                // (see `module_label`); `super` above the crate root is not
                // syntactically resolvable.
                if module_segments.len() <= 1 {
                    return None;
                }
                module_segments.pop();
                index += 1;
            }
            let rest = &segments[index..];
            if rest.is_empty() {
                return None;
            }
            module_segments.extend(rest.iter().cloned());
            Some(module_segments.join("::"))
        }
        _ => None,
    }
}

/// One lexical scope active at some point in a function body being
/// visited. `bindings` is every name a parameter, `let`/`let-else` binding,
/// closure parameter, match-arm pattern, `for` loop pattern, `if let`/
/// `while let` pattern (including `&&`-chained forms), block-scoped nested
/// `fn`/`const`/`static` item, or block-local `use` item introduces.
/// `conservative_unresolved` is set when this scope also contains a glob
/// `use` (or another `use` shape this crate cannot reduce to an exact bound
/// name): such a scope can introduce *any* name, so an unqualified call
/// resolved while it is active can never be proven not to be shadowed,
/// regardless of whether the literal name happens to already be in
/// `bindings`.
#[derive(Default)]
struct LexicalScope {
    bindings: BTreeSet<String>,
    conservative_unresolved: bool,
}

struct FunctionBodyVisitor<'a> {
    path: &'a str,
    sources: &'a BTreeMap<SpanKey, FunctionSource>,
    resolver: &'a BTreeMap<String, Vec<String>>,
    current_source: Option<String>,
    current_module_label: Option<String>,
    /// Stack of lexical scopes active at the current point in the function
    /// body being visited, innermost last. Consulted before ever matching
    /// an unqualified 1-segment call against a same-module declaration (see
    /// `record_call`): a locally bound name always shadows a module-level
    /// item for an unqualified call in real Rust, whether or not that
    /// local binding is itself callable, so it must never resolve to one
    /// here; a scope reachable by a glob/unresolved `use` blocks
    /// unqualified resolution outright for the same reason.
    scopes: Vec<LexicalScope>,
    relations: Vec<RelationDraft>,
    writes: Vec<StateWrite>,
    issues: Vec<IssueDraft>,
}

impl<'a> FunctionBodyVisitor<'a> {
    fn new(
        path: &'a str,
        sources: &'a BTreeMap<SpanKey, FunctionSource>,
        resolver: &'a BTreeMap<String, Vec<String>>,
    ) -> Self {
        Self {
            path,
            sources,
            resolver,
            current_source: None,
            current_module_label: None,
            scopes: Vec::new(),
            relations: Vec::new(),
            writes: Vec::new(),
            issues: Vec::new(),
        }
    }

    fn push_scope(
        &mut self,
        names: impl IntoIterator<Item = String>,
        conservative_unresolved: bool,
    ) {
        self.scopes.push(LexicalScope {
            bindings: names.into_iter().collect(),
            conservative_unresolved,
        });
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn bind_in_current_scope(&mut self, names: impl IntoIterator<Item = String>) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.bindings.extend(names);
        }
    }

    /// Marks the current scope conservative in place, mirroring a glob `use`
    /// (see `LexicalScope::conservative_unresolved`): called after
    /// `bind_in_current_scope` when the same pattern's `pattern_bindings`
    /// call also reported `has_unknown`, since a `Pat::Macro`/`Pat::Verbatim`
    /// binding target -- unlike a glob, which is detected before any
    /// binding is pushed -- is only known once the pattern has already been
    /// walked into the current (not a freshly pushed) scope.
    fn mark_current_scope_conservative(&mut self) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.conservative_unresolved = true;
        }
    }

    fn is_locally_bound(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .any(|scope| scope.bindings.contains(name))
    }

    /// `true` when any active scope contains a glob `use` (or another
    /// `use` shape this crate cannot reduce to an exact bound name): an
    /// unqualified call resolved while this is true can never be proven
    /// not to be shadowed by whatever that `use` actually introduces.
    fn in_conservative_unresolved_scope(&self) -> bool {
        self.scopes
            .iter()
            .any(|scope| scope.conservative_unresolved)
    }

    /// Visits a (possibly `&&`-chained, parenthesized/grouped) `if`/`while`
    /// condition strictly left to right, binding each `let`'s pattern into
    /// the current scope immediately *after* its own initializer is
    /// visited -- exactly Rust's let-chain visibility rule: a later chain
    /// element (another `&&` condition, or the following `then`/loop body,
    /// pushed by the caller as the same active scope) can see an earlier
    /// let's bindings, but that let's own initializer, and anything before
    /// it in the chain, cannot. A `let` sub-expression is only ever
    /// syntactically valid directly, or `&&`-chained, in an `if`/`while`
    /// condition (see the non-chain fallback arm), so this exhausts every
    /// shape such a condition can take.
    fn visit_let_chain_condition(&mut self, condition: &Expr) {
        match condition {
            Expr::Let(let_expr) => {
                self.visit_expr(&let_expr.expr);
                let mut bindings = Vec::new();
                let mut has_unknown = false;
                pattern_bindings(self, &let_expr.pat, &mut bindings, &mut has_unknown);
                self.bind_in_current_scope(bindings);
                if has_unknown {
                    self.mark_current_scope_conservative();
                }
            }
            Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_)) => {
                self.visit_let_chain_condition(&binary.left);
                self.visit_let_chain_condition(&binary.right);
            }
            Expr::Paren(paren) => self.visit_let_chain_condition(&paren.expr),
            Expr::Group(group) => self.visit_let_chain_condition(&group.expr),
            other => self.visit_expr(other),
        }
    }

    fn with_source(&mut self, span: Span, signature: &Signature, block: &Block) {
        let previous_source = self.current_source.clone();
        let previous_module_label = self.current_module_label.clone();
        let source = self.sources.get(&SpanKey::from(span));
        self.current_source = source.map(|source| source.source_key.clone());
        self.current_module_label = source.map(|source| source.module_label.clone());
        let mut params = Vec::new();
        let mut has_unknown = false;
        for input in &signature.inputs {
            if let FnArg::Typed(pat_type) = input {
                pattern_bindings(self, &pat_type.pat, &mut params, &mut has_unknown);
            }
        }
        self.push_scope(params, has_unknown);
        // Dispatched through `self.visit_block` (not the bare
        // `visit::visit_block` walker) so the function's own top-level
        // block gets the same hoisting/conservative-glob treatment as any
        // nested block -- a block-local `use`/`const`/`static`/`fn`
        // directly in a function's own body, not one level deeper, must
        // shadow exactly the same way.
        self.visit_block(block);
        self.pop_scope();
        self.current_source = previous_source;
        self.current_module_label = previous_module_label;
    }

    fn current(&self) -> Option<String> {
        self.current_source.clone()
    }

    fn unresolved(&mut self, kind: IngestionObstructionKind, description: String, span: Span) {
        let Some(source_key) = self.current() else {
            return;
        };
        self.issues.push(IssueDraft {
            kind,
            severity: ObstructionSeverity::Medium,
            description,
            source_keys: BTreeSet::from([source_key]),
            paths: BTreeSet::from([self.path.to_owned()]),
            related_capabilities: BTreeSet::new(),
        });
        let _ = span;
    }

    fn record_call(&mut self, call: &ExprCall) {
        let Some(source_key) = self.current() else {
            return;
        };
        let Expr::Path(path) = call.func.as_ref() else {
            self.unresolved(
                IngestionObstructionKind::RelationUnresolved,
                format!(
                    "non-path function call was not resolved at line {}",
                    call.span().start().line
                ),
                call.span(),
            );
            return;
        };
        let segments = path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            return;
        }
        let full = segments.join("::");
        // A single unqualified segment resolves only against a function or
        // method declared directly in the *calling* module -- never by
        // crate-wide short-name uniqueness. Matching any same-named
        // declaration anywhere in the crate was unsound: a call to a
        // `use`-imported or builtin/prelude name (for example `drop()`
        // after `use std::mem::drop;`) would wrongly bind to an unrelated
        // local item that merely happens to share that final identifier
        // (for example a local `fn drop` in a different module). A
        // `use`-imported binding is deliberately NOT resolved either
        // (proving where a `use` path -- a rename, a glob, or a
        // `crate`/`self`/`super`-relative path -- actually terminates
        // would need the same alias/type resolution this crate does not
        // do): it remains a conservative unresolved relation rather than a
        // guess. Nor is a same-module match accepted when the call-position
        // identifier is shadowed by a local binding: `self.scopes` (see
        // `is_locally_bound`) conservatively tracks every parameter,
        // `let`/`let-else` target, closure parameter, match-arm/`for`-loop/
        // `if let`/`while let` pattern, and block-scoped nested `fn` name in
        // lexical scope, so `fn target(){} fn caller(target: impl Fn()){
        // target(); }` never resolves `target()` to the module function --
        // it is shadowed by the parameter, exactly as real Rust name
        // resolution would treat it, regardless of whether the shadowing
        // binding is itself callable. A qualified, multi-segment path
        // resolves only against the exact, normalized full logical name;
        // the previous last-segment fallback (matching `external::worker()`
        // to an unrelated local `worker`) is likewise never used. When the
        // qualifier cannot be normalized without semantic resolution --
        // including UFCS-style `Type::method(&receiver)` call syntax, which
        // requires knowing the receiver's type -- the call is retained as
        // an explicit unresolved relation instead of guessed.
        if segments.len() == 1 && self.is_locally_bound(&segments[0]) {
            self.unresolved(
                IngestionObstructionKind::RelationUnresolved,
                format!(
                    "unqualified call `{full}` is shadowed by a local binding at line {}; it \
                     is never matched to a same-named module item",
                    call.span().start().line
                ),
                call.span(),
            );
            return;
        }
        // A glob (or otherwise unenumerable) `use` in scope (see
        // `visit_block`) can introduce a binding for *any* name, including
        // one that happens to already match a same-module declaration:
        // without knowing what it actually imports, such a match can never
        // be proven safe, so it is conservatively unresolved regardless of
        // whether `segments[0]` is already a known local binding.
        if segments.len() == 1 && self.in_conservative_unresolved_scope() {
            self.unresolved(
                IngestionObstructionKind::RelationUnresolved,
                format!(
                    "unqualified call `{full}` at line {} is in a scope reachable by a glob \
                     import or another unresolved `use` shape; it cannot be proven not to be \
                     shadowed, so it is never matched to a same-named module item",
                    call.span().start().line
                ),
                call.span(),
            );
            return;
        }
        let candidates = if segments.len() == 1 {
            self.current_module_label
                .as_deref()
                .map(|module_label| same_module_call_key(module_label, &segments[0]))
                .and_then(|key| self.resolver.get(&key))
                .cloned()
                .unwrap_or_default()
        } else {
            normalize_call_path(&segments, self.current_module_label.as_deref())
                .and_then(|normalized| self.resolver.get(&normalized))
                .cloned()
                .unwrap_or_default()
        };
        if candidates.len() != 1 {
            self.unresolved(
                IngestionObstructionKind::RelationUnresolved,
                format!(
                    "direct call `{full}` has {} local syntactic targets at line {}",
                    candidates.len(),
                    call.span().start().line
                ),
                call.span(),
            );
            return;
        }
        let target = candidates
            .into_iter()
            .next()
            .expect("one candidate checked");
        self.relations.push(RelationDraft {
            kind: "calls",
            source_key: source_key.clone(),
            target_keys: vec![target.clone()],
            attributes: Map::from_iter([
                (
                    "resolution".to_owned(),
                    Value::String("syntactic_unique".to_owned()),
                ),
                (
                    "line".to_owned(),
                    Value::Number(
                        u64::try_from(call.span().start().line)
                            .expect("usize fits u64")
                            .into(),
                    ),
                ),
            ]),
            source_path: Some(self.path.to_owned()),
            extraction_method: "reviewgraphen.ingest.rust_syn.v1",
        });
        if source_key.starts_with("test:") {
            self.relations.push(RelationDraft {
                kind: "covers",
                source_key,
                target_keys: vec![target],
                attributes: Map::from_iter([
                    (
                        "mapping".to_owned(),
                        Value::String("direct_call".to_owned()),
                    ),
                    (
                        "line".to_owned(),
                        Value::Number(
                            u64::try_from(call.span().start().line)
                                .expect("usize fits u64")
                                .into(),
                        ),
                    ),
                ]),
                source_path: Some(self.path.to_owned()),
                extraction_method: "reviewgraphen.ingest.rust_syn.v1",
            });
        }
    }

    fn record_write(&mut self, expression: &Expr, span: Span) {
        let Some(source_key) = self.current() else {
            return;
        };
        let Some(label) = expression_label(expression) else {
            self.unresolved(
                IngestionObstructionKind::RelationUnresolved,
                format!(
                    "assignment target was not reduced to a selected state-write fact at line {}",
                    span.start().line
                ),
                span,
            );
            return;
        };
        self.writes.push(StateWrite {
            source_key,
            label,
            location: location(self.path, span),
        });
    }
}

impl Visit<'_> for FunctionBodyVisitor<'_> {
    fn visit_item_fn(&mut self, item: &ItemFn) {
        self.with_source(item.span(), &item.sig, &item.block);
    }

    fn visit_impl_item_fn(&mut self, item: &syn::ImplItemFn) {
        self.with_source(item.span(), &item.sig, &item.block);
    }

    fn visit_expr_call(&mut self, call: &ExprCall) {
        self.record_call(call);
        visit::visit_expr_call(self, call);
    }

    /// Every nested block introduces its own lexical scope, so a shadow
    /// introduced inside it never leaks to sibling or enclosing code once
    /// the block ends. A nested `fn`/`const`/`static` item, and every name
    /// a block-local `use` item introduces, is hoisted -- bound for the
    /// *whole* block, not just after its textual position -- matching
    /// Rust's own item-scoping rule and preventing a call positioned
    /// before such a local item from wrongly falling through to a
    /// same-named module-level function. A block-local `use` naming a
    /// module-level function (for example `use ext::target;` shadowing an
    /// unrelated same-module `fn target`) is exactly as much a shadow as a
    /// local `fn`/`let` -- and is never itself resolved further (proving
    /// where the `use` path actually terminates would need the same
    /// alias/type resolution this crate does not do), so this only ever
    /// removes an incorrect same-module match, never adds one. A glob (or
    /// otherwise unenumerable) `use` cannot be reduced to specific names at
    /// all, so it instead marks the whole block conservative (see
    /// `in_conservative_unresolved_scope`): every unqualified call in it,
    /// not only ones matching an already-known name, is treated as
    /// possibly shadowed. A block-local tuple/unit struct (`struct
    /// target();` / `struct target;`) is likewise hoisted: both put `target`
    /// in the *value* namespace as its own constructor, shadowing an
    /// outer/module `fn target` for a bare `target()`/`target` reference in
    /// this block exactly like a local `fn` would, whether or not the
    /// resulting call actually type-checks. A block-local *named-field*
    /// struct (`struct target { .. }`) is deliberately never hoisted here:
    /// real Rust puts a named-field struct's name only in the *type*
    /// namespace, never the value namespace, so it can never itself be a
    /// call target and does not shadow a same-named callable at all --
    /// hoisting it anyway would wrongly block an otherwise-valid resolution
    /// to the outer `fn target`.
    fn visit_block(&mut self, block: &Block) {
        let mut hoisted = Vec::new();
        let mut conservative_unresolved = false;
        for stmt in &block.stmts {
            match stmt {
                syn::Stmt::Item(Item::Fn(nested)) => hoisted.push(nested.sig.ident.to_string()),
                syn::Stmt::Item(Item::Const(nested)) => hoisted.push(nested.ident.to_string()),
                syn::Stmt::Item(Item::Static(nested)) => hoisted.push(nested.ident.to_string()),
                syn::Stmt::Item(Item::Struct(nested))
                    if !matches!(nested.fields, Fields::Named(_)) =>
                {
                    hoisted.push(nested.ident.to_string());
                }
                syn::Stmt::Item(Item::Use(nested)) => {
                    let (names, has_glob) = use_item_bindings(&nested.tree);
                    hoisted.extend(names);
                    conservative_unresolved |= has_glob;
                }
                // A block-local `extern` block's `fn` and `static` items put
                // their names in the *value* namespace of the enclosing block,
                // exactly as a block-local `fn`/`static` item does (see the
                // arms above), so they shadow a same-named module-level
                // function for a naked call in this block and must be hoisted
                // the same way. A foreign type alias is type-namespace-only
                // (like a named-field struct, never hoisted), so it blocks
                // nothing.
                syn::Stmt::Item(Item::ForeignMod(nested)) => {
                    for foreign_item in &nested.items {
                        match foreign_item {
                            syn::ForeignItem::Fn(decl) => {
                                hoisted.push(decl.sig.ident.to_string());
                            }
                            syn::ForeignItem::Static(decl) => {
                                hoisted.push(decl.ident.to_string());
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        self.push_scope(hoisted, conservative_unresolved);
        visit::visit_block(self, block);
        self.pop_scope();
    }

    /// A `let`/`let-else` binding's own name(s) become visible only *after*
    /// its initializer (and `let-else` diverging block) is visited, so
    /// `let target = target;` still resolves the right-hand `target` in the
    /// outer scope, matching Rust.
    fn visit_local(&mut self, local: &Local) {
        if let Some(init) = &local.init {
            self.visit_expr(&init.expr);
            if let Some((_, diverge)) = &init.diverge {
                self.visit_expr(diverge);
            }
        }
        let mut bindings = Vec::new();
        let mut has_unknown = false;
        pattern_bindings(self, &local.pat, &mut bindings, &mut has_unknown);
        self.bind_in_current_scope(bindings);
        if has_unknown {
            self.mark_current_scope_conservative();
        }
    }

    /// A closure's parameters are scoped only to its own body.
    fn visit_expr_closure(&mut self, closure: &ExprClosure) {
        let mut bindings = Vec::new();
        let mut has_unknown = false;
        for input in &closure.inputs {
            pattern_bindings(self, input, &mut bindings, &mut has_unknown);
        }
        self.push_scope(bindings, has_unknown);
        self.visit_expr(&closure.body);
        self.pop_scope();
    }

    /// A match arm's pattern is scoped to its own guard and body only.
    fn visit_arm(&mut self, arm: &Arm) {
        let mut bindings = Vec::new();
        let mut has_unknown = false;
        pattern_bindings(self, &arm.pat, &mut bindings, &mut has_unknown);
        self.push_scope(bindings, has_unknown);
        if let Some((_, guard)) = &arm.guard {
            self.visit_expr(guard);
        }
        self.visit_expr(&arm.body);
        self.pop_scope();
    }

    /// A `for` loop's pattern is scoped to its own body only.
    fn visit_expr_for_loop(&mut self, for_loop: &ExprForLoop) {
        self.visit_expr(&for_loop.expr);
        let mut bindings = Vec::new();
        let mut has_unknown = false;
        pattern_bindings(self, &for_loop.pat, &mut bindings, &mut has_unknown);
        self.push_scope(bindings, has_unknown);
        self.visit_block(&for_loop.body);
        self.pop_scope();
    }

    /// An `if let`/let-chain condition's bindings are scoped to the
    /// `then` branch only -- never the `else` branch, which is visited with
    /// them popped again.
    fn visit_expr_if(&mut self, expr: &ExprIf) {
        self.push_scope(std::iter::empty(), false);
        self.visit_let_chain_condition(&expr.cond);
        self.visit_block(&expr.then_branch);
        self.pop_scope();
        if let Some((_, else_branch)) = &expr.else_branch {
            self.visit_expr(else_branch);
        }
    }

    /// A `while let`/let-chain condition's bindings are scoped to the loop
    /// body only.
    fn visit_expr_while(&mut self, expr: &ExprWhile) {
        self.push_scope(std::iter::empty(), false);
        self.visit_let_chain_condition(&expr.cond);
        self.visit_block(&expr.body);
        self.pop_scope();
    }

    fn visit_expr_method_call(&mut self, call: &ExprMethodCall) {
        self.unresolved(
            IngestionObstructionKind::DynamicDispatchUnresolved,
            format!(
                "method call `{}` was not resolved beyond syntactic dispatch at line {}",
                call.method,
                call.span().start().line
            ),
            call.span(),
        );
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &Macro) {
        let name = mac
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_else(|| "unknown_macro".to_owned());
        self.unresolved(
            IngestionObstructionKind::MacroExpansionUnresolved,
            format!(
                "macro invocation `{name}!` was not expanded at line {}",
                mac.span().start().line
            ),
            mac.span(),
        );
        visit::visit_macro(self, mac);
    }

    fn visit_expr_assign(&mut self, assignment: &ExprAssign) {
        self.record_write(&assignment.left, assignment.span());
        visit::visit_expr_assign(self, assignment);
    }

    fn visit_expr_binary(&mut self, binary: &ExprBinary) {
        if matches!(
            binary.op,
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
        ) {
            self.record_write(&binary.left, binary.span());
        }
        visit::visit_expr_binary(self, binary);
    }
}

#[derive(Clone)]
struct StateWrite {
    source_key: String,
    label: String,
    location: LocationDraft,
}

fn expression_label(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Path(path) if path.qself.is_none() => Some(
            path.path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>()
                .join("::"),
        ),
        Expr::Field(field) => expression_label(&field.base).map(|base| match &field.member {
            syn::Member::Named(member) => format!("{base}.{member}"),
            syn::Member::Unnamed(member) => format!("{base}.{}", member.index),
        }),
        Expr::Index(index) => expression_label(&index.expr).map(|base| format!("{base}[..]")),
        _ => None,
    }
}

#[cfg(test)]
mod anchor_tests {
    use super::*;

    fn anchors(source: &str) -> Vec<RustSymbolAnchorV1> {
        let file = ParsedFile::new(
            &crate::git::SnapshotFile {
                path: "src/lib.rs".to_owned(),
                content: source.as_bytes().to_vec(),
                content_hash: ContentHash::sha256(source.as_bytes()),
                changed_lines: BTreeSet::new(),
            },
            syn::parse_file(source).expect("test source parses"),
        );
        file_symbol_anchors(&file).into_values().collect()
    }

    #[test]
    fn rust_anchor_v1_ignores_only_comments_formatting_and_spans() {
        let compact = anchors("pub fn charge(value: u64) -> u64 { value + 1 }");
        let formatted = anchors(
            "// leading comment\n\npub fn charge( value : u64 ) -> u64 {\n    value + 1 // tail\n}\n",
        );
        assert_eq!(compact, formatted);
    }

    #[test]
    fn rust_anchor_v1_changes_for_identifier_signature_or_body_mutation() {
        let baseline = anchors("pub fn charge(value: u64) -> u64 { value + 1 }");
        let renamed = anchors("pub fn debit(value: u64) -> u64 { value + 1 }");
        let signature = anchors("pub fn charge(value: u32) -> u64 { u64::from(value) + 1 }");
        let body = anchors("pub fn charge(value: u64) -> u64 { value + 2 }");
        assert_ne!(baseline, renamed);
        assert_ne!(baseline, signature);
        assert_ne!(baseline, body);
    }
}
