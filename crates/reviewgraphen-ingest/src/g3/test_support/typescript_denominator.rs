//! Independent, byte-bound TypeScript syntax oracle for the frozen test fixture.

use std::collections::BTreeSet;

use reviewgraphen_core::ContentHash;
use tree_sitter::{Node, Parser};

const LENGTH: usize = 123;
const SHA256: &str = "sha256:eca129e7be017d0a741b7a35a3183bccb61ae6753916ec83209a4c651c1cf9d7";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct TypeScriptOracleRangeV1 {
    start: u64,
    end: u64,
}

impl TypeScriptOracleRangeV1 {
    pub(crate) const fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }

    fn from_node(node: Node<'_>, length: usize) -> Result<Self, TypeScriptDenominatorError> {
        let range = Self::new(node.start_byte() as u64, node.end_byte() as u64);
        if range.start >= range.end || node.end_byte() > length {
            return Err(TypeScriptDenominatorError::InvalidRange);
        }
        Ok(range)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum TypeScriptDenominatorKeyV1 {
    Declaration {
        range: TypeScriptOracleRangeV1,
    },
    Containment {
        parent: TypeScriptOracleRangeV1,
        child: TypeScriptOracleRangeV1,
    },
    DirectCall {
        range: TypeScriptOracleRangeV1,
    },
    Write {
        range: TypeScriptOracleRangeV1,
    },
    CallCandidate {
        range: TypeScriptOracleRangeV1,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum TypeScriptDenominatorError {
    WrongLength { expected: usize, actual: usize },
    WrongHash { actual: String },
    InvalidUtf8,
    Parse { message: String },
    InvalidRange,
    Duplicate { key: TypeScriptDenominatorKeyV1 },
    OutOfTable { key: TypeScriptDenominatorKeyV1 },
    UnexpectedConstruct { kind: &'static str },
    Missing { key: TypeScriptDenominatorKeyV1 },
}

const EXPECTED: [TypeScriptDenominatorKeyV1; 5] = [
    TypeScriptDenominatorKeyV1::Declaration {
        range: TypeScriptOracleRangeV1::new(7, 27),
    },
    TypeScriptDenominatorKeyV1::Containment {
        parent: TypeScriptOracleRangeV1::new(0, 123),
        child: TypeScriptOracleRangeV1::new(35, 98),
    },
    TypeScriptDenominatorKeyV1::DirectCall {
        range: TypeScriptOracleRangeV1::new(87, 95),
    },
    TypeScriptDenominatorKeyV1::Write {
        range: TypeScriptOracleRangeV1::new(74, 83),
    },
    TypeScriptDenominatorKeyV1::CallCandidate {
        range: TypeScriptOracleRangeV1::new(99, 121),
    },
];

struct Oracle<'a> {
    bytes: &'a [u8],
    keys: BTreeSet<TypeScriptDenominatorKeyV1>,
    program: TypeScriptOracleRangeV1,
}

impl Oracle<'_> {
    fn name<'tree>(&self, node: Node<'tree>) -> Option<&str> {
        std::str::from_utf8(&self.bytes[node.start_byte()..node.end_byte()]).ok()
    }

    fn range(&self, node: Node<'_>) -> Result<TypeScriptOracleRangeV1, TypeScriptDenominatorError> {
        TypeScriptOracleRangeV1::from_node(node, self.bytes.len())
    }

    fn record(
        &mut self,
        key: TypeScriptDenominatorKeyV1,
    ) -> Result<(), TypeScriptDenominatorError> {
        if !EXPECTED.contains(&key) {
            return Err(TypeScriptDenominatorError::OutOfTable { key });
        }
        if !self.keys.insert(key.clone()) {
            return Err(TypeScriptDenominatorError::Duplicate { key });
        }
        Ok(())
    }

    fn walk(&mut self, node: Node<'_>) -> Result<(), TypeScriptDenominatorError> {
        match node.kind() {
            "function_declaration" => {
                let name = node
                    .child_by_field_name("name")
                    .and_then(|name| self.name(name))
                    .ok_or(TypeScriptDenominatorError::UnexpectedConstruct {
                        kind: "unnamed function",
                    })?;
                let range = self.range(node)?;
                match name {
                    "callee" => self.record(TypeScriptDenominatorKeyV1::Declaration { range })?,
                    "caller" => {
                        // This is a file-lexical function, not a nested callable.
                        let parent = node.parent().ok_or(
                            TypeScriptDenominatorError::UnexpectedConstruct {
                                kind: "caller without parent",
                            },
                        )?;
                        let direct_file_child = parent.kind() == "program"
                            || (parent.kind() == "export_statement"
                                && parent
                                    .parent()
                                    .is_some_and(|outer| outer.kind() == "program"));
                        if !direct_file_child {
                            return Err(TypeScriptDenominatorError::UnexpectedConstruct {
                                kind: "nested caller",
                            });
                        }
                        self.record(TypeScriptDenominatorKeyV1::Containment {
                            parent: self.program,
                            child: range,
                        })?;
                    }
                    _ => {
                        return Err(TypeScriptDenominatorError::UnexpectedConstruct {
                            kind: "function",
                        });
                    }
                }
            }
            "call_expression" => {
                let callee = node
                    .child_by_field_name("function")
                    .and_then(|callee| self.name(callee))
                    .ok_or(TypeScriptDenominatorError::UnexpectedConstruct {
                        kind: "call target",
                    })?;
                let range = self.range(node)?;
                match callee {
                    "callee" => self.record(TypeScriptDenominatorKeyV1::DirectCall { range })?,
                    "it" => self.record(TypeScriptDenominatorKeyV1::CallCandidate { range })?,
                    _ => {
                        return Err(TypeScriptDenominatorError::UnexpectedConstruct {
                            kind: "call",
                        });
                    }
                }
            }
            "assignment_expression" => {
                let left = node.child_by_field_name("left").ok_or(
                    TypeScriptDenominatorError::UnexpectedConstruct {
                        kind: "assignment lhs",
                    },
                )?;
                if left.kind() != "identifier" || self.name(left) != Some("value") {
                    return Err(TypeScriptDenominatorError::UnexpectedConstruct {
                        kind: "assignment lhs",
                    });
                }
                let range = self.range(node)?;
                self.record(TypeScriptDenominatorKeyV1::Write { range })?;
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.walk(child)?;
        }
        Ok(())
    }
}

fn has_missing(node: Node<'_>) -> bool {
    if node.is_missing() {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor).any(has_missing)
}

pub(crate) fn typescript_denominator(
    bytes: &[u8],
) -> Result<BTreeSet<TypeScriptDenominatorKeyV1>, TypeScriptDenominatorError> {
    if bytes.len() != LENGTH {
        return Err(TypeScriptDenominatorError::WrongLength {
            expected: LENGTH,
            actual: bytes.len(),
        });
    }
    let hash = ContentHash::sha256(bytes);
    if hash.as_str() != SHA256 {
        return Err(TypeScriptDenominatorError::WrongHash {
            actual: hash.as_str().to_owned(),
        });
    }
    std::str::from_utf8(bytes).map_err(|_| TypeScriptDenominatorError::InvalidUtf8)?;
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .map_err(|error| TypeScriptDenominatorError::Parse {
            message: error.to_string(),
        })?;
    let tree = parser
        .parse(bytes, None)
        .ok_or(TypeScriptDenominatorError::Parse {
            message: "parser returned no tree".to_owned(),
        })?;
    let root = tree.root_node();
    if root.has_error() || has_missing(root) {
        return Err(TypeScriptDenominatorError::Parse {
            message: "invalid or missing syntax".to_owned(),
        });
    }
    let program = TypeScriptOracleRangeV1::from_node(root, bytes.len())?;
    if program != TypeScriptOracleRangeV1::new(0, LENGTH as u64) {
        return Err(TypeScriptDenominatorError::InvalidRange);
    }
    let mut oracle = Oracle {
        bytes,
        keys: BTreeSet::new(),
        program,
    };
    oracle.walk(root)?;
    for key in EXPECTED {
        if !oracle.keys.contains(&key) {
            return Err(TypeScriptDenominatorError::Missing { key });
        }
    }
    Ok(oracle.keys)
}
