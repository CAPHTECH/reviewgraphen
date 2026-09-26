//! Byte-bound, test-only Rust syntax oracle; no production G3 facts enter this module.

use std::collections::BTreeSet;

use proc_macro2::Span;
use reviewgraphen_core::ContentHash;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

const LENGTH: usize = 151;
const SHA256: &str = "sha256:e22111d84a1611a511f1c7bccaeca7abb03c058ab51fc753511c75e3f087eadd";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct RustOracleRangeV1 {
    start_line: u64,
    start_column: u64,
    end_line: u64,
    end_column: u64,
}

impl RustOracleRangeV1 {
    pub(crate) const fn new(sl: u64, sc: u64, el: u64, ec: u64) -> Self {
        Self {
            start_line: sl,
            start_column: sc,
            end_line: el,
            end_column: ec,
        }
    }

    fn from_span(span: Span) -> Result<Self, RustDenominatorError> {
        Self::from_spans(span, span)
    }

    // syn's item span includes leading attributes. An item identity starts at
    // its syntax keyword, but keeps the complete item's closing token as end.
    fn from_spans(start_span: Span, end_span: Span) -> Result<Self, RustDenominatorError> {
        let start = start_span.start();
        let end = end_span.end();
        let range = Self::new(
            start.line as u64,
            start.column as u64 + 1,
            end.line as u64,
            end.column as u64,
        );
        if range.start_line == 0
            || range.start_column == 0
            || range.end_line == 0
            || range.end_column == 0
            || (range.end_line, range.end_column) < (range.start_line, range.start_column)
        {
            return Err(RustDenominatorError::InvalidRange);
        }
        Ok(range)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum RustDenominatorKeyV1 {
    Declaration {
        range: RustOracleRangeV1,
    },
    Containment {
        parent: RustOracleRangeV1,
        child: RustOracleRangeV1,
    },
    DirectCall {
        range: RustOracleRangeV1,
    },
    Write {
        range: RustOracleRangeV1,
    },
    TestMarker {
        range: RustOracleRangeV1,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum RustDenominatorError {
    WrongLength { expected: usize, actual: usize },
    WrongHash { actual: String },
    InvalidUtf8,
    Parse { message: String },
    InvalidRange,
    Duplicate { key: RustDenominatorKeyV1 },
    OutOfTable { key: RustDenominatorKeyV1 },
    UnexpectedConstruct { kind: &'static str },
    Missing { key: RustDenominatorKeyV1 },
}

const EXPECTED: [RustDenominatorKeyV1; 5] = [
    RustDenominatorKeyV1::Declaration {
        range: RustOracleRangeV1::new(3, 5, 3, 18),
    },
    RustDenominatorKeyV1::Containment {
        parent: RustOracleRangeV1::new(2, 1, 10, 1),
        child: RustOracleRangeV1::new(5, 5, 9, 5),
    },
    RustDenominatorKeyV1::DirectCall {
        range: RustOracleRangeV1::new(8, 9, 8, 16),
    },
    RustDenominatorKeyV1::Write {
        range: RustOracleRangeV1::new(7, 9, 7, 17),
    },
    RustDenominatorKeyV1::TestMarker {
        range: RustOracleRangeV1::new(4, 5, 4, 11),
    },
];

#[derive(Default)]
struct Oracle {
    keys: BTreeSet<RustDenominatorKeyV1>,
    error: Option<RustDenominatorError>,
    module: Option<RustOracleRangeV1>,
    caller: bool,
}

impl Oracle {
    fn record(&mut self, result: Result<RustDenominatorKeyV1, RustDenominatorError>) {
        if self.error.is_some() {
            return;
        }
        match result {
            Ok(key) if !EXPECTED.contains(&key) => {
                self.error = Some(RustDenominatorError::OutOfTable { key });
            }
            Ok(key) if !self.keys.insert(key.clone()) => {
                self.error = Some(RustDenominatorError::Duplicate { key });
            }
            Ok(_) => {}
            Err(error) => self.error = Some(error),
        }
    }

    fn unexpected(&mut self, kind: &'static str) {
        if self.error.is_none() {
            self.error = Some(RustDenominatorError::UnexpectedConstruct { kind });
        }
    }
}

impl Visit<'_> for Oracle {
    fn visit_item_mod(&mut self, item: &syn::ItemMod) {
        if item.ident != "structural" || self.module.is_some() {
            self.unexpected("module");
            return;
        }
        let Some((_, items)) = &item.content else {
            self.unexpected("out-of-line module");
            return;
        };
        match RustOracleRangeV1::from_spans(item.mod_token.span, item.span()) {
            Ok(range) => self.module = Some(range),
            Err(error) => {
                self.error = Some(error);
                return;
            }
        }
        for item in items {
            if let syn::Item::Fn(function) = item {
                self.visit_item_fn(function);
            } else {
                self.unexpected("non-function module member");
            }
        }
    }

    fn visit_item_fn(&mut self, item: &syn::ItemFn) {
        let span = RustOracleRangeV1::from_spans(item.sig.fn_token.span, item.span());
        match item.sig.ident.to_string().as_str() {
            "callee" => self.record(span.map(|range| RustDenominatorKeyV1::Declaration { range })),
            "caller" => {
                self.record(span.and_then(|child| {
                    self.module
                        .map(|parent| RustDenominatorKeyV1::Containment { parent, child })
                        .ok_or(RustDenominatorError::UnexpectedConstruct {
                            kind: "root caller",
                        })
                }));
                for attribute in &item.attrs {
                    if attribute.path().is_ident("test") {
                        self.record(
                            RustOracleRangeV1::from_span(attribute.span())
                                .map(|range| RustDenominatorKeyV1::TestMarker { range }),
                        );
                    }
                }
                self.caller = true;
                visit::visit_block(self, &item.block);
                self.caller = false;
            }
            _ => self.unexpected("free function"),
        }
    }

    fn visit_expr_call(&mut self, call: &syn::ExprCall) {
        if !self.caller
            || !matches!(&*call.func, syn::Expr::Path(path) if path.path.is_ident("callee"))
        {
            self.unexpected("call");
            return;
        }
        self.record(
            RustOracleRangeV1::from_span(call.span())
                .map(|range| RustDenominatorKeyV1::DirectCall { range }),
        );
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_assign(&mut self, assign: &syn::ExprAssign) {
        if !self.caller
            || !matches!(&*assign.left, syn::Expr::Path(path) if path.path.is_ident("value"))
        {
            self.unexpected("assignment");
            return;
        }
        self.record(
            RustOracleRangeV1::from_span(assign.span())
                .map(|range| RustDenominatorKeyV1::Write { range }),
        );
        visit::visit_expr_assign(self, assign);
    }
}

pub(crate) fn rust_denominator(
    bytes: &[u8],
) -> Result<BTreeSet<RustDenominatorKeyV1>, RustDenominatorError> {
    if bytes.len() != LENGTH {
        return Err(RustDenominatorError::WrongLength {
            expected: LENGTH,
            actual: bytes.len(),
        });
    }
    let hash = ContentHash::sha256(bytes);
    if hash.as_str() != SHA256 {
        return Err(RustDenominatorError::WrongHash {
            actual: hash.as_str().to_owned(),
        });
    }
    let source = std::str::from_utf8(bytes).map_err(|_| RustDenominatorError::InvalidUtf8)?;
    let file = syn::parse_file(source).map_err(|error| RustDenominatorError::Parse {
        message: error.to_string(),
    })?;
    let mut oracle = Oracle::default();
    for item in &file.items {
        if let syn::Item::Mod(module) = item {
            oracle.visit_item_mod(module);
        } else {
            oracle.unexpected("root item");
        }
    }
    if let Some(error) = oracle.error {
        return Err(error);
    }
    for key in EXPECTED {
        if !oracle.keys.contains(&key) {
            return Err(RustDenominatorError::Missing { key });
        }
    }
    Ok(oracle.keys)
}
