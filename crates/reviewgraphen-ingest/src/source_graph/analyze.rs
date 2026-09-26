//! Change mapping, call resolution and obligation enumeration for the
//! source review v6 route.
//!
//! Labelling rule: `exact` is claimed only when the language's own scoping
//! rules pin exactly one declaration in this snapshot. Anything decided by
//! a bare name (method names without receiver types, type names without a
//! located declaration, cross-crate paths) is `name_only`; more than one
//! candidate is `ambiguous`.

use super::{
    CallForm, Callable, CallableKind, FileDiff, Import, Language, RULE_CHANGED_CALLEE_CALLER,
    RULE_CHANGED_PUBLIC_CALLABLE, RULE_REMOVED_PUBLIC_CALLABLE, RULE_UNANALYZED_SOURCE,
    SOURCE_REVIEW_RUN_V1_SCHEMA, SnapshotFile, SourceReviewRequest, Unanalyzed, obligation_id,
    sha256_hex,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Ambiguous candidate sets larger than this are not linked as caller
/// edges; they are counted per changed callable instead.
const MAX_AMBIGUOUS_FANOUT: usize = 5;
/// Re-export chains followed at most this deep.
const MAX_REEXPORT_DEPTH: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Confidence {
    Exact,
    NameOnly,
    Ambiguous,
}

impl Confidence {
    fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::NameOnly => "name_only",
            Self::Ambiguous => "ambiguous",
        }
    }
}

/// How one call site resolved.
#[derive(Clone, Debug)]
struct Resolution {
    targets: Vec<usize>,
    kind: &'static str,
    confidence: Confidence,
}

impl Resolution {
    fn weaken(mut self, kind: &'static str) -> Self {
        if self.confidence == Confidence::Exact {
            self.confidence = Confidence::NameOnly;
            self.kind = kind;
        }
        self
    }
}

/// A callable of the target snapshot with its global identity.
struct Node<'a> {
    file: usize,
    path: &'a str,
    callable: &'a Callable,
    key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChangeStatus {
    Unchanged,
    Added,
    Modified {
        signature_changed: bool,
        was_public: bool,
    },
    /// Touched by the diff, but the base side of its file could not be
    /// analyzed: added-vs-modified, signature and visibility are unknown.
    ChangedBaseUnknown,
}

/// One caller edge into a changed callable.
#[derive(Clone, Debug)]
struct Edge {
    caller: usize,
    lines: BTreeSet<u32>,
    kinds: BTreeSet<&'static str>,
    confidence: Confidence,
    candidate_count: usize,
}

pub(crate) struct Analysis {
    changed: Vec<ChangedCallable>,
    removed: Vec<RemovedCallable>,
    changed_files: Vec<Value>,
    call_stats: BTreeMap<&'static str, usize>,
}

struct ChangedCallable {
    node: NodeInfo,
    status: ChangeStatus,
    edges: Vec<(NodeInfo, Edge)>,
    over_fanout_sites: usize,
}

#[derive(Clone)]
struct NodeInfo {
    key: String,
    path: String,
    callable: Callable,
}

struct RemovedCallable {
    key: String,
    path: String,
    callable: Callable,
    naming_sites: Vec<(String, u32)>,
}

/// Stable per-file keys: `path#Owner.name`; when a qualified name is
/// declared more than once in a file (overloads), `path#Owner.name(params)`;
/// `~n` only if even the parameter lists coincide.
fn file_keys(path: &str, callables: &[Callable]) -> Vec<String> {
    let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
    for c in callables {
        *by_name.entry(c.qualified()).or_insert(0) += 1;
    }
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    callables
        .iter()
        .map(|c| {
            let q = c.qualified();
            let base = if by_name[&q] > 1 {
                format!("{path}#{q}({})", c.params)
            } else {
                format!("{path}#{q}")
            };
            let n = seen.entry(base.clone()).or_insert(0);
            *n += 1;
            if *n == 1 { base } else { format!("{base}~{n}") }
        })
        .collect()
}

/// Pairs target declarations with base declarations of the same file:
/// within one qualified name, a lone declaration on both sides is the same
/// callable; otherwise declarations pair by identical parameter lists.
/// Returns, per target callable, its base counterpart, and per base
/// callable whether anything matched it.
fn match_declarations(base: &[Callable], target: &[Callable]) -> (Vec<Option<usize>>, Vec<bool>) {
    let group = |cs: &[Callable]| {
        let mut g: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, c) in cs.iter().enumerate() {
            if c.kind != CallableKind::Initializer {
                g.entry(c.qualified()).or_default().push(i);
            }
        }
        g
    };
    let (bg, tg) = (group(base), group(target));
    let mut to_base = vec![None; target.len()];
    let mut base_matched = vec![false; base.len()];
    for (name, ts) in &tg {
        let Some(bs) = bg.get(name) else { continue };
        if bs.len() == 1 && ts.len() == 1 {
            to_base[ts[0]] = Some(bs[0]);
            base_matched[bs[0]] = true;
            continue;
        }
        for &t in ts {
            if let Some(&b) = bs
                .iter()
                .find(|&&b| !base_matched[b] && base[b].params == target[t].params)
            {
                to_base[t] = Some(b);
                base_matched[b] = true;
            }
        }
    }
    (to_base, base_matched)
}

struct Index<'a> {
    nodes: Vec<Node<'a>>,
    /// First node index per file.
    file_base: Vec<usize>,
    free_by_name: BTreeMap<&'a str, Vec<usize>>,
    methods_by_name: BTreeMap<&'a str, Vec<usize>>,
    paths: BTreeMap<&'a str, usize>,
}

impl<'a> Index<'a> {
    fn new(files: &'a [SnapshotFile]) -> Self {
        let mut nodes = Vec::new();
        let mut file_base = Vec::new();
        let mut free_by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut methods_by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut paths = BTreeMap::new();
        for (fi, file) in files.iter().enumerate() {
            file_base.push(nodes.len());
            paths.insert(file.path.as_str(), fi);
            let keys = file_keys(&file.path, &file.facts.callables);
            for (callable, key) in file.facts.callables.iter().zip(keys) {
                let id = nodes.len();
                match callable.kind {
                    CallableKind::Function => {
                        free_by_name.entry(&callable.name).or_default().push(id)
                    }
                    CallableKind::Method => {
                        methods_by_name.entry(&callable.name).or_default().push(id)
                    }
                    CallableKind::Initializer => {}
                }
                nodes.push(Node {
                    file: fi,
                    path: &file.path,
                    callable,
                    key,
                });
            }
        }
        Self {
            nodes,
            file_base,
            free_by_name,
            methods_by_name,
            paths,
        }
    }

    fn free(&self, name: &str) -> &[usize] {
        self.free_by_name.get(name).map_or(&[], Vec::as_slice)
    }

    fn methods(&self, name: &str) -> &[usize] {
        self.methods_by_name.get(name).map_or(&[], Vec::as_slice)
    }

    fn info(&self, id: usize) -> NodeInfo {
        let n = &self.nodes[id];
        NodeInfo {
            key: n.key.clone(),
            path: n.path.to_owned(),
            callable: n.callable.clone(),
        }
    }
}

/// Scope rules pinned the candidates: one is `exact`; several (e.g. an
/// overload set) are honestly `ambiguous`.
fn scoped(targets: Vec<usize>, kind: &'static str) -> Option<Resolution> {
    match targets.len() {
        0 => None,
        1 => Some(Resolution {
            targets,
            kind,
            confidence: Confidence::Exact,
        }),
        _ => Some(Resolution {
            targets,
            kind,
            confidence: Confidence::Ambiguous,
        }),
    }
}

/// Only a name matched: one candidate is `name_only`, several `ambiguous`.
fn by_name(targets: &[usize], kind: &'static str) -> Option<Resolution> {
    match targets.len() {
        0 => None,
        1 => Some(Resolution {
            targets: targets.to_vec(),
            kind,
            confidence: Confidence::NameOnly,
        }),
        _ => Some(Resolution {
            targets: targets.to_vec(),
            kind,
            confidence: Confidence::Ambiguous,
        }),
    }
}

/// Rust scope strings are `root::mod::sub`, where `root` identifies the
/// crate (`crate` or `crate@<dir>`).
fn rust_root(scope: &str) -> &str {
    scope.split("::").next().unwrap_or(scope)
}

/// Joins a Rust path written in `scope` into an absolute module path.
/// Returns `None` when the path starts outside this crate (an external
/// crate or a name this route cannot place).
fn rust_join(scope: &str, rel: &[String]) -> Option<Vec<String>> {
    let mut base: Vec<String> = scope.split("::").map(str::to_owned).collect();
    let mut rest = rel;
    match rest.first().map(String::as_str) {
        Some("crate") => {
            base.truncate(1);
            rest = &rest[1..];
        }
        Some("self") => rest = &rest[1..],
        Some("super") => {
            while rest.first().map(String::as_str) == Some("super") {
                if base.len() > 1 {
                    base.pop();
                }
                rest = &rest[1..];
            }
        }
        _ => return None,
    }
    base.extend(rest.iter().cloned());
    Some(base)
}

fn is_type_like(segment: &str) -> bool {
    segment.chars().next().is_some_and(char::is_uppercase)
}

struct Resolver<'a, 'b> {
    language: Language,
    index: &'b Index<'a>,
    files: &'a [SnapshotFile],
}

impl Resolver<'_, '_> {
    fn methods_of(&self, owner: &str, name: &str) -> Vec<usize> {
        self.index
            .methods(name)
            .iter()
            .copied()
            .filter(|&m| self.index.nodes[m].callable.owner.as_deref() == Some(owner))
            .collect()
    }

    fn in_file(&self, ids: &[usize], file: usize) -> Vec<usize> {
        ids.iter()
            .copied()
            .filter(|&m| self.index.nodes[m].file == file)
            .collect()
    }

    fn in_scope(&self, ids: &[usize], scope: &str) -> Vec<usize> {
        ids.iter()
            .copied()
            .filter(|&m| self.index.nodes[m].callable.scope == scope)
            .collect()
    }

    fn free_in_scope(&self, scope: &str, name: &str) -> Vec<usize> {
        self.in_scope(self.index.free(name), scope)
    }

    fn resolve(&self, file: usize, caller: usize, call: &super::RawCall) -> Option<Resolution> {
        let name = call.name.as_str();
        let local = caller - self.index.file_base[file];
        if call.form == CallForm::Plain
            && self.files[file]
                .facts
                .locals
                .get(local)
                .is_some_and(|l| l.contains(name))
        {
            return None;
        }
        if let CallForm::Qualified(q) = &call.form {
            let head_is_local = q.first().is_some_and(|h| {
                self.files[file]
                    .facts
                    .locals
                    .get(local)
                    .is_some_and(|l| l.contains(h.as_str()))
            });
            if head_is_local {
                // `alias::f()` through a body-local `use`, or `value.m()` on
                // a parameter/variable that shadows an import.
                return match self.language {
                    Language::Rust => None,
                    _ => by_name(self.index.methods(name), "method_name"),
                };
            }
        }
        let resolution = match &call.form {
            CallForm::Method { self_receiver } => {
                self.resolve_method(file, caller, name, *self_receiver)
            }
            CallForm::Plain => self.resolve_plain(file, caller, name),
            CallForm::Qualified(qualifier) => self.resolve_qualified(file, caller, name, qualifier),
        }?;
        if call.form == CallForm::Plain {
            let caller_node = &self.index.nodes[caller];
            // Kotlin: inside a member, an extension or a lambda, an implicit
            // receiver (inherited members, library receivers such as
            // `StringBuilder` in `apply {}`) takes precedence over top-level
            // functions and cannot be seen here.
            if self.language == Language::Kotlin
                && (call.in_lambda
                    || (caller_node.callable.owner.is_some() && resolution.kind != "self_receiver"))
            {
                return Some(resolution.weaken("implicit_receiver_possible"));
            }
            // Rust: a glob `use` inside the body may bind any plain name.
            if self.files[file]
                .facts
                .locals
                .get(local)
                .is_some_and(|l| l.contains(super::LOCAL_GLOB_MARKER))
            {
                return Some(resolution.weaken("local_glob_use"));
            }
        }
        Some(resolution)
    }

    fn resolve_method(
        &self,
        file: usize,
        caller: usize,
        name: &str,
        self_receiver: bool,
    ) -> Option<Resolution> {
        if self_receiver
            && !self.index.nodes[caller].callable.detached_receiver
            && let Some(owner) = self.index.nodes[caller].callable.owner.as_deref()
            && let Some(r) = self.own_member(file, caller, owner, name)
        {
            return Some(r);
        }
        by_name(self.index.methods(name), "method_name")
    }

    /// `self.m()` / `this.m()` / `Self::m()`: a member of the caller's own
    /// type. Exact only when declared in the caller's file; otherwise
    /// name-only, since same-named types elsewhere cannot be told apart.
    fn own_member(
        &self,
        file: usize,
        caller: usize,
        owner: &str,
        name: &str,
    ) -> Option<Resolution> {
        let all = self.methods_of(owner, name);
        if all.is_empty() {
            return None;
        }
        if self.language == Language::Rust {
            // Rust method resolution depends on types and trait selection
            // (inherent impls in other modules win over trait impls; owners
            // are compared by their last path segment only): never exact.
            let root = rust_root(&self.index.nodes[caller].callable.scope);
            let same_crate: Vec<usize> = all
                .iter()
                .copied()
                .filter(|&m| rust_root(&self.index.nodes[m].callable.scope) == root)
                .collect();
            let pool = if same_crate.is_empty() {
                &all
            } else {
                &same_crate
            };
            return by_name(pool, "self_receiver_by_name");
        }
        let caller_scope = self.index.nodes[caller].callable.scope.as_str();
        let same_file = self.in_scope(&self.in_file(&all, file), caller_scope);
        if !same_file.is_empty() {
            return scoped(same_file, "self_receiver");
        }
        by_name(&all, "self_receiver_other_file")
    }

    fn resolve_plain(&self, file: usize, caller: usize, name: &str) -> Option<Resolution> {
        let caller_node = &self.index.nodes[caller];
        let scope = caller_node.callable.scope.as_str();
        let imports = &self.files[file].facts.imports;
        // Kotlin: members of the enclosing class (implicit `this`) first.
        if self.language == Language::Kotlin
            && !caller_node.callable.detached_receiver
            && let Some(owner) = caller_node.callable.owner.as_deref()
        {
            let own = self.in_file(&self.methods_of(owner, name), file);
            if let Some(r) = scoped(own, "self_receiver") {
                return Some(r);
            }
        }
        for import in imports {
            match import {
                Import::Path {
                    local,
                    path,
                    scope: bound,
                } if local == name && bound == scope => {
                    // An explicit import binds the name: it resolves to the
                    // imported item or to nothing this route can locate.
                    return self.resolve_path(scope, path, "import");
                }
                Import::Named {
                    local,
                    module,
                    imported,
                } if local == name => {
                    return self.ts_import(file, module, imported);
                }
                _ => {}
            }
        }
        if let Some(r) = scoped(self.free_in_scope(scope, name), "same_scope") {
            return Some(r);
        }
        // TypeScript namespaces nest lexically: `file#A.B`, `file#A`, `file`.
        if self.language == Language::Typescript {
            let mut outer = scope;
            while let Some(i) = outer.rfind(['.', '#']) {
                outer = &outer[..i];
                if let Some(r) = scoped(self.free_in_scope(outer, name), "same_scope") {
                    // In a script (no imports/exports), namespaces merge
                    // across files and their members shadow outer names.
                    if self.files[file].facts.script_mode {
                        return Some(r.weaken("script_namespace_outer"));
                    }
                    return Some(r);
                }
            }
        }
        for import in imports {
            if let Import::Glob { path, scope: bound } = import {
                if bound != scope {
                    continue;
                }
                let module = match self.language {
                    Language::Rust => match rust_join(scope, path) {
                        Some(m) => m.join("::"),
                        None => continue,
                    },
                    _ => path.join("."),
                };
                if let Some(r) = scoped(self.free_in_scope(&module, name), "import_glob") {
                    return Some(r);
                }
            }
        }
        match self.language {
            // Without an import, a bare name is local, global or builtin.
            Language::Typescript => None,
            Language::Rust => {
                // Glob-imported from elsewhere or generated: only same-crate
                // functions are (name-level) candidates.
                let root = rust_root(scope);
                let same_crate: Vec<usize> = self
                    .index
                    .free(name)
                    .iter()
                    .copied()
                    .filter(|&f| rust_root(&self.index.nodes[f].callable.scope) == root)
                    .collect();
                by_name(&same_crate, "name")
            }
            Language::Kotlin => by_name(self.index.free(name), "name"),
        }
    }

    /// TypeScript named import (`import { imported } from module`),
    /// following re-export barrels.
    fn ts_import(&self, file: usize, module: &str, imported: &str) -> Option<Resolution> {
        let Some(target_file) = self.ts_module_file(self.files[file].path.as_str(), module) else {
            // A package or path-alias import: the name is bound to something
            // this route cannot locate; an exported function of that name
            // elsewhere is only a name-level candidate.
            let exported: Vec<usize> = self
                .index
                .free(imported)
                .iter()
                .copied()
                .filter(|&f| self.index.nodes[f].callable.public)
                .collect();
            return by_name(&exported, "package_import_name");
        };
        scoped(
            self.ts_exported_function(target_file, imported, 0),
            "import",
        )
    }

    /// Free functions exported as `name` by `file`, through re-exports.
    fn ts_exported_function(&self, file: usize, name: &str, depth: usize) -> Vec<usize> {
        let name = self.ts_local_export_name(file, name);
        let direct: Vec<usize> = self
            .index
            .free(name)
            .iter()
            .copied()
            .filter(|&f| self.index.nodes[f].file == file && self.index.nodes[f].callable.public)
            .collect();
        if !direct.is_empty() || depth >= MAX_REEXPORT_DEPTH {
            return direct;
        }
        let from = self.files[file].path.as_str();
        let mut out = Vec::new();
        for re in &self.files[file].facts.reexports {
            let next = if re.exported == name {
                Some(re.imported.as_str())
            } else if re.exported == "*" {
                Some(name)
            } else {
                None
            };
            if let (Some(next), Some(next_file)) = (next, self.ts_module_file(from, &re.module)) {
                out.extend(self.ts_exported_function(next_file, next, depth + 1));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The file and local name of the class exported as `name` by `file`,
    /// following re-exports.
    /// The local declaration name behind an export name of `file`
    /// (`export default function f`, `export default f;`,
    /// `export { f as g }`).
    fn ts_local_export_name<'n>(&'n self, file: usize, name: &'n str) -> &'n str {
        let facts = &self.files[file].facts;
        if name == "default"
            && let Some(declared) = &facts.default_export
        {
            return declared;
        }
        facts
            .export_aliases
            .iter()
            .find(|(exported, _)| exported == name)
            .map_or(name, |(_, local)| local.as_str())
    }

    fn ts_class_file(&self, file: usize, name: &str, depth: usize) -> Option<(usize, String)> {
        let name = self.ts_local_export_name(file, name);
        let declares = self.index.nodes[self.index.file_base[file]..]
            .iter()
            .take_while(|n| n.file == file)
            .any(|n| n.callable.owner.as_deref() == Some(name));
        if declares {
            return Some((file, name.to_owned()));
        }
        if depth >= MAX_REEXPORT_DEPTH {
            return None;
        }
        let from = self.files[file].path.as_str();
        let mut found = Vec::new();
        for re in &self.files[file].facts.reexports {
            let next = if re.exported == name {
                re.imported.as_str()
            } else if re.exported == "*" {
                name
            } else {
                continue;
            };
            if let Some(next_file) = self.ts_module_file(from, &re.module)
                && let Some(hit) = self.ts_class_file(next_file, next, depth + 1)
            {
                found.push(hit);
            }
        }
        found.sort();
        found.dedup();
        (found.len() == 1).then(|| found.remove(0))
    }

    /// TypeScript type-qualified call `X.f()` / `new X()`: `X` must be
    /// declared in this file or imported from a relative module to be exact.
    fn ts_type_member(&self, file: usize, owner: &str, name: &str) -> Option<Resolution> {
        let all = self.methods_of(owner, name);
        let same_file = self.in_file(&all, file);
        if !same_file.is_empty() {
            return scoped(same_file, "qualified_type");
        }
        for import in &self.files[file].facts.imports {
            if let Import::Named {
                local,
                module,
                imported,
            } = import
                && local == owner
            {
                // Bound by an import: that module (through re-exports)
                // or nothing.
                let target_file = self.ts_module_file(self.files[file].path.as_str(), module)?;
                let (class_file, class_name) = self.ts_class_file(target_file, imported, 0)?;
                let declared = self.methods_of(&class_name, name);
                return scoped(self.in_file(&declared, class_file), "qualified_type");
            }
        }
        by_name(&all, "type_name")
    }

    fn resolve_qualified(
        &self,
        file: usize,
        caller: usize,
        name: &str,
        qualifier: &[String],
    ) -> Option<Resolution> {
        let caller_node = &self.index.nodes[caller];
        let scope = caller_node.callable.scope.as_str();
        let imports = &self.files[file].facts.imports;
        let first = qualifier.first()?;
        if first == "Self" && qualifier.len() == 1 {
            let owner = caller_node.callable.owner.as_deref()?;
            return self.own_member(file, caller, owner, name);
        }
        match self.language {
            Language::Typescript => {
                if qualifier.len() == 1 {
                    for import in imports {
                        if let Import::Namespace { local, module } = import
                            && local == first
                        {
                            let target_file =
                                self.ts_module_file(self.files[file].path.as_str(), module)?;
                            return scoped(
                                self.ts_exported_function(target_file, name, 0),
                                "import",
                            );
                        }
                    }
                }
                // `N.f()` / `N.M.f()` into a namespace declared in this file.
                let ns_scope = format!("{}#{}", self.files[file].path, qualifier.join("."));
                if let Some(r) = scoped(self.free_in_scope(&ns_scope, name), "qualified_path") {
                    return Some(r);
                }
                if qualifier.len() == 1 && is_type_like(first) {
                    return self.ts_type_member(file, first, name);
                }
                // `value.m()`: a method on an unknown receiver.
                by_name(self.index.methods(name), "method_name")
            }
            Language::Rust => {
                // Expand a leading `use` alias.
                let mut path: Vec<String> = qualifier.to_vec();
                for import in imports {
                    if let Import::Path {
                        local,
                        path: full,
                        scope: bound,
                    } = import
                        && local == first
                        && bound == scope
                    {
                        let mut expanded = full.clone();
                        expanded.extend(path.iter().skip(1).cloned());
                        path = expanded;
                        break;
                    }
                }
                path.push(name.to_owned());
                if let Some(r) = self.resolve_path(scope, &path, "qualified_path") {
                    return Some(r);
                }
                // `Type::f()` with `Type` not placed by any path: same-file
                // declaration is exact, same-crate is name-level.
                if path.len() == 2 && is_type_like(&path[0]) {
                    let root = rust_root(scope);
                    let hits: Vec<usize> = self
                        .methods_of(&path[0], name)
                        .into_iter()
                        .filter(|&m| rust_root(&self.index.nodes[m].callable.scope) == root)
                        .collect();
                    // Which `Type` and which impl (inherent or trait, generic
                    // parameters shadowing a struct) needs type resolution:
                    // same-crate candidates by name only.
                    return by_name(&hits, "type_name");
                }
                None
            }
            Language::Kotlin => {
                let mut path: Vec<String> = qualifier.to_vec();
                let mut imported = false;
                for import in imports {
                    if let Import::Path {
                        local,
                        path: full,
                        scope: bound,
                    } = import
                        && local == first
                        && bound == scope
                    {
                        let mut expanded = full.clone();
                        expanded.extend(path.iter().skip(1).cloned());
                        path = expanded;
                        imported = true;
                        break;
                    }
                }
                path.push(name.to_owned());
                if imported || path.len() > 2 {
                    if let Some(r) = self.resolve_path(scope, &path, "qualified_path") {
                        return Some(r);
                    }
                    if imported {
                        return None;
                    }
                }
                if qualifier.len() == 1 && is_type_like(first) {
                    // `X.f()`: `X` declared in the caller's package, else only
                    // a name-level candidate (star imports, other packages).
                    let all = self.methods_of(first, name);
                    let same_package = self.in_scope(&all, scope);
                    if !same_package.is_empty() {
                        return scoped(same_package, "qualified_type");
                    }
                    return by_name(&all, "type_name");
                }
                // `value.m()` on a lower-case receiver.
                by_name(self.index.methods(name), "method_name")
            }
        }
    }

    /// Resolves a fully written path (`crate::a::f`, `a.b.f`,
    /// `crate::a::Type::f`, `a.b.Type.f`).
    fn resolve_path(&self, scope: &str, path: &[String], kind: &'static str) -> Option<Resolution> {
        let (name, module) = path.split_last()?;
        match self.language {
            Language::Rust => {
                let Some(absolute) = rust_join(scope, module) else {
                    // Not `crate`/`self`/`super`-rooted: a child module of the
                    // current one, or an external crate path.
                    let child = format!("{scope}::{}", module.join("::"));
                    if let Some(r) = scoped(self.free_in_scope(&child, name), kind) {
                        return Some(r);
                    }
                    // `other_crate::a::f` for another crate of this
                    // workspace: the module suffix, name-level only.
                    if module.len() >= 2 {
                        let suffix = format!("::{}", module[1..].join("::"));
                        let hits: Vec<usize> = self
                            .index
                            .free(name)
                            .iter()
                            .copied()
                            .filter(|&f| {
                                let s = &self.index.nodes[f].callable.scope;
                                s.ends_with(&suffix) && rust_root(s) != rust_root(scope)
                            })
                            .collect();
                        return by_name(&hits, "external_path_suffix");
                    }
                    return None;
                };
                let module_path = absolute.join("::");
                if let Some(r) = scoped(self.free_in_scope(&module_path, name), kind) {
                    return Some(r);
                }
                let root = rust_root(scope);
                let same_crate = |ids: &[usize]| -> Vec<usize> {
                    ids.iter()
                        .copied()
                        .filter(|&m| rust_root(&self.index.nodes[m].callable.scope) == root)
                        .collect()
                };
                // `crate::a::Type::f`: impls may live in any module and the
                // method may come from a trait: same-crate methods by name.
                if let Some(owner) = absolute.last().filter(|o| is_type_like(o)) {
                    return by_name(&same_crate(&self.methods_of(owner, name)), "type_name");
                }
                // An in-crate path that does not name a declared function:
                // `pub use` re-exports and `#[path]` modules are not
                // followed, so same-crate functions of that name are
                // name-level candidates.
                by_name(&same_crate(self.index.free(name)), "unresolved_path_name")
            }
            Language::Kotlin => {
                let package = module.join(".");
                if let Some(r) = scoped(self.free_in_scope(&package, name), kind) {
                    return Some(r);
                }
                let (owner, owner_package) = module.split_last()?;
                if is_type_like(owner) {
                    let hits =
                        self.in_scope(&self.methods_of(owner, name), &owner_package.join("."));
                    return scoped(hits, "qualified_type");
                }
                None
            }
            Language::Typescript => None,
        }
    }

    /// Resolves a relative TypeScript module specifier to a snapshot file.
    fn ts_module_file(&self, from: &str, module: &str) -> Option<usize> {
        if !(module.starts_with("./") || module.starts_with("../")) {
            return None;
        }
        let mut parts: Vec<&str> = from.split('/').collect();
        parts.pop();
        for seg in module.split('/') {
            match seg {
                "." | "" => {}
                ".." => {
                    parts.pop()?;
                }
                s => parts.push(s),
            }
        }
        let stem = parts.join("/");
        // TypeScript's module resolution: an explicit JS-flavoured
        // extension maps to its TS counterpart; no extension tries `.ts`,
        // `.tsx`, then directory indexes.
        let candidates: Vec<String> = if let Some(base) = stem.strip_suffix(".mjs") {
            vec![format!("{base}.mts")]
        } else if let Some(base) = stem.strip_suffix(".cjs") {
            vec![format!("{base}.cts")]
        } else if let Some(base) = stem.strip_suffix(".jsx") {
            vec![format!("{base}.tsx")]
        } else if let Some(base) = stem.strip_suffix(".js") {
            vec![format!("{base}.ts"), format!("{base}.tsx")]
        } else if [".ts", ".tsx", ".mts", ".cts"]
            .iter()
            .any(|e| stem.ends_with(e))
        {
            vec![stem.clone()]
        } else {
            vec![
                format!("{stem}.ts"),
                format!("{stem}.tsx"),
                format!("{stem}/index.ts"),
                format!("{stem}/index.tsx"),
            ]
        };
        candidates
            .iter()
            .find_map(|c| self.index.paths.get(c.as_str()).copied())
    }
}

pub(crate) fn analyze(
    language: Language,
    target: &[SnapshotFile],
    base: &[SnapshotFile],
    diff: &BTreeMap<String, FileDiff>,
    target_unanalyzed: &BTreeSet<String>,
) -> Analysis {
    let index = Index::new(target);
    let resolver = Resolver {
        language,
        index: &index,
        files: target,
    };

    // Change status per target node.
    let base_by_path: BTreeMap<&str, &SnapshotFile> =
        base.iter().map(|f| (f.path.as_str(), f)).collect();
    let target_by_path: BTreeMap<&str, &SnapshotFile> =
        target.iter().map(|f| (f.path.as_str(), f)).collect();
    let matches: BTreeMap<&str, (Vec<Option<usize>>, Vec<bool>)> = base
        .iter()
        .filter_map(|b| {
            let t = target_by_path.get(b.path.as_str())?;
            Some((
                b.path.as_str(),
                match_declarations(&b.facts.callables, &t.facts.callables),
            ))
        })
        .collect();
    let mut status: Vec<ChangeStatus> = vec![ChangeStatus::Unchanged; index.nodes.len()];
    for (id, node) in index.nodes.iter().enumerate() {
        if node.callable.kind == CallableKind::Initializer {
            continue;
        }
        let Some(d) = diff.get(node.path) else {
            continue;
        };
        if d.added_file {
            status[id] = ChangeStatus::Added;
            continue;
        }
        let touched = d.touches(node.callable.start_line, node.callable.end_line);
        let Some(base_file) = base_by_path.get(node.path) else {
            // The base side could not be analyzed; changed lines still make
            // the touched callables changed.
            if touched {
                status[id] = ChangeStatus::ChangedBaseUnknown;
            }
            continue;
        };
        let local = id - index.file_base[node.file];
        let old = matches[node.path].0[local].map(|b| &base_file.facts.callables[b]);
        match old {
            None => status[id] = ChangeStatus::Added,
            Some(old) if touched || old.public != node.callable.public => {
                status[id] = ChangeStatus::Modified {
                    signature_changed: old.signature != node.callable.signature,
                    was_public: old.public,
                };
            }
            Some(_) => {}
        }
    }

    // Resolve every call once.
    let mut call_stats: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut edges: BTreeMap<usize, BTreeMap<usize, Edge>> = BTreeMap::new();
    let mut over_fanout: BTreeMap<usize, usize> = BTreeMap::new();
    let mut unresolved_names: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
    for (fi, file) in target.iter().enumerate() {
        for call in &file.facts.calls {
            *call_stats.entry("total").or_insert(0) += 1;
            let caller = index.file_base[fi] + call.caller;
            let Some(resolution) = resolver.resolve(fi, caller, call) else {
                *call_stats.entry("unresolved").or_insert(0) += 1;
                unresolved_names
                    .entry(call.name.clone())
                    .or_default()
                    .push((file.path.clone(), call.line));
                continue;
            };
            let over = resolution.confidence == Confidence::Ambiguous
                && resolution.targets.len() > MAX_AMBIGUOUS_FANOUT;
            *call_stats
                .entry(if over {
                    "over_fanout"
                } else {
                    resolution.confidence.as_str()
                })
                .or_insert(0) += 1;
            for &callee in &resolution.targets {
                if status[callee] == ChangeStatus::Unchanged {
                    continue;
                }
                if over {
                    *over_fanout.entry(callee).or_insert(0) += 1;
                    continue;
                }
                if callee == caller {
                    continue;
                }
                let edge = edges
                    .entry(callee)
                    .or_default()
                    .entry(caller)
                    .or_insert_with(|| Edge {
                        caller,
                        lines: BTreeSet::new(),
                        kinds: BTreeSet::new(),
                        confidence: resolution.confidence,
                        candidate_count: resolution.targets.len(),
                    });
                edge.lines.insert(call.line);
                edge.kinds.insert(resolution.kind);
                if resolution.confidence < edge.confidence {
                    edge.confidence = resolution.confidence;
                    edge.candidate_count = resolution.targets.len();
                }
            }
        }
    }

    let mut changed = Vec::new();
    for (id, st) in status.iter().enumerate() {
        if *st == ChangeStatus::Unchanged {
            continue;
        }
        let e = edges
            .remove(&id)
            .unwrap_or_default()
            .into_values()
            .map(|edge| (index.info(edge.caller), edge))
            .collect();
        changed.push(ChangedCallable {
            node: index.info(id),
            status: st.clone(),
            edges: e,
            over_fanout_sites: over_fanout.get(&id).copied().unwrap_or(0),
        });
    }

    // Removed callables: base declarations of changed files that no
    // target declaration matched. A target file that was not (fully)
    // analyzed proves nothing about removal, so it contributes none.
    let mut removed = Vec::new();
    for base_file in base {
        if target_unanalyzed.contains(&base_file.path) {
            continue;
        }
        let keys = file_keys(&base_file.path, &base_file.facts.callables);
        let matched = matches.get(base_file.path.as_str()).map(|m| m.1.as_slice());
        for (i, (callable, key)) in base_file.facts.callables.iter().zip(keys).enumerate() {
            if callable.kind == CallableKind::Initializer || matched.is_some_and(|m| m[i]) {
                continue;
            }
            let mut naming_sites = unresolved_names
                .get(&callable.name)
                .cloned()
                .unwrap_or_default();
            naming_sites.sort();
            removed.push(RemovedCallable {
                key,
                path: base_file.path.clone(),
                callable: callable.clone(),
                naming_sites,
            });
        }
    }

    let changed_files = diff
        .iter()
        .map(|(path, d)| {
            json!({
                "path": path,
                "status": if d.added_file { "added" } else if d.deleted_file { "deleted" } else { "modified" },
                "changed_line_ranges": d.ranges.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>(),
                "deletion_points": d.deletion_points,
            })
        })
        .collect();

    Analysis {
        changed,
        removed,
        changed_files,
        call_stats,
    }
}

fn callable_json(key: &str, path: &str, c: &Callable) -> Value {
    json!({
        "key": key,
        "path": path,
        "name": c.name,
        "owner": c.owner,
        "kind": c.kind.as_str(),
        "public": c.public,
        "start_line": c.start_line,
        "end_line": c.end_line,
        "signature": c.signature,
    })
}

fn change_json(status: &ChangeStatus, now_public: bool) -> Value {
    match status {
        ChangeStatus::Unchanged => json!({"status": "unchanged"}),
        ChangeStatus::Added => json!({"status": "added"}),
        ChangeStatus::Modified {
            signature_changed,
            was_public,
        } => json!({
            "status": "modified",
            "signature_changed": signature_changed,
            "visibility_changed": *was_public != now_public,
        }),
        ChangeStatus::ChangedBaseUnknown => json!({
            "status": "changed",
            "base_analyzed": false,
            "signature_changed": null,
            "visibility_changed": null,
        }),
    }
}

fn obligation(rule_id: &str, target_key: Value, subject: Value) -> Value {
    json!({
        "id": obligation_id(rule_id, &target_key),
        "rule_id": rule_id,
        "status": "deferred",
        "target_key": target_key,
        "subject": subject,
    })
}

pub(crate) fn render(
    request: &SourceReviewRequest,
    base: &str,
    target: &str,
    analysis: &Analysis,
    unanalyzed: &[Unanalyzed],
    inventory: Value,
) -> Value {
    let mut obligations = Vec::new();
    let mut changed_json = Vec::new();
    for c in &analysis.changed {
        let callee = callable_json(&c.node.key, &c.node.path, &c.node.callable);
        let change = change_json(&c.status, c.node.callable.public);
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, e) in &c.edges {
            *counts.entry(e.confidence.as_str()).or_insert(0) += 1;
        }
        changed_json.push(json!({
            "callable": callee,
            "change": change,
            "callers": {
                "exact": counts.get("exact").copied().unwrap_or(0),
                "name_only": counts.get("name_only").copied().unwrap_or(0),
                "ambiguous": counts.get("ambiguous").copied().unwrap_or(0),
                "unlinked_over_fanout_call_sites": c.over_fanout_sites,
            },
        }));
        // Public now, or public before: a narrowed visibility is a contract
        // change too.
        let was_public = matches!(
            c.status,
            ChangeStatus::Modified {
                was_public: true,
                ..
            }
        );
        if c.node.callable.public || was_public {
            obligations.push(obligation(
                RULE_CHANGED_PUBLIC_CALLABLE,
                json!([c.node.key]),
                json!({"callable": callee, "change": change}),
            ));
        }
        for (caller, e) in &c.edges {
            obligations.push(obligation(
                RULE_CHANGED_CALLEE_CALLER,
                json!([c.node.key, caller.key]),
                json!({
                    "callee": callee,
                    "callee_change": change,
                    "caller": callable_json(&caller.key, &caller.path, &caller.callable),
                    "call_site_lines": e.lines.iter().collect::<Vec<_>>(),
                    "resolution": {
                        "confidence": e.confidence.as_str(),
                        "kinds": e.kinds.iter().collect::<Vec<_>>(),
                        "candidate_count": e.candidate_count,
                    },
                }),
            ));
        }
    }
    let mut removed_json = Vec::new();
    for r in &analysis.removed {
        let subject = callable_json(&r.key, &r.path, &r.callable);
        removed_json.push(subject.clone());
        if r.callable.public {
            obligations.push(obligation(
                RULE_REMOVED_PUBLIC_CALLABLE,
                json!([r.key]),
                json!({
                    "base_callable": subject,
                    "unresolved_target_call_sites_with_same_name": r
                        .naming_sites
                        .iter()
                        .map(|(p, l)| json!({"path": p, "line": l}))
                        .collect::<Vec<_>>(),
                }),
            ));
        }
    }
    for u in unanalyzed {
        let (side, reason) = match u.reason.strip_prefix("base:") {
            Some(r) => ("base", r),
            None => ("target", u.reason.as_str()),
        };
        obligations.push(obligation(
            RULE_UNANALYZED_SOURCE,
            json!([u.path, side]),
            json!({"path": u.path, "snapshot": side, "reason": reason, "syntax_error_lines": u.lines}),
        ));
    }
    obligations.sort_by(|a, b| {
        (a["rule_id"].as_str(), a["target_key"].to_string())
            .cmp(&(b["rule_id"].as_str(), b["target_key"].to_string()))
    });
    obligations.dedup_by(|a, b| a["id"] == b["id"]);
    let mut by_rule: BTreeMap<String, usize> = BTreeMap::new();
    for o in &obligations {
        *by_rule
            .entry(o["rule_id"].as_str().unwrap_or_default().to_owned())
            .or_insert(0) += 1;
    }
    let mut calls = serde_json::Map::new();
    for key in [
        "total",
        "exact",
        "name_only",
        "ambiguous",
        "over_fanout",
        "unresolved",
    ] {
        calls.insert(
            key.to_owned(),
            json!(analysis.call_stats.get(key).copied().unwrap_or(0)),
        );
    }
    let mut run = json!({
        "schema": SOURCE_REVIEW_RUN_V1_SCHEMA,
        "language": request.language.as_str(),
        "base_commit": base,
        "target_commit": target,
        "authority": {
            "trusted_pass": false,
            "result_status": if unanalyzed.is_empty() { "deferred" } else { "incomplete" },
        },
        "inventory": inventory,
        "changed_files": analysis.changed_files,
        "changed_callables": changed_json,
        "removed_callables": removed_json,
        "obligations": obligations,
        "coverage": {
            "obligations": obligations.len(),
            "by_rule": by_rule,
            "calls": calls,
            "max_ambiguous_fanout": MAX_AMBIGUOUS_FANOUT,
        },
        "limitations": request.language.limitations(),
        "unanalyzed_files": unanalyzed.iter().map(|u| json!({"path": u.path, "reason": u.reason, "syntax_error_lines": u.lines})).collect::<Vec<_>>(),
    });
    let preimage = serde_json::to_vec(&run).unwrap_or_default();
    run["run_id"] = json!(format!("run:sha256:{}", sha256_hex(&preimage)));
    run
}
