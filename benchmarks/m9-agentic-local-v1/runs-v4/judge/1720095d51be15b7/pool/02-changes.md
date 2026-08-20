# Proposed changes

Each change below is an independent proposal for the same specification. They are listed in content-hash order, which carries no information about their origin.

## change_id `b6adae1a98003eff`

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
@@ -1811,69 +1812,107 @@
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
-    /// to the outer `fn target`.
+    /// to the outer `fn target`. A block-local `extern` foreign module
+    /// (`extern "C" { fn target(); }`) is hoisted exactly like a block-local
+    /// `fn`: its `fn` and `static` declarations are value-namespace items, so
+    /// each one shadows a same-named module-level function for a bare call in
+    /// this block; a foreign module declaring only *other* names shadows
+    /// nothing and must not make the block conservative-unresolved.
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
+                // A block-local `extern "C" { ... }` foreign module is an
+                // item like any other: its `fn` and `static` declarations
+                // put their names in the *value* namespace of the enclosing
+                // block, exactly as a block-local `fn`/`static` does, so each
+                // one shadows a same-named module-level function for a bare
+                // call in this block (a foreign `static` and `type` each use
+                // a single namespace, so a value-namespace call can never
+                // reach a same-named foreign `type` either -- hoisting only
+                // the value-namespace names is the whole of the shadow). A
+                // foreign module declaring *other* names contributes no
+                // shadow at all, so it is never a conservative-unresolved
+                // scope: marking the whole block unresolved on the mere
+                // presence of any foreign module would wrongly block a bare
+                // call that real Rust still resolves to the module function.
+                syn::Stmt::Item(Item::ForeignMod(nested)) => {
+                    for item in &nested.items {
+                        match item {
+                            ForeignItem::Fn(external) => {
+                                hoisted.push(external.sig.ident.to_string())
+                            }
+                            ForeignItem::Static(external) => {
+                                hoisted.push(external.ident.to_string())
+                            }
+                            // A foreign `type` is in the *type* namespace
+                            // only and a foreign macro is in the *macro*
+                            // namespace only: neither is a value-namespace
+                            // binding, so neither can shadow a bare call
+                            // target, exactly as a block-level `type`/
+                            // `macro` item is never hoisted here.
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
