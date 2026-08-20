# Proposed changes

Each change below is an independent proposal for the same specification. They are listed in content-hash order, which carries no information about their origin.

## change_id `274a12fa9d3b1f4a`

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
+    Item, ItemFn, ItemImpl, ItemMacro, ItemMod, Local, Macro, Pat, Signature, Type, UseTree,
+    Visibility,
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
@@ -1830,50 +1831,77 @@
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
+                syn::Stmt::Item(Item::ForeignMod(nested)) => {
+                    // A block-local foreign module (`extern "C" { .. }`) hoists its
+                    // `fn` and `static` declarations into the enclosing block's
+                    // *value* namespace, exactly like a block-local `fn`/`static`
+                    // item, so each such name shadows a same-named module-level
+                    // function for a naked call in this block. Only the value
+                    // namespace matters here: a foreign `type` sits in the type
+                    // namespace (never a call target) and an unexpanded macro is
+                    // opaque but never a `fn`/`static` by name, so neither
+                    // shadows a same-named callable. The shadow is scoped to this
+                    // block alone (pushed as a fresh `LexicalScope`, popped when
+                    // the block ends) and never marks the whole block
+                    // conservatively unresolved: a foreign module that declares
+                    // *other* names only must leave a same-named `target()`
+                    // resolvable to the module-level `target`.
+                    for foreign_item in &nested.items {
+                        match foreign_item {
+                            ForeignItem::Fn(foreign_fn) => {
+                                hoisted.push(foreign_fn.sig.ident.to_string());
+                            }
+                            ForeignItem::Static(foreign_static) => {
+                                hoisted.push(foreign_static.ident.to_string());
+                            }
+                            _ => {}
+                        }
+                    }
+                }
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
```
