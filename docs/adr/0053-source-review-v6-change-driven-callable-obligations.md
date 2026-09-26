# ADR 0053: Source review v6 — change-driven callable obligations for Rust, TypeScript and Kotlin

- Status: Proposed
- Date: 2026-09-26

## Context

A smoke check on 2026-09-26 found three gaps in the existing routes. It
used small synthetic repositories, two change scenarios per language, one
run each.

1. **Kotlin** (`kotlin.normalized_review@3`, registry `kotlin.r3`) emits
   `obligations: []` and `enumeration: "unavailable"`. This is by design:
   the R3 extractor observes only `class`/`interface`/`object` declarations,
   with no functions, visibility or calls.
2. **TypeScript** (`typescript.obligation_report@1`, registry r2) never
   emits `relation.changed_public_callee`. `d_change_witnesses` is the
   literal `"unavailable"`, base sources are never read, and
   `typescript/changes.rs::change_witnesses` is `todo!()`.
3. **Methods** are out of scope everywhere:
   - Rust v4 Node and D gate on `kind == "function"`, and method calls are
     recorded as obstructions (ADR 0038, ADR 0040).
   - TypeScript v5 does not collect `method_definition`.

Each gap is pinned by rule IDs, registry/tuple hashes, schemas and frozen
acceptance bytes. ADR 0038 L236-238 and L1813-1816 and ADR 0040 L116-120
forbid changing an existing rule trigger in place.

The product's consumer is an AI reviewer. It needs one thing from a run: for
a base→target change, which changed callables to review and which callers
to re-check, with honest labels on how each link was established. It does
not need a human-readable report.

## Decision

Add a **new, separate route**. No existing route, rule ID, registry, schema,
golden or byte changes.

### Request

The request schema is `reviewgraphen.source_review_request.v6`. Unknown
fields are refused.

| field | meaning |
| --- | --- |
| `language` | `rust`, `typescript` or `kotlin` |
| `base_revision`, `target_revision` | revision names in the invocation repository |
| `limits` (optional) | `max_files`, `max_file_bytes`, `max_total_source_bytes`; all must be positive |

- The invocation directory must be the Git top level. A mismatch, including
  one caused by a configured `core.worktree`, fails closed with exit 20.
- Malformed requests exit with code 3. Repository, read or write failures
  exit with code 20.
- The run is written to `<artifacts>/source-review.run.v1.json` through the
  existing descriptor-relative `AdmittedRoot` writer (T1). stdout carries the
  same bytes.

### Reading

All reads go through `git::source_review_v6`, which reuses the hygienic
`git_command` policy:

- the environment is cleared;
- host and user config is disabled;
- `GIT_NO_LAZY_FETCH=1`;
- `--no-replace-objects`;
- the output and time bounds are the same as other routes.

The reachable commands are `rev-parse`, `ls-tree`, `cat-file blob`, and a
pinned diff:

- `-c core.quotePath=false diff --no-renames --text --no-ext-diff --no-textconv --diff-algorithm=myers --inter-hunk-context=0 --indent-heuristic -U0`;
- restricted to the language's pathspecs (`*.rs`; `*.ts`, `*.tsx`, `*.mts`, `*.cts`; `*.kt`).

How the patch is parsed:

- Header paths are decoded, both the C-quoted form and the TAB-suffixed
  form.
- Hunk bodies are consumed by their declared line counts, so a content line
  is never read as a header.
- Every changed path in `diff --name-status -z` must also appear in the
  parsed patch. A path missing from the patch becomes a
  `diff_unparsed` gap.

Only regular tracked blobs of the requested language are read.

- TypeScript: `.ts`, `.tsx`, `.mts`, `.cts`, excluding `.d.*ts`.
- Kotlin: `.kt`.

Base sources are read only for files that changed.

- A mode-only change (same blob) is not a change.
- A regular-file↔symlink change is a `file_type_changed` gap.
- Analysis runs on a thread with a 1 GiB stack. Bracket nesting beyond
  20,000 levels is reported as `nesting_too_deep` instead of being parsed.

### Extraction

| language | parser | callables | call forms |
| --- | --- | --- | --- |
| Rust | `syn` (unexpanded) | free fns, inherent/trait-impl methods, trait methods, inline modules | `f()`, `a::b::f()`, `Type::f()`, `Self::f()`, `x.m()`, `self.m()`; macro arguments when they parse as expressions |
| TypeScript | tree-sitter-typescript 0.23.2 | function declarations, const arrow/function expressions, class methods and arrow fields, abstract methods | `f()`, `ns.f()`, `X.f()`, `this.m()`, `x.m()` |
| Kotlin | tree-sitter-kotlin-ng 1.1.0 | top-level fns, class/object/interface/companion members, extension fns (owner = receiver type) | `f()`, `X.f()`, `a.b.f()`, `this.m()`, `x.m()`, implicit `this` |

Public means:

| language | public |
| --- | --- |
| Rust | exact `pub`, trait-impl methods, or methods of a `pub` trait |
| TypeScript | exported, or listed in `export { }`, and not `private`/`protected`/`#` |
| Kotlin | no `private`, `internal` or `protected` on the declaration or any enclosing class |

**Syntax errors.** Rust files that `syn` cannot parse are unanalyzed. For
TypeScript and Kotlin, facts outside ERROR/MISSING nodes are kept, and the
file is *also* reported as `partial_syntax_error` with those lines. For
example, tree-sitter-typescript 0.23.2 cannot parse `sql<T>\`..\``.

### Callers outside function bodies

Code that runs outside any function body is attributed to a synthetic
`<init>` caller of kind `initializer`, one per file or owning type:

- module-level statements and initializers;
- TypeScript class field initializers and `static {}` blocks;
- Kotlin property initializers, `init`, secondary constructors and
  accessors;
- Rust `const` and `static` initializers.

An `<init>` caller is never a resolution target, a change subject or a
removal subject.

### Local names

Local names are bound per callable: parameters, local variables and
patterns, catch parameters, and nested or local functions. A plain call to
a local name is not resolved against module-level declarations.

Nested and local functions are not separate callables. Their calls belong
to the enclosing callable.

### Change mapping

- Callable keys are `path#Owner.name`. When a file declares the same
  qualified name more than once (overloads), the key is
  `path#Owner.name(<params>)`.
- Base and target declarations of one file are paired within each
  qualified name:
  - one declaration on each side pairs directly;
  - otherwise declarations pair by identical parameter lists.
- A target callable is:
  - `added` if its file is new or it has no base partner;
  - `modified` if a target-side hunk range intersects its lines, a pure
    deletion point falls strictly inside it, or its visibility changed.
    `signature_changed` compares the whitespace-normalized heads, including
    visibility. `visibility_changed` is reported too.
- An unpaired base callable is `removed`. The exception is a target file
  that was not fully analyzed: it reports no removals.
- When the base side of a changed file could not be analyzed, touched
  callables get `{"status": "changed", "base_analyzed": false}`. Their
  `signature_changed` and `visibility_changed` are `null`, not `false`.
- Keys are for identification, not for change tracking. Adding a second
  declaration of a name, or changing an overload's parameters, changes the
  key.

### Resolution

`exact` means the language's scoping rules pin exactly one declaration in
this snapshot. More than one pinned declaration (for example an overload
set) is `ambiguous`. A match by bare name is `name_only`.

| confidence | kinds |
| --- | --- |
| `exact` | `import`, `import_glob`, `same_scope`, `qualified_path` (free functions placed by scope rules); TypeScript/Kotlin `self_receiver` (declared in the caller's file and scope) and `qualified_type` (the type is declared in the caller's file or package, or located through a relative import or re-export) |
| `name_only` | `name`, `method_name`, `type_name`, `self_receiver_by_name`, `self_receiver_other_file`, `package_import_name`, `external_path_suffix`, `unresolved_path_name`, `implicit_receiver_possible`, `local_glob_use` |
| `ambiguous` | any kind with more than one candidate |

Precedence and scoping:

- Kotlin: member (implicit `this`), then explicit import, then same
  package, then star import, then name.
- Rust: `use`, then same module, then glob `use`, then name within the same
  crate.
- TypeScript: only imported (following `export … from` barrels) or
  same-file names.
- Imports bind only in the module or package that declares them.
- Rust crates are told apart by Cargo's target layout:
  - `<dir>/src/lib.rs` is `crate@<dir>`;
  - `src/main.rs` is its own crate when a `src/lib.rs` exists;
  - `src/bin/<name>` is a crate;
  - a file outside `src/` roots its own crate.

  A cross-crate path is at most `name_only`.
- Rust method calls are **never** `exact`. This covers `self.m()`,
  `Self::m()`, `Type::m()` and `crate::a::Type::m()`. The target depends on
  type and trait resolution: inherent impls in other modules win over
  trait impls, and generic parameters shadow structs. Owners are compared
  by their last path segment. Same-crate candidates are linked by name.
- An in-crate Rust path that does not name a declared function links
  same-crate functions of that name as `unresolved_path_name`. This covers
  `pub use` re-exports and `#[path]` modules, which are not followed.
- A `use` inside a Rust function body binds its names locally, so they are
  not resolved. A glob `use` inside a body weakens every plain call in it
  to `local_glob_use`.
- A qualified call whose head is a local name is never resolved as a
  path or namespace. Examples are a body-local `use` alias or a parameter
  that shadows a namespace import. For TypeScript and Kotlin it is a
  method on a value (`method_name`).
- Kotlin `this@Label.m()` is not the caller's own receiver.
- Kotlin companion members keep the enclosing class as `owner`, so
  `Owner.f()` finds them, but they have no implicit instance receiver.
  Their unqualified calls are never `self_receiver`.
- A Kotlin top-level extension property's accessor is attributed to an
  `<init>` owned by the receiver type, so it is weakened like an extension
  function.
- In a TypeScript script (no top-level import or export), a plain call
  resolved in an enclosing scope from inside a namespace is weakened to
  `script_namespace_outer`. Namespaces merge across script files and their
  members shadow outer names.
- TypeScript `for (… of/in …)` bindings and a named function expression's
  own name are local names.
- TypeScript exports follow `export { local as name }`,
  `export default local;` and `export default class Name`.
- An explicitly imported or path-qualified type that cannot be located in
  the snapshot resolves to nothing. It never falls back to a repo type of
  the same bare name.
- A Kotlin plain call is weakened to `implicit_receiver_possible` when it
  is made from a member, an extension function, or a lambda literal and
  was not resolved to a same-file member of the caller's type. In those
  positions an implicit receiver takes precedence over top-level functions
  and cannot be seen here. Implicit receivers include inherited members and
  library receivers such as `apply {}` on `StringBuilder`.
- TypeScript `this` is the class instance only outside nested non-arrow
  functions, object-literal methods and classes.
- TypeScript namespaces scope as `file#A.B`. Plain calls look outward
  through enclosing namespaces. `N.f()` resolves into a namespace declared
  in the file. A namespace member is public only when every enclosing
  namespace is exported.
- TypeScript module specifiers map `.js` to `.ts` then `.tsx`, `.mjs` to
  `.mts`, `.cjs` to `.cts` and `.jsx` to `.tsx`. A specifier without an
  extension tries `.ts`, `.tsx`, then `index.ts` and `index.tsx`.
  - `import d from` resolves through `export default function name`.
  - Classes resolve through `export … from` barrels.
- One caller edge aggregates every call site from that caller to that
  callee:
  - `confidence` is the strongest across the sites, because one exact site
    proves the relation;
  - `kinds` lists all resolution kinds seen;
  - `call_site_lines` lists every site.
- TypeScript `new X()` resolves to `X.constructor`, and a capitalized JSX
  tag `<X/>` resolves as a call to `X`.
- An ambiguous candidate set larger than 5 is not linked. It is counted per
  changed callable as `unlinked_over_fanout_call_sites`.

### Obligations

Every obligation has status `deferred`. `trusted_pass` is always false.

| rule ID | target key | when |
| --- | --- | --- |
| `node.changed_public_callable_contract@1` | `[callee]` | a callable is added or modified and is public now or was public before |
| `relation.changed_callee_caller@1` | `[callee, caller]` | a resolved call edge into any changed callable. It carries `call_site_lines` and `resolution {confidence, kinds, candidate_count}` |
| `node.removed_public_callable@1` | `[base callable]` | a public callable was removed. It carries the target call sites that still name it but are unresolved |
| `capability_gap.unanalyzed_source@1` | `[path, "target"\|"base"]` | a tracked source file was not, or only partially, analyzed (bounds, UTF-8, parse, `partial_syntax_error` with its lines, `nesting_too_deep`, `diff_unparsed`, `file_type_changed`). This sets `result_status: "incomplete"` |

Obligation IDs are `sha256({rule_id, target_key})`. The run ID is the
sha256 of the canonical run without `run_id`. Output keys are sorted, so a
run is byte-deterministic.

The run also carries `changed_callables` (with per-confidence caller
counts), `removed_callables`, `changed_files`, `coverage.calls` (the
resolution histogram) and `limitations`.

## Consequences

- Kotlin gets change-driven obligations. In 0.2.0 the Kotlin v5 (R3)
  publication route is retired: it enumerated no obligations and accepted
  only a fixed set of source-set layouts. Kotlin v5 requests are refused
  with exit 3.
- TypeScript gets changed-callee→caller relations. The r2 bytes are
  unchanged.
- Changed methods are review subjects in all three languages. Method call
  edges are labelled `name_only` or `ambiguous`, because nothing here infers
  receiver types.
- The route is not a proof of complete caller enumeration. The
  `limitations` array states what is never seen:
  - calls through function values;
  - reflection;
  - macro bodies that do not parse as expressions;
  - package and path-alias imports in TypeScript;
  - non-callable declarations.
- Its evidence is `crates/reviewgraphen-cli/tests/source_review_v6_acceptance.rs`,
  the unit tests in `source_graph/tests.rs`, and the Kotlin cases of
  `o9r_git_config_hygiene_acceptance.rs`. Those cases show that
  repository-local Git configuration does not change the run, or that the run
  fails closed.

## Known unresolved (2026-09-26)

Four independent review rounds were run on synthetic repros. Items found
and not fixed:

- TypeScript `import * as X` followed by `X.hidden()`, where `hidden` is
  declared but exported only under another name, resolves `exact` to
  `hidden`. This only occurs in code that does not compile.

Not checked by any round [U]:

- TypeScript and Kotlin semantics against real compilers. The Rust repros
  in rounds 3 and 4 were checked with `cargo`.
- macro_rules-generated Rust functions.
- TypeScript `declare global` and `tsconfig` path aliases.
- Kotlin typealias-qualified calls and operator functions.
- Behaviour at `max_files` and `max_total_source_bytes`.
- Scale beyond four real repositories, with one run each.

## Revisit when

- Receiver-type inference is available, which would promote method edges to
  `exact`.
- TypeScript path aliases (`tsconfig` `paths`) need resolving.
- Constants, types and fields need their own change obligations.
