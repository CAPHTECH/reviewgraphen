//! Kotlin extraction for source review v6 (tree-sitter-kotlin-ng).

use super::{
    CallForm, Callable, CallableKind, Extracted, FileFacts, INITIALIZER_NAME, Import, RawCall,
    normalize_ws,
};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Node, Parser};

struct Ctx<'s> {
    src: &'s str,
    package: String,
    facts: FileFacts,
    /// Synthetic initializer callable per owner (`None`: file level).
    initializers: BTreeMap<Option<String>, usize>,
}

fn text<'s>(src: &'s str, node: Node<'_>) -> &'s str {
    &src[node.byte_range()]
}

fn children<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    (0..node.named_child_count()).filter_map(move |i| node.named_child(i))
}

/// Public in the Kotlin sense relevant to review: no `private`, `internal`
/// or `protected` visibility modifier (Kotlin's default is public).
fn declared_public(src: &str, node: Node<'_>) -> bool {
    children(node)
        .filter(|c| c.kind() == "modifiers")
        .flat_map(children)
        .filter(|m| m.kind() == "visibility_modifier")
        .all(|m| text(src, m) == "public")
}

/// Last simple identifier of a (possibly generic or nullable) type.
fn type_simple_name(src: &str, ty: Node<'_>) -> Option<String> {
    let raw = text(src, ty);
    let base = raw
        .split('<')
        .next()
        .unwrap_or(raw)
        .trim_end_matches('?')
        .trim();
    base.rsplit('.')
        .next()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Declarations whose code runs outside any function body; calls in them
/// belong to the owner's synthetic initializer.
const INITIALIZER_CONTEXTS: &[&str] = &[
    "property_declaration",
    "anonymous_initializer",
    "secondary_constructor",
    "getter",
    "setter",
    "primary_constructor",
    "delegation_specifiers",
    "enum_entry",
];

#[derive(Clone, Copy)]
struct Scope<'o> {
    owner: Option<&'o str>,
    visible: bool,
    caller: Option<usize>,
    in_lambda: bool,
    /// Inside a companion object: no implicit instance of `owner`.
    companion: bool,
}

impl Ctx<'_> {
    fn push(&mut self, callable: Callable) -> usize {
        self.facts.callables.push(callable);
        self.facts.locals.push(BTreeSet::new());
        self.facts.callables.len() - 1
    }

    fn initializer(&mut self, owner: Option<&str>, companion: bool) -> usize {
        let key = owner.map(|o| {
            if companion {
                format!("{o}.companion")
            } else {
                o.to_owned()
            }
        });
        if let Some(&id) = self.initializers.get(&key) {
            return id;
        }
        let id = self.push(Callable {
            name: INITIALIZER_NAME.to_owned(),
            owner: key.clone(),
            kind: CallableKind::Initializer,
            public: false,
            start_line: 0,
            end_line: 0,
            signature: String::new(),
            params: String::new(),
            scope: self.package.clone(),
            detached_receiver: companion,
        });
        self.initializers.insert(key, id);
        id
    }

    fn bind_local(&mut self, caller: Option<usize>, node: Node<'_>) {
        let Some(caller) = caller else { return };
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if n.kind() == "identifier" {
                self.facts.locals[caller].insert(text(self.src, n).to_owned());
                continue;
            }
            if matches!(
                n.kind(),
                "type" | "user_type" | "nullable_type" | "annotation" | "modifiers" | "expression"
            ) {
                continue;
            }
            stack.extend(children(n));
        }
    }

    fn walk(&mut self, node: Node<'_>, scope: Scope<'_>) {
        match node.kind() {
            "package_header" => {
                if let Some(q) = children(node)
                    .find(|c| c.kind() == "qualified_identifier" || c.kind() == "identifier")
                {
                    self.package = normalize_ws(text(self.src, q)).replace(' ', "");
                }
            }
            "import" => self.import(node),
            "function_declaration" => self.function(node, scope),
            "class_declaration" | "object_declaration" => {
                let name = node
                    .child_by_field_name("name")
                    .map(|n| text(self.src, n).to_owned());
                let own = name.as_deref().or(scope.owner);
                let inner = Scope {
                    owner: own,
                    visible: scope.visible
                        && scope.caller.is_none()
                        && declared_public(self.src, node),
                    caller: None,
                    in_lambda: false,
                    companion: false,
                };
                for c in children(node) {
                    match c.kind() {
                        "class_body" | "enum_class_body" => self.walk(c, inner),
                        k if INITIALIZER_CONTEXTS.contains(&k) => {
                            let init = self.initializer(own, false);
                            self.walk_children(
                                c,
                                Scope {
                                    caller: Some(init),
                                    ..inner
                                },
                            );
                        }
                        _ => {}
                    }
                }
            }
            "companion_object" => {
                // Members are called as `Owner.f()`; inside, the implicit
                // receiver is the companion, not an `Owner` instance.
                let inner = Scope {
                    visible: scope.visible && declared_public(self.src, node),
                    caller: None,
                    in_lambda: false,
                    companion: true,
                    ..scope
                };
                for c in children(node) {
                    if c.kind() == "class_body" {
                        self.walk(c, inner);
                    }
                }
            }
            "lambda_literal" | "annotated_lambda" | "anonymous_function" => {
                self.walk_children(
                    node,
                    Scope {
                        in_lambda: true,
                        ..scope
                    },
                );
            }
            "variable_declaration" | "parameter" | "lambda_parameters" => {
                self.bind_local(scope.caller, node);
                self.walk_children(node, scope);
            }
            "call_expression" => {
                if let Some(c) = scope.caller {
                    self.call(node, c, scope.in_lambda);
                }
                self.walk_children(node, scope);
            }
            k if scope.caller.is_none() && INITIALIZER_CONTEXTS.contains(&k) => {
                // A top-level extension property (`val R.x get() = ..`) runs
                // with an implicit `R` receiver.
                let receiver = (scope.owner.is_none() && k == "property_declaration")
                    .then(|| {
                        children(node)
                            .take_while(|c| {
                                !matches!(
                                    c.kind(),
                                    "variable_declaration" | "multi_variable_declaration"
                                )
                            })
                            .filter(|c| matches!(c.kind(), "user_type" | "nullable_type" | "type"))
                            .last()
                            .and_then(|t| type_simple_name(self.src, t))
                    })
                    .flatten();
                let init = self.initializer(receiver.as_deref().or(scope.owner), scope.companion);
                self.walk_children(
                    node,
                    Scope {
                        caller: Some(init),
                        ..scope
                    },
                );
            }
            _ => self.walk_children(node, scope),
        }
    }

    fn walk_children(&mut self, node: Node<'_>, scope: Scope<'_>) {
        for c in children(node) {
            self.walk(c, scope);
        }
    }

    fn function(&mut self, node: Node<'_>, scope: Scope<'_>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let name_text = text(self.src, name).to_owned();
        let body = children(node).find(|c| c.kind() == "function_body");
        let params = children(node).find(|c| c.kind() == "function_value_parameters");
        // A local function is a local name of its enclosing callable; its
        // body's calls belong to that callable.
        if let Some(caller) = scope.caller {
            self.facts.locals[caller].insert(name_text);
            if let Some(p) = params {
                self.walk(p, scope);
            }
            if let Some(b) = body {
                self.walk(b, scope);
            }
            return;
        }
        // A type written before the name is an extension receiver.
        let receiver = children(node)
            .filter(|c| c.end_byte() <= name.start_byte())
            .filter(|c| {
                matches!(
                    c.kind(),
                    "user_type" | "nullable_type" | "type" | "parenthesized_type"
                )
            })
            .last()
            .and_then(|t| type_simple_name(self.src, t));
        let head_end = body.map_or(node.end_byte(), |b| b.start_byte());
        let signature = normalize_ws(&self.src[node.start_byte()..head_end]);
        let detached = scope.companion && receiver.is_none();
        let (kind, own) = match (scope.owner, receiver) {
            (_, Some(r)) => (CallableKind::Method, Some(r)),
            (Some(o), None) => (CallableKind::Method, Some(o.to_owned())),
            (None, None) => (CallableKind::Function, None),
        };
        let id = self.push(Callable {
            name: name_text,
            owner: own.clone(),
            kind,
            public: scope.visible && declared_public(self.src, node),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature,
            params: params.map_or_else(String::new, |p| normalize_ws(text(self.src, p))),
            scope: self.package.clone(),
            // An extension function has its own receiver even in a companion.
            detached_receiver: detached,
        });
        let owner = own.as_deref().or(scope.owner);
        let inner = Scope {
            owner,
            visible: scope.visible,
            caller: Some(id),
            in_lambda: false,
            companion: detached,
        };
        // Parameters bind names; default values are code of this callable.
        if let Some(p) = params {
            self.walk(p, inner);
        }
        if let Some(b) = body {
            self.walk(b, inner);
        }
    }

    fn import(&mut self, node: Node<'_>) {
        let raw = normalize_ws(text(self.src, node));
        let rest = raw.trim_start_matches("import").trim();
        if let Some(pkg) = rest.strip_suffix(".*") {
            self.facts.imports.push(Import::Glob {
                path: pkg.split('.').map(|s| s.trim().to_owned()).collect(),
                scope: self.package.clone(),
            });
            return;
        }
        let (path, alias) = match rest.split_once(" as ") {
            Some((p, a)) => (p.trim(), Some(a.trim())),
            None => (rest, None),
        };
        let segs: Vec<String> = path.split('.').map(|s| s.trim().to_owned()).collect();
        let Some(last) = segs.last().cloned() else {
            return;
        };
        self.facts.imports.push(Import::Path {
            local: alias.map_or(last, str::to_owned),
            path: segs,
            scope: self.package.clone(),
        });
    }

    /// Identifier chain of a pure `a.b.c` navigation, or `None`.
    fn chain(&self, node: Node<'_>) -> Option<Vec<String>> {
        match node.kind() {
            "identifier" => Some(vec![text(self.src, node).to_owned()]),
            "navigation_expression" => {
                // `a?.b` is a value navigation, never a package path.
                if text(self.src, node).contains("?.") {
                    return None;
                }
                let kids: Vec<Node<'_>> = children(node).collect();
                let (last, first) = kids.split_last()?;
                if last.kind() != "identifier" || first.len() != 1 {
                    return None;
                }
                let mut head = self.chain(first[0])?;
                head.push(text(self.src, *last).to_owned());
                Some(head)
            }
            _ => None,
        }
    }

    fn call(&mut self, call: Node<'_>, caller: usize, in_lambda: bool) {
        let Some(callee) = call.named_child(0) else {
            return;
        };
        let line = call.start_position().row as u32 + 1;
        let (name, form) = match callee.kind() {
            "identifier" => (text(self.src, callee).to_owned(), CallForm::Plain),
            "navigation_expression" => {
                let kids: Vec<Node<'_>> = children(callee).collect();
                let Some((member, receiver)) = kids.split_last() else {
                    return;
                };
                if member.kind() != "identifier" {
                    return;
                }
                let name = text(self.src, *member).to_owned();
                let safe = self.src[callee.start_byte()..member.start_byte()]
                    .trim_end()
                    .ends_with("?.");
                let form = match receiver {
                    // `this@Label` names an outer receiver, not the caller's own.
                    [r] if r.kind() == "this_expression" => CallForm::Method {
                        self_receiver: !text(self.src, *r).contains('@'),
                    },
                    [r] if !safe => match self.chain(*r) {
                        Some(segs) => CallForm::Qualified(segs),
                        None => CallForm::Method {
                            self_receiver: false,
                        },
                    },
                    _ => CallForm::Method {
                        self_receiver: false,
                    },
                };
                (name, form)
            }
            _ => return,
        };
        self.facts.calls.push(RawCall {
            caller,
            line,
            name,
            form,
            in_lambda,
        });
    }
}

pub(crate) fn extract(_path: &str, source: &str) -> Extracted {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
        .map_err(|_| "parser_unavailable".to_owned())?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "parse_failed".to_owned())?;
    let root = tree.root_node();
    let mut ctx = Ctx {
        src: source,
        package: String::new(),
        facts: FileFacts::default(),
        initializers: BTreeMap::new(),
    };
    let top = Scope {
        owner: None,
        visible: true,
        caller: None,
        in_lambda: false,
        companion: false,
    };
    // The package header precedes declarations; read it first.
    for c in children(root) {
        if c.kind() == "package_header" {
            ctx.walk(c, top);
        }
    }
    for c in children(root) {
        if c.kind() != "package_header" {
            ctx.walk(c, top);
        }
    }
    ctx.facts.syntax_error_lines = super::tree_error_lines(root);
    Ok(ctx.facts)
}
