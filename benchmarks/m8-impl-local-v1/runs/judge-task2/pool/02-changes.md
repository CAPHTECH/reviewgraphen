# Proposed changes

Each change below is an independent proposal for the same specification. They are listed in content-hash order, which carries no information about their origin.

## change_id `8b6a4b6e707df101`

Unified diff against the current `crates/reviewgraphen-ingest/src/rust.rs`, with 25 lines of context on each side:

```diff
@@ -1784,96 +1784,106 @@
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
-    /// the block ends. A nested `fn`/`const`/`static` item, and every name
-    /// a block-local `use` item introduces, is hoisted -- bound for the
+    /// the block ends. A nested `fn`/`const`/`static` item, every name a
+    /// block-local foreign module's `fn`/`static` declaration introduces,
+    /// and every name a block-local `use` item introduces, is hoisted -- bound for the
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
+                syn::Stmt::Item(Item::ForeignMod(nested)) => {
+                    for declaration in &nested.items {
+                        match declaration {
+                            syn::ForeignItem::Fn(decl) => hoisted.push(decl.sig.ident.to_string()),
+                            syn::ForeignItem::Static(decl) => hoisted.push(decl.ident.to_string()),
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

## change_id `fe6e01320b1f9c84`

Unified diff against the current `crates/reviewgraphen-ingest/src/rust.rs`, with 25 lines of context on each side:

```diff
@@ -1830,50 +1830,71 @@
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
+                // A block-local `extern` block's `fn` and `static` items put
+                // their names in the *value* namespace of the enclosing block,
+                // exactly as a block-local `fn`/`static` item does (see the
+                // arms above), so they shadow a same-named module-level
+                // function for a naked call in this block and must be hoisted
+                // the same way. A foreign type alias is type-namespace-only
+                // (like a named-field struct, never hoisted), so it blocks
+                // nothing.
+                syn::Stmt::Item(Item::ForeignMod(nested)) => {
+                    for foreign_item in &nested.items {
+                        match foreign_item {
+                            syn::ForeignItem::Fn(decl) => {
+                                hoisted.push(decl.sig.ident.to_string());
+                            }
+                            syn::ForeignItem::Static(decl) => {
+                                hoisted.push(decl.ident.to_string());
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
