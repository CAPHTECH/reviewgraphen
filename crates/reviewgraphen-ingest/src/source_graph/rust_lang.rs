//! Rust extraction for source review v6 (`syn`, unexpanded source).

use super::{
    CallForm, Callable, CallableKind, Extracted, FileFacts, INITIALIZER_NAME, Import, RawCall,
    normalize_ws,
};
use quote::ToTokens;
use std::collections::BTreeSet;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Expr, ImplItem, Item, TraitItem, UseTree, Visibility};

/// Module path of a file, rooted at its crate. Crate roots follow Cargo's
/// target layout: `<dir>/src/lib.rs` is the library (`crate@<dir>`, or
/// `crate` at the repository root); `src/main.rs` is its own binary crate
/// when a `src/lib.rs` exists beside it; `src/bin/<name>(.rs|/main.rs)` is
/// a binary crate; any file outside a `src/` directory (integration tests,
/// examples, build scripts) roots its own crate.
pub(crate) fn module_of(path: &str, has_lib: impl Fn(&str) -> bool) -> String {
    let stem = path.strip_suffix(".rs").unwrap_or(path);
    let (crate_dir, rel) = match stem.rfind("src/") {
        Some(i) if i == 0 || stem.as_bytes()[i - 1] == b'/' => (&stem[..i], &stem[i + 4..]),
        _ => return format!("crate@{stem}"),
    };
    let lib_root = if crate_dir.is_empty() {
        String::from("crate")
    } else {
        format!("crate@{}", crate_dir.trim_end_matches('/'))
    };
    let mut segs: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
    let mut root = lib_root.clone();
    if segs == ["main"] && has_lib(&format!("{crate_dir}src/lib.rs")) {
        root = format!("{lib_root}#main");
        segs.clear();
    } else if segs.first() == Some(&"bin") && segs.len() >= 2 {
        root = format!("{lib_root}#bin/{}", segs[1]);
        segs.drain(..2);
        if segs.first() == Some(&"main") && segs.len() == 1 {
            segs.clear();
        }
    }
    if segs.len() == 1 && matches!(segs[0], "lib" | "main") {
        segs.clear();
    }
    if matches!(segs.last(), Some(&"mod")) {
        segs.pop();
    }
    let mut out = root;
    for s in segs {
        out.push_str("::");
        out.push_str(s);
    }
    out
}

/// Owner name of an impl's self type: the last path segment, or the
/// normalized tokens of any other type (`[u8]`, `(T, T)`, `dyn Tr`).
fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => type_name(&r.elem),
        syn::Type::Group(g) => type_name(&g.elem),
        syn::Type::Paren(p) => type_name(&p.elem),
        other => Some(normalize_ws(&other.to_token_stream().to_string())),
    }
}

fn is_pub(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

fn line_range(span: proc_macro2::Span) -> (u32, u32) {
    (span.start().line as u32, span.end().line as u32)
}

struct Extractor {
    facts: FileFacts,
    scope: Vec<String>,
    initializer: Option<usize>,
}

impl Extractor {
    fn scope(&self) -> String {
        self.scope.join("::")
    }

    fn push(&mut self, callable: Callable) -> usize {
        self.facts.callables.push(callable);
        self.facts.locals.push(BTreeSet::new());
        self.facts.callables.len() - 1
    }

    #[allow(clippy::too_many_arguments)]
    fn push_fn(
        &mut self,
        name: String,
        owner: Option<String>,
        kind: CallableKind,
        vis_public: bool,
        span: proc_macro2::Span,
        sig: &syn::Signature,
        vis: &Visibility,
    ) -> usize {
        let (start_line, end_line) = line_range(span);
        let head = format!("{} {}", vis.to_token_stream(), sig.to_token_stream());
        let id = self.push(Callable {
            name,
            owner,
            kind,
            public: vis_public,
            start_line,
            end_line,
            signature: normalize_ws(&head),
            params: normalize_ws(&sig.inputs.to_token_stream().to_string()),
            scope: self.scope(),
            detached_receiver: false,
        });
        let mut v = CallVisitor::new(id);
        for input in &sig.inputs {
            v.visit_fn_arg(input);
        }
        self.absorb(v);
        id
    }

    fn absorb(&mut self, v: CallVisitor) {
        self.facts.locals[v.caller].extend(v.locals);
        self.facts.calls.extend(v.calls);
    }

    fn collect_calls(&mut self, caller: usize, block: &syn::Block) {
        let mut v = CallVisitor::new(caller);
        v.visit_block(block);
        self.absorb(v);
    }

    fn initializer(&mut self) -> usize {
        if let Some(id) = self.initializer {
            return id;
        }
        let id = self.push(Callable {
            name: INITIALIZER_NAME.to_owned(),
            owner: None,
            kind: CallableKind::Initializer,
            public: false,
            start_line: 0,
            end_line: 0,
            signature: String::new(),
            params: String::new(),
            scope: self.scope(),
            detached_receiver: false,
        });
        self.initializer = Some(id);
        id
    }

    fn items(&mut self, items: &[Item]) {
        for item in items {
            self.item(item);
        }
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Fn(f) => {
                let id = self.push_fn(
                    f.sig.ident.to_string(),
                    None,
                    CallableKind::Function,
                    is_pub(&f.vis),
                    f.span(),
                    &f.sig,
                    &f.vis,
                );
                self.collect_calls(id, &f.block);
            }
            Item::Impl(imp) => {
                let Some(owner) = type_name(&imp.self_ty) else {
                    return;
                };
                let trait_impl = imp.trait_.is_some();
                for it in &imp.items {
                    match it {
                        ImplItem::Fn(m) => {
                            let id = self.push_fn(
                                m.sig.ident.to_string(),
                                Some(owner.clone()),
                                CallableKind::Method,
                                trait_impl || is_pub(&m.vis),
                                m.span(),
                                &m.sig,
                                &m.vis,
                            );
                            self.collect_calls(id, &m.block);
                        }
                        ImplItem::Const(c) => {
                            let init = self.initializer();
                            let mut v = CallVisitor::new(init);
                            v.visit_expr(&c.expr);
                            self.absorb(v);
                        }
                        _ => {}
                    }
                }
            }
            Item::Trait(t) => {
                let owner = t.ident.to_string();
                for it in &t.items {
                    if let TraitItem::Fn(m) = it {
                        let id = self.push_fn(
                            m.sig.ident.to_string(),
                            Some(owner.clone()),
                            CallableKind::Method,
                            is_pub(&t.vis),
                            m.span(),
                            &m.sig,
                            &t.vis,
                        );
                        if let Some(block) = &m.default {
                            self.collect_calls(id, block);
                        }
                    }
                }
            }
            Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    let saved = self.initializer.take();
                    self.scope.push(m.ident.to_string());
                    self.items(items);
                    self.scope.pop();
                    self.initializer = saved;
                }
            }
            Item::Const(c) => {
                let init = self.initializer();
                let mut v = CallVisitor::new(init);
                v.visit_expr(&c.expr);
                self.absorb(v);
            }
            Item::Static(s) => {
                let init = self.initializer();
                let mut v = CallVisitor::new(init);
                v.visit_expr(&s.expr);
                self.absorb(v);
            }
            Item::Use(u) => {
                let mut prefix = Vec::new();
                self.use_tree(&u.tree, &mut prefix);
            }
            _ => {}
        }
    }

    fn use_tree(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
        match tree {
            UseTree::Path(p) => {
                prefix.push(p.ident.to_string());
                self.use_tree(&p.tree, prefix);
                prefix.pop();
            }
            UseTree::Name(n) => {
                let name = n.ident.to_string();
                if name == "self" {
                    if let Some(last) = prefix.last().cloned() {
                        self.facts.imports.push(Import::Path {
                            local: last,
                            path: prefix.clone(),
                            scope: self.scope(),
                        });
                    }
                    return;
                }
                let mut path = prefix.clone();
                path.push(name.clone());
                self.facts.imports.push(Import::Path {
                    local: name,
                    path,
                    scope: self.scope(),
                });
            }
            UseTree::Rename(r) => {
                let mut path = prefix.clone();
                path.push(r.ident.to_string());
                self.facts.imports.push(Import::Path {
                    local: r.rename.to_string(),
                    path,
                    scope: self.scope(),
                });
            }
            UseTree::Group(g) => {
                for t in &g.items {
                    self.use_tree(t, prefix);
                }
            }
            UseTree::Glob(_) => self.facts.imports.push(Import::Glob {
                path: prefix.clone(),
                scope: self.scope(),
            }),
        }
    }
}

/// Collects the call sites of one callable body, and the names bound
/// locally in it (parameters, `let` patterns, closure parameters, nested
/// functions). Calls inside nested items belong to the enclosing callable.
struct CallVisitor {
    caller: usize,
    calls: Vec<RawCall>,
    locals: BTreeSet<String>,
}

impl CallVisitor {
    fn new(caller: usize) -> Self {
        Self {
            caller,
            calls: Vec::new(),
            locals: BTreeSet::new(),
        }
    }
}

impl<'ast> Visit<'ast> for CallVisitor {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let Expr::Path(p) = &*node.func {
            let segs: Vec<String> = p
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            if let Some((name, qualifier)) = segs.split_last() {
                self.calls.push(RawCall {
                    caller: self.caller,
                    line: node.span().start().line as u32,
                    name: name.clone(),
                    form: if qualifier.is_empty() {
                        CallForm::Plain
                    } else {
                        CallForm::Qualified(qualifier.to_vec())
                    },
                    in_lambda: false,
                });
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let self_receiver = matches!(&*node.receiver, Expr::Path(p) if p.path.is_ident("self"));
        self.calls.push(RawCall {
            caller: self.caller,
            line: node.method.span().start().line as u32,
            name: node.method.to_string(),
            form: CallForm::Method { self_receiver },
            in_lambda: false,
        });
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        // Best effort: `format!`, `vec!`, `assert!`, ... whose arguments
        // parse as comma-separated expressions.
        let parser = Punctuated::<Expr, syn::Token![,]>::parse_terminated;
        if let Ok(args) = syn::parse::Parser::parse2(parser, node.tokens.clone()) {
            for arg in &args {
                self.visit_expr(arg);
            }
        }
    }

    fn visit_pat_ident(&mut self, node: &'ast syn::PatIdent) {
        self.locals.insert(node.ident.to_string());
        syn::visit::visit_pat_ident(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        // A nested function is a local name; its body's calls are part of
        // the enclosing callable.
        self.locals.insert(node.sig.ident.to_string());
        syn::visit::visit_item_fn(self, node);
    }

    /// A `use` inside a body binds its names locally (shadowing module
    /// items); a glob `use` makes every plain name in the body uncertain.
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let mut stack = vec![&node.tree];
        while let Some(t) = stack.pop() {
            match t {
                UseTree::Path(p) => stack.push(&p.tree),
                UseTree::Name(n) => {
                    self.locals.insert(n.ident.to_string());
                }
                UseTree::Rename(r) => {
                    self.locals.insert(r.rename.to_string());
                }
                UseTree::Glob(_) => {
                    self.locals.insert(super::LOCAL_GLOB_MARKER.to_owned());
                }
                UseTree::Group(g) => stack.extend(g.items.iter()),
            }
        }
    }

    fn visit_item_mod(&mut self, _: &'ast syn::ItemMod) {}
}

pub(crate) fn extract(path: &str, source: &str, has_lib: impl Fn(&str) -> bool) -> Extracted {
    let file = syn::parse_file(source).map_err(|_| "parse_failed".to_owned())?;
    let mut ex = Extractor {
        facts: FileFacts::default(),
        scope: module_of(path, has_lib)
            .split("::")
            .map(str::to_owned)
            .collect(),
        initializer: None,
    };
    ex.items(&file.items);
    Ok(ex.facts)
}
