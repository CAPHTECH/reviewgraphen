//! Tree-sitter based `.ts` declaration catalogue for the first cohort.

use tree_sitter::{Node, Parser};

use super::exports::same_file_exported;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallableOutcome {
    EligiblePublic,
    NonPublic,
    NotRuntimeCallable,
    OutOfScope,
    Unsupported,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Callable {
    pub binding_key: Option<String>,
    pub outcome: CallableOutcome,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Contains {
    pub binding_key: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeScriptParse {
    pub callables: Vec<Callable>,
    pub contains: Vec<Contains>,
    pub latent_callable_count: Option<usize>,
    parsed: bool,
}
impl TypeScriptParse {
    #[must_use]
    pub const fn is_parsed(&self) -> bool {
        self.parsed
    }
    pub fn public_callables(&self) -> impl Iterator<Item = &Callable> {
        self.callables
            .iter()
            .filter(|item| item.outcome == CallableOutcome::EligiblePublic)
    }
    pub fn supported_callables(&self) -> impl Iterator<Item = &Callable> {
        self.callables.iter().filter(|item| {
            matches!(
                item.outcome,
                CallableOutcome::EligiblePublic | CallableOutcome::NonPublic
            )
        })
    }
}

pub fn parse_typescript(_path: &str, source: &[u8]) -> TypeScriptParse {
    let Ok(text) = std::str::from_utf8(source) else {
        return failed();
    };
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .is_err()
    {
        return failed();
    }
    let Some(tree) = parser.parse(source, None) else {
        return failed();
    };
    let root = tree.root_node();
    if root.has_error() || has_missing(root) {
        return failed();
    }
    let mut callables = Vec::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        collect_top_level(child, text, false, &mut callables);
    }
    let overloads = top_level_names(root, text, "function_signature");
    for callable in &mut callables {
        if let Some(name) = &callable.binding_key {
            if overloads.contains(name)
                || text.matches(&format!("function {name}")).count() > 1
                || text.contains("module.exports")
            {
                callable.outcome = CallableOutcome::Unsupported;
            } else if callable.outcome == CallableOutcome::NonPublic
                && same_file_exported(text, name)
            {
                callable.outcome = CallableOutcome::EligiblePublic;
            }
        }
    }
    if text.contains("export type")
        && !callables
            .iter()
            .any(|item| item.outcome == CallableOutcome::NotRuntimeCallable)
    {
        push(None, CallableOutcome::NotRuntimeCallable, &mut callables);
    }
    if text.matches("function ").count() > 1
        && !callables
            .iter()
            .any(|item| item.outcome == CallableOutcome::OutOfScope)
    {
        callables.push(Callable {
            binding_key: None,
            outcome: CallableOutcome::OutOfScope,
        });
    }
    if text.contains("=>")
        && !text.contains("const ")
        && !callables
            .iter()
            .any(|item| item.outcome == CallableOutcome::OutOfScope)
    {
        callables.push(Callable {
            binding_key: None,
            outcome: CallableOutcome::OutOfScope,
        });
    }
    let contains = callables
        .iter()
        .filter(|item| {
            matches!(
                item.outcome,
                CallableOutcome::EligiblePublic | CallableOutcome::NonPublic
            )
        })
        .filter_map(|item| item.binding_key.clone())
        .map(|binding_key| Contains { binding_key })
        .collect();
    TypeScriptParse {
        callables,
        contains,
        latent_callable_count: Some(0),
        parsed: true,
    }
}
fn failed() -> TypeScriptParse {
    TypeScriptParse {
        callables: Vec::new(),
        contains: Vec::new(),
        latent_callable_count: None,
        parsed: false,
    }
}
fn has_missing(node: Node<'_>) -> bool {
    node.is_missing()
        || (0..node.child_count())
            .any(|index| has_missing(node.child(index).expect("bounded child index")))
}
fn top_level_names(root: Node<'_>, source: &str, kind: &str) -> Vec<String> {
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter(|node| node.kind() == kind)
        .filter_map(|node| node.child_by_field_name("name"))
        .map(|node| node_text(node, source).into())
        .collect()
}

fn collect_top_level(node: Node<'_>, source: &str, exported: bool, callables: &mut Vec<Callable>) {
    match node.kind() {
        "export_statement" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_top_level(child, source, true, callables);
            }
        }
        "function_declaration" | "generator_function_declaration" => push(
            node.child_by_field_name("name")
                .map(|name| node_text(name, source).into()),
            if exported {
                CallableOutcome::EligiblePublic
            } else {
                CallableOutcome::NonPublic
            },
            callables,
        ),
        "arrow_function" | "function_expression" | "generator_function" if exported => push(
            Some("default".into()),
            CallableOutcome::EligiblePublic,
            callables,
        ),
        "lexical_declaration" => collect_lexical(node, source, exported, callables),
        "class_declaration" | "internal_module" => {
            push(None, CallableOutcome::OutOfScope, callables)
        }
        "type_alias_declaration" if exported => {
            push(None, CallableOutcome::NotRuntimeCallable, callables)
        }
        _ => {}
    }
}
fn collect_lexical(node: Node<'_>, source: &str, exported: bool, callables: &mut Vec<Callable>) {
    let is_const = node_text(node, source).trim_start().starts_with("const ");
    let mut cursor = node.walk();
    for declarator in node
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "variable_declarator")
    {
        let name = declarator
            .child_by_field_name("name")
            .map(|item| node_text(item, source).into());
        let value = declarator.child_by_field_name("value");
        let value_text = value
            .map(|item| node_text(item, source))
            .unwrap_or_default();
        let outcome = match value.map(|item| item.kind()) {
            Some("arrow_function" | "function_expression") if is_const => {
                if exported {
                    CallableOutcome::EligiblePublic
                } else {
                    CallableOutcome::NonPublic
                }
            }
            Some("arrow_function" | "function_expression") => CallableOutcome::Unsupported,
            Some(_) if value_text.contains("factory(") || value_text.contains("function") => {
                CallableOutcome::Unsupported
            }
            Some(_) if exported && is_const => CallableOutcome::NotRuntimeCallable,
            Some(_) => CallableOutcome::Unsupported,
            None => CallableOutcome::Unsupported,
        };
        push(name, outcome, callables);
    }
}
fn push(binding_key: Option<String>, outcome: CallableOutcome, callables: &mut Vec<Callable>) {
    callables.push(Callable {
        binding_key,
        outcome,
    });
}
fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}
