# Proposed changes

Each change below is an independent proposal for the same specification. They are listed in content-hash order, which carries no information about their origin.

## change_id `ba6d5921d6ba35c7`

Unified diff against the current `crates/reviewgraphen-ingest/src/rust.rs`, with 25 lines of context on each side:

```diff
@@ -1,41 +1,42 @@
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
-    ExprForLoop, ExprIf, ExprMethodCall, ExprWhile, Fields, File, FnArg, ImplItem, Item, ItemFn,
-    ItemImpl, ItemMacro, ItemMod, Local, Macro, Pat, Signature, Type, UseTree, Visibility,
+    ExprForLoop, ExprIf, ExprMethodCall, ExprWhile, Fields, File, FnArg, ForeignItem, ImplItem,
+    Item, ItemFn, ItemForeignMod, ItemImpl, ItemMacro, ItemMod, Local, Macro, Pat, Signature, Type,
+    UseTree, Visibility,
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
```
