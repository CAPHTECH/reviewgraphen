//! TypeScript extraction for source review v6 (tree-sitter).

use super::{
    CallForm, Callable, CallableKind, Extracted, FileFacts, INITIALIZER_NAME, Import, RawCall,
    Reexport, normalize_ws,
};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Node, Parser};

struct Ctx<'s> {
    src: &'s str,
    path: &'s str,
    facts: FileFacts,
    /// Top-level names listed in `export { .. }` clauses.
    exported_names: BTreeSet<String>,
    /// Per callable: a non-private class member (published when its class
    /// is exported by a later `export { Class }`).
    member_visible: Vec<bool>,
    /// Synthetic initializer callable per owner (`None`: module level).
    initializers: BTreeMap<Option<String>, usize>,
    /// Enclosing namespaces (`namespace A.B`), part of the scope.
    namespaces: Vec<String>,
    /// Whether an `export` at this nesting level is visible from outside
    /// the file (false inside a non-exported namespace).
    export_visible: bool,
}

fn text<'s>(src: &'s str, node: Node<'_>) -> &'s str {
    &src[node.byte_range()]
}

fn line(node: Node<'_>) -> u32 {
    node.start_position().row as u32 + 1
}

fn end_line(node: Node<'_>) -> u32 {
    node.end_position().row as u32 + 1
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .to_owned()
}

fn named<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    (0..node.named_child_count()).filter_map(move |i| node.named_child(i))
}

const FUNCTION_VALUES: &[&str] = &[
    "arrow_function",
    "function_expression",
    "function",
    "generator_function",
];

impl<'s> Ctx<'s> {
    fn head(&self, decl: Node<'_>, body: Option<Node<'_>>) -> String {
        let end = body.map_or(decl.end_byte(), |b| b.start_byte());
        normalize_ws(&self.src[decl.start_byte()..end])
    }

    fn params(&self, function: Node<'_>) -> String {
        function
            .child_by_field_name("parameters")
            .or_else(|| function.child_by_field_name("parameter"))
            .map_or_else(String::new, |p| normalize_ws(text(self.src, p)))
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        name: String,
        owner: Option<String>,
        kind: CallableKind,
        public: bool,
        member_visible: bool,
        range: Node<'_>,
        head: String,
        params: String,
    ) -> usize {
        self.facts.callables.push(Callable {
            name,
            owner,
            kind,
            public,
            start_line: line(range),
            end_line: end_line(range),
            signature: head,
            params,
            scope: self.scope(),
            detached_receiver: false,
        });
        self.facts.locals.push(BTreeSet::new());
        self.member_visible.push(member_visible);
        self.facts.callables.len() - 1
    }

    fn scope(&self) -> String {
        if self.namespaces.is_empty() {
            self.path.to_owned()
        } else {
            format!("{}#{}", self.path, self.namespaces.join("."))
        }
    }

    fn initializer(&mut self, owner: Option<&str>) -> usize {
        let key = owner.map(str::to_owned);
        if let Some(&id) = self.initializers.get(&key) {
            return id;
        }
        self.facts.callables.push(Callable {
            name: INITIALIZER_NAME.to_owned(),
            owner: key.clone(),
            kind: CallableKind::Initializer,
            public: false,
            start_line: 0,
            end_line: 0,
            signature: String::new(),
            params: String::new(),
            scope: self.scope(),
            detached_receiver: false,
        });
        self.facts.locals.push(BTreeSet::new());
        self.member_visible.push(false);
        let id = self.facts.callables.len() - 1;
        self.initializers.insert(key, id);
        id
    }

    /// A function-valued declaration: registers it and records its body.
    #[allow(clippy::too_many_arguments)]
    fn function_value(
        &mut self,
        name: String,
        owner: Option<String>,
        kind: CallableKind,
        public: bool,
        member_visible: bool,
        range: Node<'_>,
        function: Node<'_>,
    ) {
        let body = function.child_by_field_name("body");
        let head = self.head(range, body);
        let params = self.params(function);
        let id = self.push(
            name,
            owner,
            kind,
            public,
            member_visible,
            range,
            head,
            params,
        );
        self.bind_parameters(id, function);
        if let Some(b) = body {
            self.calls(b, id);
        }
    }

    fn bind_parameters(&mut self, caller: usize, function: Node<'_>) {
        if let Some(p) = function
            .child_by_field_name("parameters")
            .or_else(|| function.child_by_field_name("parameter"))
        {
            self.bind_pattern(caller, p);
        }
    }

    /// Binds every identifier of a parameter list / binding pattern,
    /// skipping type annotations and default values.
    fn bind_pattern(&mut self, caller: usize, node: Node<'_>) {
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            match n.kind() {
                "identifier" | "shorthand_property_identifier_pattern" => {
                    self.facts.locals[caller].insert(text(self.src, n).to_owned());
                }
                "type_annotation" | "decorator" => {}
                "assignment_pattern" | "required_parameter" | "optional_parameter" => {
                    if let Some(p) = n
                        .child_by_field_name("pattern")
                        .or_else(|| n.child_by_field_name("left"))
                    {
                        stack.push(p);
                    }
                }
                "pair_pattern" => {
                    if let Some(v) = n.child_by_field_name("value") {
                        stack.push(v);
                    }
                }
                _ => stack.extend(named(n)),
            }
        }
    }

    /// Walks module-level statements and declarations.
    fn walk(&mut self, node: Node<'_>, exported: bool) {
        match node.kind() {
            "export_statement" => self.export_statement(node),
            "function_declaration" | "generator_function_declaration" => {
                let Some(name) = node.child_by_field_name("name") else {
                    return;
                };
                let name = text(self.src, name).to_owned();
                self.function_value(
                    name,
                    None,
                    CallableKind::Function,
                    exported,
                    true,
                    node,
                    node,
                );
            }
            "lexical_declaration" | "variable_declaration" => {
                for decl in named(node).filter(|d| d.kind() == "variable_declarator") {
                    let (Some(name), value) = (
                        decl.child_by_field_name("name"),
                        decl.child_by_field_name("value"),
                    ) else {
                        continue;
                    };
                    match value {
                        Some(v)
                            if FUNCTION_VALUES.contains(&v.kind())
                                && name.kind() == "identifier" =>
                        {
                            let n = text(self.src, name).to_owned();
                            self.function_value(
                                n,
                                None,
                                CallableKind::Function,
                                exported,
                                true,
                                decl,
                                v,
                            );
                        }
                        Some(v) => {
                            let init = self.initializer(None);
                            self.calls(v, init);
                        }
                        None => {}
                    }
                }
            }
            "class_declaration" | "abstract_class_declaration" | "class" => {
                let owner = node
                    .child_by_field_name("name")
                    .map(|n| text(self.src, n).to_owned());
                if let (Some(owner), Some(body)) = (owner, node.child_by_field_name("body")) {
                    self.class_body(body, &owner, exported);
                }
            }
            "import_statement" => self.import(node),
            // Members of a namespace are exported only by their own `export`,
            // and only if the namespace itself is visible.
            "internal_module" | "module" => {
                let Some(body) = node.child_by_field_name("body") else {
                    return;
                };
                let name = node
                    .child_by_field_name("name")
                    .map(|n| normalize_ws(text(self.src, n)).replace(' ', ""))
                    .unwrap_or_default();
                let saved = self.export_visible;
                self.export_visible = saved && exported;
                self.namespaces.push(name);
                for c in named(body) {
                    self.walk(c, false);
                }
                self.namespaces.pop();
                self.export_visible = saved;
            }
            // `namespace M {}` at statement level parses as an expression.
            "expression_statement"
                if named(node).count() == 1
                    && named(node).all(|c| matches!(c.kind(), "internal_module" | "module")) =>
            {
                for c in named(node) {
                    self.walk(c, exported);
                }
            }
            "program" | "statement_block" => {
                for c in named(node) {
                    self.walk(c, false);
                }
            }
            "ambient_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "comment" => {}
            _ => {
                // Module-level expression statements and the like.
                let init = self.initializer(None);
                self.calls(node, init);
            }
        }
    }

    fn export_statement(&mut self, node: Node<'_>) {
        let source = node
            .child_by_field_name("source")
            .map(|s| unquote(text(self.src, s)));
        let visible = self.export_visible;
        for d in named(node).filter(|d| d.kind() == "decorator") {
            let init = self.initializer(None);
            self.calls(d, init);
        }
        let is_default = (0..node.child_count())
            .filter_map(|i| node.child(i))
            .any(|c| c.kind() == "default");
        if let Some(decl) = node.child_by_field_name("declaration") {
            if is_default
                && matches!(
                    decl.kind(),
                    "function_declaration"
                        | "generator_function_declaration"
                        | "class_declaration"
                        | "abstract_class_declaration"
                        | "class"
                )
                && let Some(n) = decl.child_by_field_name("name")
            {
                self.facts.default_export = Some(text(self.src, n).to_owned());
            }
            self.walk(decl, visible);
            return;
        }
        if let Some(module) = source {
            // `export { a as b } from "./m"` / `export * from "./m"`.
            let clause = named(node).find(|c| c.kind() == "export_clause");
            match clause {
                Some(clause) => {
                    for spec in named(clause).filter(|s| s.kind() == "export_specifier") {
                        let Some(name) = spec.child_by_field_name("name") else {
                            continue;
                        };
                        let imported = unquote(text(self.src, name));
                        let exported = spec
                            .child_by_field_name("alias")
                            .map_or_else(|| imported.clone(), |a| unquote(text(self.src, a)));
                        self.facts.reexports.push(Reexport {
                            exported,
                            module: module.clone(),
                            imported,
                        });
                    }
                }
                None if !named(node).any(|c| c.kind() == "namespace_export") => {
                    self.facts.reexports.push(Reexport {
                        exported: "*".to_owned(),
                        module,
                        imported: "*".to_owned(),
                    });
                }
                None => {}
            }
            return;
        }
        if is_default {
            // `export default function () {}` / `export default () => ..`
            for c in named(node) {
                if FUNCTION_VALUES.contains(&c.kind()) {
                    self.function_value(
                        "default".to_owned(),
                        None,
                        CallableKind::Function,
                        visible,
                        true,
                        c,
                        c,
                    );
                } else if matches!(
                    c.kind(),
                    "function_declaration"
                        | "generator_function_declaration"
                        | "class"
                        | "class_declaration"
                ) {
                    if let Some(n) = c.child_by_field_name("name") {
                        self.facts.default_export = Some(text(self.src, n).to_owned());
                    }
                    self.walk(c, visible);
                } else if c.kind() == "identifier" {
                    // `export default name;`
                    let local = text(self.src, c).to_owned();
                    self.facts
                        .export_aliases
                        .push(("default".to_owned(), local.clone()));
                    self.exported_names.insert(local);
                } else if c.kind() != "decorator" {
                    let init = self.initializer(None);
                    self.calls(c, init);
                }
            }
            return;
        }
        // `export { a, b as c }` without a source.
        for spec in named(node)
            .filter(|c| c.kind() == "export_clause")
            .flat_map(named)
            .filter(|s| s.kind() == "export_specifier")
        {
            if let Some(name) = spec.child_by_field_name("name") {
                let local = text(self.src, name).to_owned();
                if let Some(alias) = spec.child_by_field_name("alias") {
                    self.facts
                        .export_aliases
                        .push((unquote(text(self.src, alias)), local.clone()));
                }
                self.exported_names.insert(local);
            }
        }
    }

    fn class_body(&mut self, body: Node<'_>, owner: &str, exported: bool) {
        // Class-level decorators precede the body.
        if let Some(class) = body.parent() {
            for d in named(class).filter(|d| d.kind() == "decorator") {
                let init = self.initializer(Some(owner));
                self.calls(d, init);
            }
        }
        for m in named(body) {
            // Member and parameter decorators run at class definition time.
            let mut decorators = Vec::new();
            let mut stack: Vec<Node<'_>> = named(m).collect();
            while let Some(n) = stack.pop() {
                if n.kind() == "decorator" {
                    decorators.push(n);
                } else if matches!(
                    n.kind(),
                    "formal_parameters" | "required_parameter" | "optional_parameter"
                ) {
                    stack.extend(named(n));
                }
            }
            for d in decorators {
                let init = self.initializer(Some(owner));
                self.calls(d, init);
            }
            let restricted = |ctx: &Self, name_node: Node<'_>| {
                name_node.kind() == "private_property_identifier"
                    || (0..m.child_count()).filter_map(|j| m.child(j)).any(|c| {
                        c.kind() == "accessibility_modifier" && text(ctx.src, c) != "public"
                    })
            };
            match m.kind() {
                "method_definition" | "abstract_method_signature" => {
                    let Some(name_node) = m.child_by_field_name("name") else {
                        continue;
                    };
                    let r = restricted(self, name_node);
                    let name = text(self.src, name_node).to_owned();
                    self.function_value(
                        name,
                        Some(owner.to_owned()),
                        CallableKind::Method,
                        exported && !r,
                        !r,
                        m,
                        m,
                    );
                }
                "public_field_definition" | "field_definition" => {
                    let Some(name_node) = m.child_by_field_name("name") else {
                        continue;
                    };
                    match m.child_by_field_name("value") {
                        Some(v) if FUNCTION_VALUES.contains(&v.kind()) => {
                            let r = restricted(self, name_node);
                            let name = text(self.src, name_node).to_owned();
                            self.function_value(
                                name,
                                Some(owner.to_owned()),
                                CallableKind::Method,
                                exported && !r,
                                !r,
                                m,
                                v,
                            );
                        }
                        Some(v) => {
                            let init = self.initializer(Some(owner));
                            self.calls(v, init);
                        }
                        None => {}
                    }
                }
                "class_static_block" => {
                    let init = self.initializer(Some(owner));
                    self.calls(m, init);
                }
                _ => {}
            }
        }
    }

    fn import(&mut self, node: Node<'_>) {
        let Some(source) = node.child_by_field_name("source") else {
            return;
        };
        let module = unquote(text(self.src, source));
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            match n.kind() {
                "import_specifier" => {
                    let Some(name) = n.child_by_field_name("name") else {
                        continue;
                    };
                    let imported = unquote(text(self.src, name));
                    let local = n
                        .child_by_field_name("alias")
                        .map_or_else(|| imported.clone(), |a| text(self.src, a).to_owned());
                    self.facts.imports.push(Import::Named {
                        local,
                        module: module.clone(),
                        imported,
                    });
                }
                "namespace_import" => {
                    if let Some(id) = named(n).find(|c| c.kind() == "identifier") {
                        self.facts.imports.push(Import::Namespace {
                            local: text(self.src, id).to_owned(),
                            module: module.clone(),
                        });
                    }
                }
                "import_clause" => {
                    for c in named(n) {
                        if c.kind() == "identifier" {
                            self.facts.imports.push(Import::Named {
                                local: text(self.src, c).to_owned(),
                                module: module.clone(),
                                imported: "default".to_owned(),
                            });
                        } else {
                            stack.push(c);
                        }
                    }
                }
                _ => stack.extend(named(n)),
            }
        }
    }

    /// Records call sites under `node` for `caller`. Nested function
    /// declarations and local variables are local names of `caller`; their
    /// bodies' calls belong to `caller`.
    fn calls(&mut self, node: Node<'_>, caller: usize) {
        // `rebound`: inside a non-arrow function, object-literal method or
        // class, where `this` is no longer the enclosing class instance.
        let mut stack = vec![(node, false)];
        while let Some((n, rebound)) = stack.pop() {
            let rebinds = rebound
                || (n != node
                    && matches!(
                        n.kind(),
                        "function_expression"
                            | "function"
                            | "generator_function"
                            | "function_declaration"
                            | "generator_function_declaration"
                            | "method_definition"
                            | "class"
                            | "class_declaration"
                    ));
            match n.kind() {
                "function_declaration" | "generator_function_declaration" | "class_declaration" => {
                    if let Some(name) = n.child_by_field_name("name") {
                        self.facts.locals[caller].insert(text(self.src, name).to_owned());
                    }
                    self.bind_parameters(caller, n);
                }
                "variable_declarator" => {
                    if let Some(name) = n.child_by_field_name("name") {
                        self.bind_pattern(caller, name);
                    }
                }
                "arrow_function" | "function_expression" | "function" | "method_definition" => {
                    self.bind_parameters(caller, n);
                    // A named function expression binds its own name inside.
                    if n.kind() != "method_definition"
                        && let Some(name) = n.child_by_field_name("name")
                    {
                        self.facts.locals[caller].insert(text(self.src, name).to_owned());
                    }
                }
                "for_in_statement" => {
                    if let Some(left) = n.child_by_field_name("left") {
                        self.bind_pattern(caller, left);
                    }
                }
                "catch_clause" => {
                    if let Some(p) = n.child_by_field_name("parameter") {
                        self.bind_pattern(caller, p);
                    }
                }
                "call_expression" => {
                    if let Some(f) = n.child_by_field_name("function") {
                        self.call(f, n, caller, rebinds);
                    }
                }
                "new_expression" => {
                    if let Some(c) = n.child_by_field_name("constructor")
                        && c.kind() == "identifier"
                    {
                        self.facts.calls.push(RawCall {
                            caller,
                            line: line(n),
                            name: "constructor".to_owned(),
                            form: CallForm::Qualified(vec![text(self.src, c).to_owned()]),
                            in_lambda: false,
                        });
                    }
                }
                "jsx_opening_element" | "jsx_self_closing_element" => {
                    if let Some(name) = n.child_by_field_name("name") {
                        let tag = text(self.src, name);
                        if name.kind() == "identifier"
                            && tag.chars().next().is_some_and(char::is_uppercase)
                        {
                            self.facts.calls.push(RawCall {
                                caller,
                                line: line(n),
                                name: tag.to_owned(),
                                form: CallForm::Plain,
                                in_lambda: false,
                            });
                        }
                    }
                }
                _ => {}
            }
            for i in (0..n.named_child_count()).rev() {
                if let Some(c) = n.named_child(i) {
                    stack.push((c, rebinds));
                }
            }
        }
    }

    fn call(&mut self, f: Node<'_>, call: Node<'_>, caller: usize, this_rebound: bool) {
        let (name, form) = match f.kind() {
            "identifier" => (text(self.src, f).to_owned(), CallForm::Plain),
            "member_expression" => {
                let (Some(obj), Some(prop)) = (
                    f.child_by_field_name("object"),
                    f.child_by_field_name("property"),
                ) else {
                    return;
                };
                let name = text(self.src, prop).to_owned();
                let optional = f.child_by_field_name("optional_chain").is_some()
                    || self.src[obj.end_byte()..prop.start_byte()].contains("?.");
                let form = match obj.kind() {
                    "this" => CallForm::Method {
                        self_receiver: !this_rebound,
                    },
                    "identifier" if !optional => {
                        CallForm::Qualified(vec![text(self.src, obj).to_owned()])
                    }
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
            line: line(call),
            name,
            form,
            in_lambda: false,
        });
    }
}

pub(crate) fn extract(path: &str, source: &str) -> Extracted {
    let mut parser = Parser::new();
    let language = if path.ends_with(".tsx") {
        tree_sitter_typescript::LANGUAGE_TSX
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT
    };
    parser
        .set_language(&language.into())
        .map_err(|_| "parser_unavailable".to_owned())?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "parse_failed".to_owned())?;
    let root = tree.root_node();
    let mut ctx = Ctx {
        src: source,
        path,
        facts: FileFacts::default(),
        exported_names: BTreeSet::new(),
        member_visible: Vec::new(),
        initializers: BTreeMap::new(),
        namespaces: Vec::new(),
        export_visible: true,
    };
    ctx.facts.script_mode =
        !named(root).any(|c| matches!(c.kind(), "import_statement" | "export_statement"));
    ctx.walk(root, false);
    // Late `export { name }` clauses publish top-level functions and the
    // visible members of exported classes.
    let exported = std::mem::take(&mut ctx.exported_names);
    for (c, visible) in ctx.facts.callables.iter_mut().zip(&ctx.member_visible) {
        if c.kind == CallableKind::Initializer {
            continue;
        }
        let named = match &c.owner {
            None => exported.contains(&c.name),
            Some(owner) => *visible && exported.contains(owner),
        };
        if named {
            c.public = true;
        }
    }
    ctx.facts.syntax_error_lines = super::tree_error_lines(root);
    Ok(ctx.facts)
}
