//! Source review v6: one language-neutral, change-driven obligation route
//! for Rust, TypeScript and Kotlin (ADR 0053).
//!
//! The route reads two committed snapshots of the invocation repository
//! through the hygienic read-only Git policy, extracts callables (free
//! functions *and* methods) and call sites per language, maps the pinned
//! zero-context diff onto callable ranges, resolves call sites against the
//! target snapshot, and enumerates deferred review obligations:
//!
//! - `node.changed_public_callable_contract@1`: a public callable that was
//!   added or modified between base and target;
//! - `relation.changed_callee_caller@1`: every caller of a changed callable,
//!   each edge labelled with how it was resolved (exact or name-only);
//! - `node.removed_public_callable@1`: a public callable present in base and
//!   absent from target, with the target call sites that still name it;
//! - `capability_gap.unanalyzed_source@1`: a tracked source file of the
//!   requested language that could not be analyzed, so caller enumeration is
//!   known to be incomplete.
//!
//! Nothing here is an authority: every obligation is `deferred`, and
//! `trusted_pass` is always `false`. Existing v1–v5 routes, rule ids,
//! registries and bytes are untouched.

mod analyze;
mod kotlin;
mod rust_lang;
mod typescript_lang;

use crate::IngestError;
use crate::git::source_review_v6 as git;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// Request schema id accepted by this route.
pub const SOURCE_REVIEW_REQUEST_V6_SCHEMA: &str = "reviewgraphen.source_review_request.v6";
/// Run schema id emitted by this route.
pub const SOURCE_REVIEW_RUN_V1_SCHEMA: &str = "reviewgraphen.source_review_run.v1";
/// File name of the run inside the artifact root.
pub const SOURCE_REVIEW_RUN_V1_FILE: &str = "source-review.run.v1.json";

pub(crate) const RULE_CHANGED_PUBLIC_CALLABLE: &str = "node.changed_public_callable_contract@1";
pub(crate) const RULE_CHANGED_CALLEE_CALLER: &str = "relation.changed_callee_caller@1";
pub(crate) const RULE_REMOVED_PUBLIC_CALLABLE: &str = "node.removed_public_callable@1";
pub(crate) const RULE_UNANALYZED_SOURCE: &str = "capability_gap.unanalyzed_source@1";

/// Languages this route analyzes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    Typescript,
    Kotlin,
}

impl Language {
    fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Typescript => "typescript",
            Self::Kotlin => "kotlin",
        }
    }

    fn accepts(self, path: &str) -> bool {
        match self {
            Self::Rust => path.ends_with(".rs"),
            Self::Typescript => {
                (path.ends_with(".ts")
                    || path.ends_with(".tsx")
                    || path.ends_with(".mts")
                    || path.ends_with(".cts"))
                    && !path.ends_with(".d.ts")
                    && !path.ends_with(".d.mts")
                    && !path.ends_with(".d.cts")
            }
            Self::Kotlin => path.ends_with(".kt"),
        }
    }

    fn pathspecs(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["*.rs"],
            Self::Typescript => &["*.ts", "*.tsx", "*.mts", "*.cts"],
            Self::Kotlin => &["*.kt"],
        }
    }

    fn limitations(self) -> Vec<&'static str> {
        let mut common = vec![
            "method_calls_resolved_by_name_without_type_inference",
            "calls_through_function_values_or_callbacks_not_tracked",
            "reflection_and_generated_code_not_analyzed",
            "only_tracked_files_of_the_requested_language_are_analyzed",
        ];
        match self {
            Self::Rust => common.extend([
                "macro_bodies_analyzed_only_when_arguments_parse_as_expressions",
                "trait_object_and_generic_dispatch_resolved_by_method_name",
            ]),
            Self::Typescript => common.extend([
                "package_imports_and_path_aliases_not_resolved",
                "interface_dispatch_resolved_by_method_name",
            ]),
            Self::Kotlin => common.extend([
                "overloads_distinguished_by_declaration_order_only",
                "interface_and_extension_dispatch_resolved_by_name",
            ]),
        }
        common.push("non_callable_declarations_constants_types_fields_not_enumerated");
        common
    }
}

/// Bounds on what one run reads.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_source_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_files: 20_000,
            max_file_bytes: 2 * 1024 * 1024,
            max_total_source_bytes: 512 * 1024 * 1024,
        }
    }
}

/// A v6 request. Unknown fields are refused.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceReviewRequest {
    pub schema: String,
    pub language: Language,
    pub base_revision: String,
    pub target_revision: String,
    #[serde(default)]
    pub limits: Option<Limits>,
}

/// Error of the v6 route: either the request is refused, or the snapshot
/// could not be read.
#[derive(Debug, thiserror::Error)]
pub enum SourceReviewError {
    #[error("invalid source review v6 request: {0}")]
    Request(String),
    #[error(transparent)]
    Ingest(#[from] IngestError),
}

impl SourceReviewError {
    pub fn is_request_refusal(&self) -> bool {
        matches!(self, Self::Request(_))
    }
}

/// Parses and validates request bytes.
pub fn parse_request(bytes: &[u8]) -> Result<SourceReviewRequest, SourceReviewError> {
    let request: SourceReviewRequest = serde_json::from_slice(bytes)
        .map_err(|error| SourceReviewError::Request(error.to_string()))?;
    if request.schema != SOURCE_REVIEW_REQUEST_V6_SCHEMA {
        return Err(SourceReviewError::Request("unexpected schema".to_owned()));
    }
    for revision in [&request.base_revision, &request.target_revision] {
        if revision.is_empty() || revision.starts_with('-') || revision.contains('\0') {
            return Err(SourceReviewError::Request(
                "revision must be a non-empty revision name".to_owned(),
            ));
        }
    }
    let limits = request.limits.unwrap_or_default();
    if limits.max_files == 0 || limits.max_file_bytes == 0 || limits.max_total_source_bytes == 0 {
        return Err(SourceReviewError::Request(
            "limits must be positive".to_owned(),
        ));
    }
    Ok(request)
}

// ---------------------------------------------------------------- model

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallableKind {
    Function,
    Method,
    /// Synthetic caller for code outside any function body: module-level
    /// initializers, class field initializers, static/`init` blocks,
    /// secondary constructors, property accessors. Never a resolution
    /// target and never a contract subject.
    Initializer,
}

impl CallableKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Initializer => "initializer",
        }
    }
}

/// Name of the synthetic initializer callable.
pub(crate) const INITIALIZER_NAME: &str = "<init>";
/// Local-name marker: a glob import inside the callable body, so any plain
/// name may be bound locally.
pub(crate) const LOCAL_GLOB_MARKER: &str = "*";

/// One declared callable in one file.
#[derive(Clone, Debug)]
pub(crate) struct Callable {
    pub name: String,
    /// Owning type (impl/class/object/trait/interface, or an extension
    /// receiver type); `None` for free functions.
    pub owner: Option<String>,
    pub kind: CallableKind,
    pub public: bool,
    pub start_line: u32,
    pub end_line: u32,
    /// Whitespace-normalized declaration head (everything before the body).
    pub signature: String,
    /// Whitespace-normalized parameter list; distinguishes overloads.
    pub params: String,
    /// Resolution scope: Rust module path, Kotlin package, TypeScript file.
    pub scope: String,
    /// Kotlin companion members: `owner` names the enclosing class for
    /// `Owner.f()` calls, but there is no implicit instance receiver.
    pub detached_receiver: bool,
}

impl Callable {
    fn qualified(&self) -> String {
        match &self.owner {
            Some(owner) => format!("{owner}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CallForm {
    /// `f(..)`
    Plain,
    /// `a::b::f(..)`, `Type::f(..)`, `Obj.f(..)`, `ns.f(..)`: qualifier
    /// segments without the final name.
    Qualified(Vec<String>),
    /// `recv.m(..)`; `self_receiver` for `self`/`this`.
    Method { self_receiver: bool },
}

/// One call site, attributed to the innermost enclosing callable of its file.
#[derive(Clone, Debug)]
pub(crate) struct RawCall {
    pub caller: usize,
    pub line: u32,
    pub name: String,
    pub form: CallForm,
    /// Kotlin: the call sits inside a lambda literal, whose implicit
    /// receiver is unknown.
    pub in_lambda: bool,
}

/// A name brought into scope by an import/use declaration.
#[derive(Clone, Debug)]
pub(crate) enum Import {
    /// Rust `use a::b::c as d` / Kotlin `import a.b.c as d`: local name to a
    /// qualified path (segments, last one is the imported item).
    Path {
        local: String,
        path: Vec<String>,
        /// Module/package the declaration is in; it binds only there.
        scope: String,
    },
    /// TypeScript `import { x as y } from "./m"`.
    Named {
        local: String,
        module: String,
        imported: String,
    },
    /// TypeScript `import * as ns from "./m"`.
    Namespace { local: String, module: String },
    /// Rust `use a::b::*` / Kotlin `import a.b.*`: every item of a module.
    Glob { path: Vec<String>, scope: String },
}

/// TypeScript `export { a as b } from "./m"` / `export * from "./m"`
/// (`exported == "*"`).
#[derive(Clone, Debug)]
pub(crate) struct Reexport {
    pub exported: String,
    pub module: String,
    pub imported: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FileFacts {
    pub callables: Vec<Callable>,
    pub calls: Vec<RawCall>,
    pub imports: Vec<Import>,
    pub reexports: Vec<Reexport>,
    /// Per callable: names bound locally (parameters, local variables,
    /// nested functions). A plain call to such a name is local and is not
    /// resolved against module-level declarations.
    pub locals: Vec<std::collections::BTreeSet<String>>,
    /// TypeScript: `(exported, local)` for `export { local as exported }`
    /// and `export default local;` (`exported == "default"`).
    pub export_aliases: Vec<(String, String)>,
    /// TypeScript: the file has no top-level import/export (a global
    /// script whose namespaces may merge with other files).
    pub script_mode: bool,
    /// TypeScript: name of the function or class declared by
    /// `export default function name` / `export default class Name`.
    pub default_export: Option<String>,
    /// Lines of ERROR/MISSING syntax nodes. Facts outside them are kept;
    /// the file is still reported as only partially analyzed.
    pub syntax_error_lines: Vec<u32>,
}

/// Lines of every ERROR/MISSING node of a tree-sitter tree.
pub(crate) fn tree_error_lines(root: tree_sitter::Node<'_>) -> Vec<u32> {
    let mut lines = std::collections::BTreeSet::new();
    if root.has_error() {
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            if n.is_error() || n.is_missing() {
                lines.insert(n.start_position().row as u32 + 1);
                continue;
            }
            if !n.has_error() {
                continue;
            }
            for i in 0..n.child_count() {
                if let Some(c) = n.child(i) {
                    stack.push(c);
                }
            }
        }
    }
    lines.into_iter().collect()
}

/// Per-language extraction result.
pub(crate) type Extracted = Result<FileFacts, String>;

fn extract(
    language: Language,
    path: &str,
    source: &str,
    tracked: &BTreeMap<String, TreeEntry>,
) -> Extracted {
    match language {
        Language::Rust => rust_lang::extract(path, source, |p| tracked.contains_key(p)),
        Language::Typescript => typescript_lang::extract(path, source),
        Language::Kotlin => kotlin::extract(path, source),
    }
}

pub(crate) fn normalize_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------- git

#[derive(Clone, Debug)]
struct TreeEntry {
    oid: String,
    size: u64,
}

fn list_blobs(root: &Path, commit: &str) -> Result<BTreeMap<String, TreeEntry>, IngestError> {
    let raw = git::list_tree(root, commit)?;
    let mut out = BTreeMap::new();
    for record in raw.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let text = String::from_utf8_lossy(record);
        let Some((head, path)) = text.split_once('\t') else {
            continue;
        };
        let fields: Vec<&str> = head.split_whitespace().collect();
        // mode type oid size; regular blobs only (no symlinks, submodules).
        if fields.len() != 4 || fields[1] != "blob" || !fields[0].starts_with("100") {
            continue;
        }
        let size = fields[3].parse().unwrap_or(u64::MAX);
        out.insert(
            path.to_owned(),
            TreeEntry {
                oid: fields[2].to_owned(),
                size,
            },
        );
    }
    Ok(out)
}

/// Target-side changes of one file from a zero-context patch.
#[derive(Clone, Debug, Default)]
pub(crate) struct FileDiff {
    pub added_file: bool,
    pub deleted_file: bool,
    /// Inclusive target-side line ranges that were added or replaced.
    pub ranges: Vec<(u32, u32)>,
    /// Pure deletions: lines were removed between target line `n` and `n+1`.
    pub deletion_points: Vec<u32>,
}

impl FileDiff {
    /// Whether a target-side declaration spanning `start..=end` was touched.
    pub(crate) fn touches(&self, start: u32, end: u32) -> bool {
        self.ranges.iter().any(|&(a, b)| a <= end && start <= b)
            || self.deletion_points.iter().any(|&n| start <= n && n < end)
    }
}

/// Decodes a patch header path (`a/x`, `"b/\303\251 x"`, `b/a b<TAB>`),
/// without its `a/`/`b/` prefix; `None` for `/dev/null`.
fn header_path(raw: &str) -> Option<String> {
    if raw == "/dev/null" {
        return None;
    }
    let decoded = if let Some(inner) = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        let mut bytes = Vec::new();
        let mut chars = inner.bytes().peekable();
        while let Some(c) = chars.next() {
            if c != b'\\' {
                bytes.push(c);
                continue;
            }
            match chars.next() {
                Some(b'n') => bytes.push(b'\n'),
                Some(b't') => bytes.push(b'\t'),
                Some(b'"') => bytes.push(b'"'),
                Some(b'\\') => bytes.push(b'\\'),
                Some(d @ b'0'..=b'7') => {
                    let mut v = u32::from(d - b'0');
                    for _ in 0..2 {
                        if let Some(&n @ b'0'..=b'7') = chars.peek() {
                            v = v * 8 + u32::from(n - b'0');
                            chars.next();
                        }
                    }
                    bytes.push(v as u8);
                }
                Some(other) => bytes.push(other),
                None => {}
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        // Git appends a TAB after a header path that contains a space.
        raw.strip_suffix('\t').unwrap_or(raw).to_owned()
    };
    decoded
        .strip_prefix("a/")
        .or_else(|| decoded.strip_prefix("b/"))
        .map(str::to_owned)
}

/// Parses a zero-context patch. Hunk bodies are consumed by their declared
/// line counts, so content lines are never mistaken for headers.
fn parse_diff(raw: &[u8]) -> BTreeMap<String, FileDiff> {
    let mut out: BTreeMap<String, FileDiff> = BTreeMap::new();
    let text = String::from_utf8_lossy(raw);
    let mut current: Option<String> = None;
    let mut minus_path: Option<String> = None;
    let mut pending_new = false;
    let (mut old_left, mut new_left) = (0u32, 0u32);
    for line in text.split('\n') {
        if old_left > 0 || new_left > 0 {
            if line.starts_with('-') && old_left > 0 {
                old_left -= 1;
                continue;
            }
            if line.starts_with('+') && new_left > 0 {
                new_left -= 1;
                continue;
            }
            if line.starts_with('\\') {
                continue;
            }
            old_left = 0;
            new_left = 0;
        }
        if line.starts_with("diff --git ") {
            current = None;
            minus_path = None;
            pending_new = false;
        } else if line.starts_with("new file mode") {
            pending_new = true;
        } else if let Some(raw) = line.strip_prefix("--- ") {
            minus_path = header_path(raw);
        } else if let Some(raw) = line.strip_prefix("+++ ") {
            match header_path(raw) {
                Some(path) => {
                    out.entry(path.clone()).or_default().added_file = pending_new;
                    current = Some(path);
                }
                None => {
                    if let Some(path) = minus_path.clone() {
                        out.entry(path).or_default().deleted_file = true;
                    }
                    current = None;
                }
            }
        } else if let Some(rest) = line.strip_prefix("@@ ") {
            let mut specs = rest.split_whitespace();
            let (Some(minus), Some(plus)) = (specs.next(), specs.next()) else {
                continue;
            };
            let count = |spec: &str| -> (u32, u32) {
                match spec.split_once(',') {
                    Some((s, c)) => (s.parse().unwrap_or(0), c.parse().unwrap_or(0)),
                    None => (spec.parse().unwrap_or(0), 1),
                }
            };
            let (_, old_count) = count(minus.trim_start_matches('-'));
            let (start, new_count) = count(plus.trim_start_matches('+'));
            old_left = old_count;
            new_left = new_count;
            let Some(path) = current.clone() else {
                continue;
            };
            let entry = out.entry(path).or_default();
            if new_count == 0 {
                entry.deletion_points.push(start);
            } else {
                entry.ranges.push((start, start + new_count - 1));
            }
        }
    }
    out
}

/// Parses `diff --name-status -z` (no renames): `status NUL path NUL`.
fn parse_name_status(raw: &[u8]) -> BTreeMap<String, char> {
    let mut out = BTreeMap::new();
    let mut fields = raw.split(|b| *b == 0).filter(|f| !f.is_empty());
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        let status = status.first().copied().map_or('?', char::from);
        out.insert(String::from_utf8_lossy(path).into_owned(), status);
    }
    out
}

// ---------------------------------------------------------------- run

/// One analyzed snapshot file.
pub(crate) struct SnapshotFile {
    pub path: String,
    pub facts: FileFacts,
}

/// Why a tracked file of the requested language was not analyzed.
#[derive(Clone, Debug)]
pub(crate) struct Unanalyzed {
    pub path: String,
    pub reason: String,
    /// For `partial_syntax_error`: the lines whose facts may be missing.
    pub lines: Vec<u32>,
}

struct Snapshot {
    files: Vec<SnapshotFile>,
    unanalyzed: Vec<Unanalyzed>,
    tracked_files: usize,
    read_bytes: u64,
}

fn read_snapshot(
    root: &Path,
    language: Language,
    blobs: &BTreeMap<String, TreeEntry>,
    only: Option<&[String]>,
    limits: Limits,
) -> Result<Snapshot, SourceReviewError> {
    let mut files = Vec::new();
    let mut unanalyzed = Vec::new();
    let mut tracked = 0usize;
    let mut total = 0u64;
    for (path, entry) in blobs {
        if !language.accepts(path) {
            continue;
        }
        if let Some(only) = only
            && !only.iter().any(|p| p == path)
        {
            continue;
        }
        tracked += 1;
        let bound = if tracked > limits.max_files {
            Some("max_files_exceeded")
        } else if entry.size > limits.max_file_bytes {
            Some("max_file_bytes_exceeded")
        } else if total.saturating_add(entry.size) > limits.max_total_source_bytes {
            Some("max_total_source_bytes_exceeded")
        } else {
            None
        };
        if let Some(reason) = bound {
            unanalyzed.push(Unanalyzed {
                path: path.clone(),
                reason: reason.to_owned(),
                lines: Vec::new(),
            });
            continue;
        }
        let bytes = git::blob(root, &entry.oid)?;
        total += bytes.len() as u64;
        let Ok(source) = String::from_utf8(bytes) else {
            unanalyzed.push(Unanalyzed {
                path: path.clone(),
                reason: "not_utf8".to_owned(),
                lines: Vec::new(),
            });
            continue;
        };
        if let Some(reason) = too_deep(&source) {
            unanalyzed.push(Unanalyzed {
                path: path.clone(),
                reason: reason.to_owned(),
                lines: Vec::new(),
            });
            continue;
        }
        match extract(language, path, &source, blobs) {
            Ok(facts) => {
                if !facts.syntax_error_lines.is_empty() {
                    unanalyzed.push(Unanalyzed {
                        path: path.clone(),
                        reason: "partial_syntax_error".to_owned(),
                        lines: facts.syntax_error_lines.clone(),
                    });
                }
                files.push(SnapshotFile {
                    path: path.clone(),
                    facts,
                });
            }
            Err(reason) => unanalyzed.push(Unanalyzed {
                path: path.clone(),
                reason,
                lines: Vec::new(),
            }),
        }
    }
    Ok(Snapshot {
        files,
        unanalyzed,
        tracked_files: tracked,
        read_bytes: total,
    })
}

/// Parsers and walkers recurse per syntactic nesting level. The route runs
/// on a 1 GiB stack, which covers long operator chains in any file within
/// `max_file_bytes`; bracket nesting beyond this bound (far above
/// hand-written code) is reported unanalyzed instead of risking the stack.
fn too_deep(source: &str) -> Option<&'static str> {
    const MAX_NESTING: usize = 20_000;
    let (mut depth, mut max_depth) = (0usize, 0usize);
    for b in source.bytes() {
        match b {
            b'(' | b'[' | b'{' => {
                depth += 1;
                max_depth = max_depth.max(depth);
            }
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    (max_depth > MAX_NESTING).then_some("nesting_too_deep")
}

/// Runs the v6 route in `root` (the invocation directory, which must be the
/// repository top level) and returns canonical run bytes. The work runs on
/// a thread with a large stack: syntax trees are walked recursively.
pub fn review(root: &Path, request_bytes: &[u8]) -> Result<Vec<u8>, SourceReviewError> {
    let root = root.to_path_buf();
    let request_bytes = request_bytes.to_vec();
    std::thread::Builder::new()
        .name("source-review-v6".to_owned())
        .stack_size(1 << 30)
        .spawn(move || review_inner(&root, &request_bytes))
        .map_err(|e| SourceReviewError::Ingest(IngestError::AdapterOutput(e.to_string())))?
        .join()
        .map_err(|_| {
            SourceReviewError::Ingest(IngestError::AdapterOutput(
                "analysis thread panicked".to_owned(),
            ))
        })?
}

fn review_inner(root: &Path, request_bytes: &[u8]) -> Result<Vec<u8>, SourceReviewError> {
    let request = parse_request(request_bytes)?;
    let limits = request.limits.unwrap_or_default();
    let top = git::toplevel(root)?;
    let top = String::from_utf8_lossy(&top).trim().to_owned();
    let canonical_root = std::fs::canonicalize(root)
        .map_err(|_| SourceReviewError::Request("invocation root is not accessible".to_owned()))?;
    let canonical_top = std::fs::canonicalize(&top)
        .map_err(|_| SourceReviewError::Request("repository root is not accessible".to_owned()))?;
    if canonical_root != canonical_top {
        // The repository's own layout (a subdirectory, or a configured
        // `core.worktree`) is not a malformed request: fail closed like the
        // other routes' repository-root mismatch.
        return Err(IngestError::RepositoryRootMismatch {
            requested: canonical_root,
            actual: canonical_top,
        }
        .into());
    }
    let base = git::resolve_commit(root, &request.base_revision)
        .map_err(|_| SourceReviewError::Request("base_revision does not resolve".to_owned()))?;
    let target = git::resolve_commit(root, &request.target_revision)
        .map_err(|_| SourceReviewError::Request("target_revision does not resolve".to_owned()))?;
    let base_blobs = list_blobs(root, &base)?;
    let target_blobs = list_blobs(root, &target)?;

    let pathspecs = request.language.pathspecs();
    let mut diff: BTreeMap<String, FileDiff> =
        parse_diff(&git::diff_u0(root, &base, &target, pathspecs)?)
            .into_iter()
            .filter(|(path, _)| request.language.accepts(path))
            .collect();
    // Cross-check the patch against the authoritative NUL-separated list:
    // a changed path the patch parser did not see is a gap, never a silent
    // omission.
    let mut diff_gaps = Vec::new();
    for (path, status) in
        parse_name_status(&git::diff_name_status(root, &base, &target, pathspecs)?)
    {
        if !request.language.accepts(&path) {
            continue;
        }
        if status == 'T' {
            // Regular file <-> symlink/submodule: one side is not source.
            diff.remove(&path);
            diff_gaps.push(Unanalyzed {
                path,
                reason: "file_type_changed".to_owned(),
                lines: Vec::new(),
            });
            continue;
        }
        match diff.get_mut(&path) {
            Some(d) => {
                d.added_file |= status == 'A';
                d.deleted_file |= status == 'D';
            }
            None => {
                // A mode-only change has identical content on both sides.
                let same = matches!(
                    (base_blobs.get(&path), target_blobs.get(&path)),
                    (Some(b), Some(t)) if b.oid == t.oid
                );
                if !same {
                    diff_gaps.push(Unanalyzed {
                        path,
                        reason: "diff_unparsed".to_owned(),
                        lines: Vec::new(),
                    });
                }
            }
        }
    }
    let target_snapshot = read_snapshot(root, request.language, &target_blobs, None, limits)?;
    let base_paths: Vec<String> = diff
        .iter()
        .filter(|(_, d)| !d.added_file)
        .map(|(p, _)| p.clone())
        .collect();
    let base_snapshot = read_snapshot(
        root,
        request.language,
        &base_blobs,
        Some(&base_paths),
        limits,
    )?;

    let target_unanalyzed: std::collections::BTreeSet<String> = target_snapshot
        .unanalyzed
        .iter()
        .chain(&diff_gaps)
        .map(|u| u.path.clone())
        .collect();
    let analysis = analyze::analyze(
        request.language,
        &target_snapshot.files,
        &base_snapshot.files,
        &diff,
        &target_unanalyzed,
    );
    let mut unanalyzed = target_snapshot.unanalyzed.clone();
    unanalyzed.extend(diff_gaps);
    for u in &base_snapshot.unanalyzed {
        unanalyzed.push(Unanalyzed {
            path: u.path.clone(),
            reason: format!("base:{}", u.reason),
            lines: u.lines.clone(),
        });
    }
    let run = analyze::render(
        &request,
        &base,
        &target,
        &analysis,
        &unanalyzed,
        json!({
            "tracked_files": target_snapshot.tracked_files,
            "analyzed_files": target_snapshot.files.len(),
            "unanalyzed_files": target_snapshot.unanalyzed.len(),
            "read_bytes": target_snapshot.read_bytes,
            "base_files_analyzed": base_snapshot.files.len(),
        }),
    );
    let mut bytes =
        serde_json::to_vec(&run).map_err(|e| IngestError::AdapterOutput(e.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(crate) fn obligation_id(rule_id: &str, target_key: &Value) -> String {
    let preimage = json!({"rule_id": rule_id, "target_key": target_key});
    let bytes = serde_json::to_vec(&preimage).unwrap_or_default();
    format!("obligation:sha256:{}", sha256_hex(&bytes))
}

#[cfg(test)]
mod tests;
