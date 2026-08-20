# Proposed changes

Each change below is an independent proposal for the same specification. They are listed in content-hash order, which carries no information about their origin.

## change_id `cb18f48cfffcbc02`

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
+                // A block-local `extern "C" { .. }` puts its `fn` and `static`
+                // names in the *value* namespace of the enclosing block, exactly
+                // like a block-local `fn`/`static` item (Rust hoists them for the
+                // whole block), so a same-named naked call in this block must be
+                // treated as shadowed, never matched to a same-named module item.
+                // Only the value-namespace members bind a name for a naked call:
+                // a foreign `type` is type-namespace-only and a foreign
+                // `macro`/verbatim is not a value binding, so -- mirroring the
+                // named-struct exclusion above -- they deliberately contribute no
+                // shadow and cannot wrongly block an otherwise-valid resolution.
+                // The name is scoped to this block by the surrounding
+                // push/pop_scope, so it never leaks past it (see `visit_block`).
+                syn::Stmt::Item(Item::ForeignMod(nested)) => {
+                    for foreign in &nested.items {
+                        match foreign {
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
