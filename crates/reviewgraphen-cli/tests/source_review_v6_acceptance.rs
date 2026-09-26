//! Source review v6 (ADR 0053) end to end through the real binary: changed
//! methods and free functions, their callers, removed and added callables,
//! request refusals, artifact publication and determinism, for Rust,
//! TypeScript and Kotlin.

use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const SCHEMA: &str = "reviewgraphen.source_review_request.v6";
const RUN_FILE: &str = "source-review.run.v1.json";

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

struct Repo {
    dir: TempDir,
    commits: Vec<String>,
}

impl Repo {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        Self {
            dir,
            commits: Vec::new(),
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, path: &str, text: &str) {
        let p = self.root().join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn remove(&self, path: &str) {
        fs::remove_file(self.root().join(path)).unwrap();
    }

    fn commit(&mut self) -> String {
        git(self.root(), &["add", "-A"]);
        git(self.root(), &["commit", "-q", "--allow-empty", "-m", "c"]);
        let c = git(self.root(), &["rev-parse", "HEAD"]);
        self.commits.push(c.clone());
        c
    }
}

struct Outcome {
    code: i32,
    stdout: Vec<u8>,
    stderr: String,
}

fn review(cwd: &Path, request: &str, artifacts: &str) -> Outcome {
    let req = TempDir::new().unwrap();
    let req_path = req.path().join("request.json");
    fs::write(&req_path, request).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
        .args(["review", "--request"])
        .arg(&req_path)
        .args(["--artifacts", artifacts])
        .current_dir(cwd)
        .output()
        .unwrap();
    Outcome {
        code: out.status.code().unwrap_or(-1),
        stdout: out.stdout,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn request(language: &str, base: &str, target: &str) -> String {
    format!(
        r#"{{"schema":"{SCHEMA}","language":"{language}","base_revision":"{base}","target_revision":"{target}"}}"#
    )
}

fn review_run(repo: &Repo, language: &str, base: &str, target: &str) -> Value {
    let art = format!(".rg6-{}", &target[..8]);
    let out = review(repo.root(), &request(language, base, target), &art);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let written = fs::read(repo.root().join(&art).join(RUN_FILE)).unwrap();
    assert_eq!(written, out.stdout, "artifact equals stdout");
    fs::remove_dir_all(repo.root().join(&art)).unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}

/// `(rule, target_key...)` rows, plus the resolution confidence for edges.
fn rows(run: &Value) -> Vec<String> {
    run["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            let keys: Vec<&str> = o["target_key"]
                .as_array()
                .unwrap()
                .iter()
                .map(|k| k.as_str().unwrap())
                .collect();
            let mut row = format!("{} {}", o["rule_id"].as_str().unwrap(), keys.join(" <- "));
            if let Some(conf) = o["subject"]["resolution"]["confidence"].as_str() {
                row.push_str(&format!(" [{conf}]"));
            }
            row
        })
        .collect()
}

fn assert_rows(run: &Value, expected: &[&str]) {
    let mut got = rows(run);
    got.sort();
    let mut want: Vec<String> = expected.iter().map(|s| (*s).to_owned()).collect();
    want.sort();
    assert_eq!(got, want, "run: {run:#}");
    assert_eq!(run["authority"]["trusted_pass"], false);
}

// ------------------------------------------------------------------ Rust

fn rust_repo() -> Repo {
    let mut r = Repo::new();
    r.write(
        "Cargo.toml",
        "[package]\nname = \"bank\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    r.write("src/lib.rs", "pub mod account;\npub mod transfer;\n");
    r.write(
        "src/account.rs",
        "pub struct Account { pub balance: i64 }\nimpl Account {\n    pub fn withdraw(&mut self, amount: i64) -> bool {\n        if amount <= 0 || amount > self.balance { return false; }\n        self.balance -= amount;\n        true\n    }\n}\npub fn format_balance(a: &Account) -> String { format!(\"{}\", a.balance) }\npub fn legacy() {}\n",
    );
    r.write(
        "src/transfer.rs",
        "use crate::account::{Account, format_balance};\npub fn transfer(from: &mut Account, to: &mut Account, amount: i64) -> bool {\n    if from.withdraw(amount) { to.balance += amount; true } else { false }\n}\npub fn summary(a: &Account) -> String { format_balance(a) }\n#[cfg(test)]\nmod tests {\n    use super::*;\n    fn t(a: &Account) { let _ = summary(a); }\n}\n",
    );
    r.commit();
    // c1: method guard dropped.
    r.write(
        "src/account.rs",
        "pub struct Account { pub balance: i64 }\nimpl Account {\n    pub fn withdraw(&mut self, amount: i64) -> bool {\n        if amount > self.balance { return false; }\n        self.balance -= amount;\n        true\n    }\n}\npub fn format_balance(a: &Account) -> String { format!(\"{}\", a.balance) }\npub fn legacy() {}\n",
    );
    r.commit();
    // c2: free function body and signature change; `legacy` removed;
    // a new file with a new public function.
    r.write(
        "src/account.rs",
        "pub struct Account { pub balance: i64 }\nimpl Account {\n    pub fn withdraw(&mut self, amount: i64) -> bool {\n        if amount > self.balance { return false; }\n        self.balance -= amount;\n        true\n    }\n}\npub fn format_balance(a: &Account) -> String { format!(\"{} JPY\", a.balance / 100) }\n",
    );
    r.write("src/extra.rs", "pub fn added() -> u8 { 1 }\n");
    r.commit();
    r
}

#[test]
fn rust_changed_method_links_its_caller() {
    let r = rust_repo();
    let run = review_run(&r, "rust", &r.commits[0], &r.commits[1]);
    assert_rows(
        &run,
        &[
            "node.changed_public_callable_contract@1 src/account.rs#Account.withdraw",
            "relation.changed_callee_caller@1 src/account.rs#Account.withdraw <- src/transfer.rs#transfer [name_only]",
        ],
    );
    assert_eq!(run["changed_callables"][0]["change"]["status"], "modified");
    assert_eq!(run["changed_callables"][0]["callable"]["kind"], "method");
}

#[test]
fn rust_changed_free_function_removed_and_added_callables() {
    let r = rust_repo();
    let run = review_run(&r, "rust", &r.commits[1], &r.commits[2]);
    assert_rows(
        &run,
        &[
            "node.changed_public_callable_contract@1 src/account.rs#format_balance",
            "node.changed_public_callable_contract@1 src/extra.rs#added",
            "relation.changed_callee_caller@1 src/account.rs#format_balance <- src/transfer.rs#summary [exact]",
            "node.removed_public_callable@1 src/account.rs#legacy",
        ],
    );
    let added = run["changed_callables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["callable"]["name"] == "added")
        .unwrap();
    assert_eq!(added["change"]["status"], "added");
}

#[test]
fn rust_glob_import_in_test_module_resolves_exactly() {
    let mut r = rust_repo();
    let base = r.commits[2].clone();
    r.write(
        "src/transfer.rs",
        "use crate::account::{Account, format_balance};\npub fn transfer(from: &mut Account, to: &mut Account, amount: i64) -> bool {\n    if from.withdraw(amount) { to.balance += amount; true } else { false }\n}\npub fn summary(a: &Account) -> String { format!(\"[{}]\", format_balance(a)) }\n#[cfg(test)]\nmod tests {\n    use super::*;\n    fn t(a: &Account) { let _ = summary(a); }\n}\n",
    );
    let target = r.commit();
    let run = review_run(&r, "rust", &base, &target);
    assert_rows(
        &run,
        &[
            "node.changed_public_callable_contract@1 src/transfer.rs#summary",
            "relation.changed_callee_caller@1 src/transfer.rs#summary <- src/transfer.rs#t [exact]",
        ],
    );
    let edge = run["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["rule_id"] == "relation.changed_callee_caller@1")
        .unwrap();
    assert_eq!(edge["subject"]["resolution"]["kinds"][0], "import_glob");
}

// ------------------------------------------------------------ TypeScript

fn ts_repo() -> Repo {
    let mut r = Repo::new();
    r.write("package.json", "{}\n");
    r.write(
        "src/account.ts",
        "export class Account {\n  constructor(public balance: number) {}\n  withdraw(amount: number): boolean {\n    if (amount <= 0 || amount > this.balance) return false;\n    this.balance -= amount;\n    return true;\n  }\n}\nexport function formatBalance(a: Account): string { return String(a.balance); }\n",
    );
    r.write(
        "src/transfer.ts",
        "import { Account, formatBalance } from \"./account\";\nexport function transfer(from: Account, to: Account, amount: number): boolean {\n  if (from.withdraw(amount)) { to.balance += amount; return true; }\n  return false;\n}\nexport function summary(a: Account): string { return formatBalance(a); }\n",
    );
    r.commit();
    r.write(
        "src/account.ts",
        "export class Account {\n  constructor(public balance: number) {}\n  withdraw(amount: number): boolean {\n    if (amount > this.balance) return false;\n    this.balance -= amount;\n    return true;\n  }\n}\nexport function formatBalance(a: Account): string { return String(a.balance); }\n",
    );
    r.commit();
    r.write(
        "src/account.ts",
        "export class Account {\n  constructor(public balance: number) {}\n  withdraw(amount: number): boolean {\n    if (amount > this.balance) return false;\n    this.balance -= amount;\n    return true;\n  }\n}\nexport function formatBalance(a: Account, unit = \"JPY\"): string { return `${a.balance / 100} ${unit}`; }\n",
    );
    r.commit();
    r
}

#[test]
fn typescript_changed_method_links_its_caller() {
    let r = ts_repo();
    let run = review_run(&r, "typescript", &r.commits[0], &r.commits[1]);
    assert_rows(
        &run,
        &[
            "node.changed_public_callable_contract@1 src/account.ts#Account.withdraw",
            "relation.changed_callee_caller@1 src/account.ts#Account.withdraw <- src/transfer.ts#transfer [name_only]",
        ],
    );
}

#[test]
fn typescript_changed_imported_function_links_its_caller_exactly() {
    let r = ts_repo();
    let run = review_run(&r, "typescript", &r.commits[1], &r.commits[2]);
    assert_rows(
        &run,
        &[
            "node.changed_public_callable_contract@1 src/account.ts#formatBalance",
            "relation.changed_callee_caller@1 src/account.ts#formatBalance <- src/transfer.ts#summary [exact]",
        ],
    );
    assert_eq!(
        run["changed_callables"][0]["change"]["signature_changed"],
        true
    );
}

#[test]
fn typescript_partial_syntax_error_is_a_gap_not_a_silent_drop() {
    let mut r = ts_repo();
    let base = r.commits[2].clone();
    r.write(
        "src/q.ts",
        "export function q() { return sql<{ a: string }>`x`.run(); }\n",
    );
    let target = r.commit();
    let run = review_run(&r, "typescript", &base, &target);
    assert_eq!(run["authority"]["result_status"], "incomplete");
    let rows = rows(&run);
    assert!(
        rows.contains(&"capability_gap.unanalyzed_source@1 src/q.ts <- target".to_owned()),
        "{rows:?}"
    );
    assert!(
        rows.contains(&"node.changed_public_callable_contract@1 src/q.ts#q".to_owned()),
        "{rows:?}"
    );
}

// ---------------------------------------------------------------- Kotlin

fn kotlin_repo() -> Repo {
    let mut r = Repo::new();
    r.write("settings.gradle.kts", "rootProject.name = \"bank\"\n");
    r.write(
        "src/main/kotlin/bank/Account.kt",
        "package bank\nclass Account(var balance: Long) {\n    fun withdraw(amount: Long): Boolean {\n        if (amount <= 0 || amount > balance) return false\n        balance -= amount\n        return true\n    }\n}\nfun formatBalance(a: Account): String = a.balance.toString()\n",
    );
    r.write(
        "src/main/kotlin/bank/Transfer.kt",
        "package bank\nfun transfer(from: Account, to: Account, amount: Long): Boolean {\n    if (from.withdraw(amount)) { to.balance += amount; return true }\n    return false\n}\nfun summary(a: Account): String = formatBalance(a)\n",
    );
    r.write(
        "src/main/kotlin/app/Main.kt",
        "package app\nimport bank.formatBalance\nimport bank.Account\nfun main() { println(formatBalance(Account(1))) }\n",
    );
    r.commit();
    r.write(
        "src/main/kotlin/bank/Account.kt",
        "package bank\nclass Account(var balance: Long) {\n    fun withdraw(amount: Long): Boolean {\n        if (amount > balance) return false\n        balance -= amount\n        return true\n    }\n}\nfun formatBalance(a: Account): String = a.balance.toString()\n",
    );
    r.commit();
    r.write(
        "src/main/kotlin/bank/Account.kt",
        "package bank\nclass Account(var balance: Long) {\n    fun withdraw(amount: Long): Boolean {\n        if (amount > balance) return false\n        balance -= amount\n        return true\n    }\n}\nfun formatBalance(a: Account): String = \"${a.balance / 100} JPY\"\n",
    );
    r.commit();
    r
}

#[test]
fn kotlin_changed_method_links_its_caller() {
    let r = kotlin_repo();
    let run = review_run(&r, "kotlin", &r.commits[0], &r.commits[1]);
    assert_rows(
        &run,
        &[
            "node.changed_public_callable_contract@1 src/main/kotlin/bank/Account.kt#Account.withdraw",
            "relation.changed_callee_caller@1 src/main/kotlin/bank/Account.kt#Account.withdraw <- src/main/kotlin/bank/Transfer.kt#transfer [name_only]",
        ],
    );
}

#[test]
fn kotlin_changed_function_links_same_package_and_imported_callers() {
    let r = kotlin_repo();
    let run = review_run(&r, "kotlin", &r.commits[1], &r.commits[2]);
    assert_rows(
        &run,
        &[
            "node.changed_public_callable_contract@1 src/main/kotlin/bank/Account.kt#formatBalance",
            "relation.changed_callee_caller@1 src/main/kotlin/bank/Account.kt#formatBalance <- src/main/kotlin/bank/Transfer.kt#summary [exact]",
            "relation.changed_callee_caller@1 src/main/kotlin/bank/Account.kt#formatBalance <- src/main/kotlin/app/Main.kt#main [exact]",
        ],
    );
}

// ------------------------------------------------------ contract / safety

#[test]
fn runs_are_byte_deterministic() {
    let r = kotlin_repo();
    let a = review_run(&r, "kotlin", &r.commits[0], &r.commits[2]);
    let b = review_run(&r, "kotlin", &r.commits[0], &r.commits[2]);
    assert_eq!(a, b);
    assert!(a["run_id"].as_str().unwrap().starts_with("run:sha256:"));
}

#[test]
fn requests_are_refused_with_exit_3() {
    let r = rust_repo();
    let (b, t) = (&r.commits[0], &r.commits[1]);
    for bad in [
        format!(
            r#"{{"schema":"{SCHEMA}","language":"rust","base_revision":"{b}","target_revision":"{t}","extra":1}}"#
        ),
        format!(
            r#"{{"schema":"{SCHEMA}","language":"cobol","base_revision":"{b}","target_revision":"{t}"}}"#
        ),
        format!(
            r#"{{"schema":"{SCHEMA}","language":"rust","base_revision":"--output=x","target_revision":"{t}"}}"#
        ),
        format!(
            r#"{{"schema":"{SCHEMA}","language":"rust","base_revision":"nope","target_revision":"{t}"}}"#
        ),
        format!(
            r#"{{"schema":"{SCHEMA}","language":"rust","base_revision":"{b}","target_revision":"{t}","limits":{{"max_files":0,"max_file_bytes":1,"max_total_source_bytes":1}}}}"#
        ),
    ] {
        let out = review(r.root(), &bad, ".rg6-bad");
        assert_eq!(out.code, 3, "{bad}: {}", out.stderr);
        assert!(out.stdout.is_empty());
        assert!(!r.root().join(".rg6-bad").exists());
    }
    // Not the repository top level: a repository-layout refusal (exit 20).
    let sub = r.root().join("src");
    let out = review(&sub, &request("rust", b, t), ".rg6-sub");
    assert_eq!(out.code, 20, "{}", out.stderr);
    assert!(out.stdout.is_empty());
}

#[test]
fn existing_artifact_root_is_refused() {
    let r = rust_repo();
    fs::create_dir(r.root().join(".rg6-existing")).unwrap();
    let out = review(
        r.root(),
        &request("rust", &r.commits[0], &r.commits[1]),
        ".rg6-existing",
    );
    assert_eq!(out.code, 20, "{}", out.stderr);
    assert!(
        fs::read_dir(r.root().join(".rg6-existing"))
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn file_bound_is_reported_as_gap() {
    let mut r = rust_repo();
    let base = r.commits[2].clone();
    r.write(
        "src/big.rs",
        &format!("pub fn big() {{}}\n{}", "// pad\n".repeat(200)),
    );
    let target = r.commit();
    let req = format!(
        r#"{{"schema":"{SCHEMA}","language":"rust","base_revision":"{base}","target_revision":"{target}","limits":{{"max_files":100,"max_file_bytes":1000,"max_total_source_bytes":100000}}}}"#
    );
    let out = review(r.root(), &req, ".rg6-bound");
    assert_eq!(out.code, 0, "{}", out.stderr);
    let run: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(run["authority"]["result_status"], "incomplete");
    assert!(
        rows(&run).contains(&"capability_gap.unanalyzed_source@1 src/big.rs <- target".to_owned())
    );
    // Removed file: base-only declarations are reported removed.
    r.remove("src/big.rs");
    let t2 = r.commit();
    let run = run_ok(&r, &target, &t2);
    assert!(
        rows(&run).contains(&"node.removed_public_callable@1 src/big.rs#big".to_owned()),
        "{run:#}"
    );
}

fn run_ok(r: &Repo, base: &str, target: &str) -> Value {
    review_run(r, "rust", base, target)
}

// ------------------------------------------- review regressions (B*, M*, m*)

fn edges(run: &Value) -> Vec<String> {
    run["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["rule_id"] == "relation.changed_callee_caller@1")
        .map(|o| {
            format!(
                "{} <- {} [{}]",
                o["target_key"][0].as_str().unwrap(),
                o["target_key"][1].as_str().unwrap(),
                o["subject"]["resolution"]["confidence"].as_str().unwrap()
            )
        })
        .collect()
}

#[test]
fn b1_paths_with_spaces_and_non_ascii_are_not_dropped() {
    let mut r = Repo::new();
    for p in ["src/a b.rs", "src/é.rs", "src/plain.rs"] {
        r.write(p, "pub fn h() -> u8 { 1 }\n");
    }
    let base = r.commit();
    for p in ["src/a b.rs", "src/é.rs", "src/plain.rs"] {
        r.write(p, "pub fn h() -> u8 { 2 }\n");
    }
    let target = r.commit();
    let run = review_run(&r, "rust", &base, &target);
    let changed: Vec<&str> = run["changed_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(changed, ["src/a b.rs", "src/plain.rs", "src/é.rs"]);
    assert_eq!(run["changed_callables"].as_array().unwrap().len(), 3);
    assert_eq!(run["authority"]["result_status"], "deferred");
}

#[test]
fn b2_same_named_function_in_another_crate_is_not_exact() {
    let mut r = Repo::new();
    r.write(
        "crates/a/src/lib.rs",
        "pub fn helper() -> u8 { 1 }\npub fn uses_helper() -> u8 { helper() }\n",
    );
    r.write("crates/b/src/lib.rs", "pub fn helper() -> u8 { 1 }\n");
    let base = r.commit();
    r.write("crates/b/src/lib.rs", "pub fn helper() -> u8 { 2 }\n");
    let target = r.commit();
    let run = review_run(&r, "rust", &base, &target);
    assert!(edges(&run).is_empty(), "{:?}", edges(&run));
}

#[test]
fn b3_external_or_imported_types_do_not_bind_to_repo_types() {
    let mut r = Repo::new();
    r.write("src/lib.rs", "pub mod a;\npub mod b;\n");
    r.write(
        "src/a.rs",
        "pub fn mk() { let _ = std::io::Error::new(std::io::ErrorKind::Other, \"x\"); }\n",
    );
    r.write(
        "src/b.rs",
        "pub struct Error;\nimpl Error { pub fn new() -> Error { Error } }\n",
    );
    let base = r.commit();
    r.write(
        "src/b.rs",
        "pub struct Error;\nimpl Error { pub fn new() -> Error { let e = Error; e } }\n",
    );
    let target = r.commit();
    let rs = review_run(&r, "rust", &base, &target);
    assert!(edges(&rs).is_empty(), "{:?}", edges(&rs));

    let mut t = Repo::new();
    t.write(
        "src/boot.ts",
        "import { Router } from \"express\";\nexport function boot() { Router.use(); }\n",
    );
    t.write(
        "src/router.ts",
        "export class Router { static use() { return 1; } }\n",
    );
    let base = t.commit();
    t.write(
        "src/router.ts",
        "export class Router { static use() { return 2; } }\n",
    );
    let target = t.commit();
    let run = review_run(&t, "typescript", &base, &target);
    assert!(edges(&run).is_empty(), "{:?}", edges(&run));
}

#[test]
fn b4_kotlin_local_function_shadows_package_function() {
    let mut r = Repo::new();
    r.write("src/p/A.kt", "package p\nfun local() = 1\n");
    r.write(
        "src/p/U.kt",
        "package p\nclass User { fun q(): Int { fun local() = 7; return local() } }\nfun other() = local()\n",
    );
    let base = r.commit();
    r.write("src/p/A.kt", "package p\nfun local() = 2\n");
    let target = r.commit();
    let run = review_run(&r, "kotlin", &base, &target);
    assert_eq!(
        edges(&run),
        ["src/p/A.kt#local <- src/p/U.kt#other [exact]"]
    );
}

#[test]
fn m1_unparseable_target_file_reports_no_removals() {
    let mut r = Repo::new();
    r.write("src/lib.rs", "pub fn f() {}\npub fn g() {}\n");
    let base = r.commit();
    r.write("src/lib.rs", "pub fn f() {}\npub fn g( {}\n");
    let target = r.commit();
    let run = review_run(&r, "rust", &base, &target);
    assert!(run["removed_callables"].as_array().unwrap().is_empty());
    assert_eq!(run["authority"]["result_status"], "incomplete");
}

#[test]
fn m2_removed_overload_is_named_by_its_parameters() {
    let mut r = Repo::new();
    r.write(
        "src/A.kt",
        "package p\nfun over(a: Int) = 1\nfun over(a: String) = 2\n",
    );
    let base = r.commit();
    r.write("src/A.kt", "package p\nfun over(a: String) = 2\n");
    let target = r.commit();
    let run = review_run(&r, "kotlin", &base, &target);
    assert_rows(
        &run,
        &["node.removed_public_callable@1 src/A.kt#over((a: Int))"],
    );
}

#[test]
fn m3_calls_outside_function_bodies_have_initializer_callers() {
    let mut r = Repo::new();
    r.write("src/t.ts", "export function target() { return 1; }\n");
    r.write(
        "src/u.ts",
        "import { target } from \"./t\";\nexport const value = target();\nexport class C { x = target(); static { target(); } }\n",
    );
    let base = r.commit();
    r.write("src/t.ts", "export function target() { return 2; }\n");
    let target = r.commit();
    let run = review_run(&r, "typescript", &base, &target);
    let mut e = edges(&run);
    e.sort();
    assert_eq!(
        e,
        [
            "src/t.ts#target <- src/u.ts#<init> [exact]",
            "src/t.ts#target <- src/u.ts#C.<init> [exact]",
        ]
    );
}

#[test]
fn m4_typescript_barrel_reexport_resolves_exactly() {
    let mut r = Repo::new();
    r.write("src/impl.ts", "export function target() { return 1; }\n");
    r.write("src/index.ts", "export { target } from \"./impl\";\n");
    r.write(
        "src/use.ts",
        "import { target } from \"./index\";\nexport function user() { return target(); }\n",
    );
    let base = r.commit();
    r.write("src/impl.ts", "export function target() { return 2; }\n");
    let target = r.commit();
    let run = review_run(&r, "typescript", &base, &target);
    assert_eq!(
        edges(&run),
        ["src/impl.ts#target <- src/use.ts#user [exact]"]
    );
}

#[test]
fn m5_kotlin_call_in_receiver_lambda_is_not_exact() {
    let mut r = Repo::new();
    r.write(
        "src/S.kt",
        "package p\nclass Svc {\n  fun make() = Builder().apply { name(\"x\") }\n  fun name(s: String) = 1\n}\n",
    );
    let base = r.commit();
    r.write(
        "src/S.kt",
        "package p\nclass Svc {\n  fun make() = Builder().apply { name(\"x\") }\n  fun name(s: String) = 2\n}\n",
    );
    let target = r.commit();
    let run = review_run(&r, "kotlin", &base, &target);
    assert_eq!(
        edges(&run),
        ["src/S.kt#Svc.name <- src/S.kt#Svc.make [name_only]"]
    );
}

#[test]
fn m2b_public_to_private_is_a_contract_change() {
    let mut r = Repo::new();
    r.write("src/lib.rs", "pub fn api() {}\n");
    let base = r.commit();
    r.write("src/lib.rs", "fn api() {}\n");
    let target = r.commit();
    let run = review_run(&r, "rust", &base, &target);
    assert_rows(
        &run,
        &["node.changed_public_callable_contract@1 src/lib.rs#api"],
    );
    assert_eq!(
        run["changed_callables"][0]["change"]["visibility_changed"],
        true
    );
}

#[test]
fn m1b_unanalyzed_in_both_snapshots_has_distinct_obligation_ids() {
    let mut r = Repo::new();
    r.write("src/lib.rs", "pub fn f( {}\n");
    let base = r.commit();
    r.write("src/lib.rs", "pub fn f( {} \n");
    let target = r.commit();
    let run = review_run(&r, "rust", &base, &target);
    let ids: Vec<&str> = run["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].as_str().unwrap())
        .collect();
    let mut dedup = ids.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(ids.len(), dedup.len());
    assert_eq!(ids.len(), 2, "{run:#}");
}

// ------------------------------------------------- review round 2 (N*)

fn changed_names(run: &Value) -> Vec<String> {
    let mut v: Vec<String> = run["changed_callables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["callable"]["key"].as_str().unwrap().to_owned())
        .collect();
    v.sort();
    v
}

fn two_commits(files0: &[(&str, &str)], files1: &[(&str, &str)]) -> (Repo, String, String) {
    let mut r = Repo::new();
    for (p, t) in files0 {
        r.write(p, t);
    }
    let a = r.commit();
    for (p, t) in files1 {
        r.write(p, t);
    }
    let b = r.commit();
    (r, a, b)
}

#[test]
fn n1_kotlin_member_and_lambda_calls_to_top_level_are_not_exact() {
    let (r, a, b) = two_commits(
        &[
            (
                "src/p/H.kt",
                "package p\nfun helper() = 1\nfun append(s: String) = 1\n",
            ),
            (
                "src/p/S.kt",
                "package p\nclass Sub : Base() { fun use() = helper() }\nfun Base.ext() = helper()\nfun top() = helper()\nfun b(sb: StringBuilder) = sb.apply { append(\"x\") }\n",
            ),
        ],
        &[(
            "src/p/H.kt",
            "package p\nfun helper() = 2\nfun append(s: String) = 2\n",
        )],
    );
    let run = review_run(&r, "kotlin", &a, &b);
    let mut e = edges(&run);
    e.sort();
    assert_eq!(
        e,
        [
            "src/p/H.kt#append <- src/p/S.kt#b [name_only]",
            "src/p/H.kt#helper <- src/p/S.kt#Base.ext [name_only]",
            "src/p/H.kt#helper <- src/p/S.kt#Sub.use [name_only]",
            "src/p/H.kt#helper <- src/p/S.kt#top [exact]",
        ]
    );
}

#[test]
fn n2_n3_n4_rust_scoping_is_not_overclaimed() {
    let (r, a, b) = two_commits(
        &[
            (
                "src/lib.rs",
                "pub mod other;\npub mod g;\npub mod util;\npub fn helper() -> u8 { 1 }\npub fn blockuse() -> u8 { use crate::other::helper; helper() }\n",
            ),
            (
                "src/other.rs",
                "pub fn helper() -> u8 { 7 }\npub struct T;\nimpl T { pub fn make() -> u8 { 7 } }\n",
            ),
            (
                "src/g.rs",
                "use crate::other::*;\nmod inner { pub struct T; impl T { pub fn make() -> u8 { 1 } } }\npub fn call() -> u8 { T::make() }\n",
            ),
            ("src/util.rs", "pub fn u() -> u8 { 1 }\n"),
            (
                "src/bin/tool.rs",
                "mod util;\nfn main() { crate::util::u(); }\n",
            ),
            ("src/bin/util.rs", "pub fn u() -> u8 { 2 }\n"),
        ],
        &[
            (
                "src/lib.rs",
                "pub mod other;\npub mod g;\npub mod util;\npub fn helper() -> u8 { 2 }\npub fn blockuse() -> u8 { use crate::other::helper; helper() }\n",
            ),
            (
                "src/g.rs",
                "use crate::other::*;\nmod inner { pub struct T; impl T { pub fn make() -> u8 { 2 } } }\npub fn call() -> u8 { T::make() }\n",
            ),
            ("src/util.rs", "pub fn u() -> u8 { 3 }\n"),
        ],
    );
    let run = review_run(&r, "rust", &a, &b);
    for e in edges(&run) {
        assert!(!e.ends_with("[exact]"), "false exact: {e}");
    }
    assert!(
        !edges(&run).iter().any(|e| e.contains("blockuse")),
        "{:?}",
        edges(&run)
    );
    assert!(
        !edges(&run).iter().any(|e| e.contains("bin/tool")),
        "{:?}",
        edges(&run)
    );
}

#[test]
fn n4b_n4c_typescript_this_rebinding_and_mjs_resolution() {
    let (r, a, b) = two_commits(
        &[
            (
                "src/c.ts",
                "export class C {\n  m() { return 1; }\n  run() { const o = { go() { return this.m(); } }; return this.m() + o.go(); }\n}\n",
            ),
            ("src/x.ts", "export function f() { return 1; }\n"),
            ("src/x.mts", "export function f() { return 1; }\n"),
            (
                "src/u.ts",
                "import { f } from \"./x.mjs\";\nexport function user() { return f(); }\n",
            ),
        ],
        &[
            (
                "src/c.ts",
                "export class C {\n  m() { return 2; }\n  run() { const o = { go() { return this.m(); } }; return this.m() + o.go(); }\n}\n",
            ),
            ("src/x.ts", "export function f() { return 2; }\n"),
        ],
    );
    let run = review_run(&r, "typescript", &a, &b);
    let mut e = edges(&run);
    e.sort();
    // `this.m()` in the class method is exact; the object-literal one is
    // only a method-name match (both on line 3, one edge); `user` imports
    // x.mts, not x.ts, so it is no caller of the changed x.ts#f.
    assert_eq!(e, ["src/c.ts#C.m <- src/c.ts#C.run [exact]"]);
    let o = run["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["rule_id"] == "relation.changed_callee_caller@1")
        .unwrap();
    assert_eq!(
        o["subject"]["resolution"]["kinds"],
        serde_json::json!(["method_name", "self_receiver"])
    );
}

#[test]
fn n5_typescript_namespaces_are_analyzed_and_scoped() {
    let (r, a, b) = two_commits(
        &[(
            "src/n.ts",
            "namespace M { export function h() { return 1; } function priv() { return h(); } }\nexport namespace N { export function g() { return 1; } }\nmodule Q { export function k() { return 1; } }\nexport function use1() { return N.g(); }\n",
        )],
        &[(
            "src/n.ts",
            "namespace M { export function h() { return 2; } function priv() { return h(); } }\nexport namespace N { export function g() { return 2; } }\nmodule Q { export function k() { return 2; } }\nexport function use1() { return N.g(); }\n",
        )],
    );
    let run = review_run(&r, "typescript", &a, &b);
    assert_eq!(
        changed_names(&run),
        ["src/n.ts#g", "src/n.ts#h", "src/n.ts#k", "src/n.ts#priv"]
    );
    let mut e = edges(&run);
    e.sort();
    assert_eq!(
        e,
        [
            "src/n.ts#g <- src/n.ts#use1 [exact]",
            "src/n.ts#h <- src/n.ts#priv [exact]"
        ]
    );
    // Only N (exported) publishes its members.
    assert_rows_contains(&run, "node.changed_public_callable_contract@1 src/n.ts#g");
    assert!(!rows(&run).contains(&"node.changed_public_callable_contract@1 src/n.ts#k".to_owned()));
    assert!(!rows(&run).contains(&"node.changed_public_callable_contract@1 src/n.ts#h".to_owned()));
}

fn assert_rows_contains(run: &Value, row: &str) {
    assert!(
        rows(run).contains(&row.to_owned()),
        "{row} not in {:?}",
        rows(run)
    );
}

#[test]
fn n6_kotlin_default_arguments() {
    let (r, a, b) = two_commits(
        &[
            ("src/T.kt", "package p\nfun t() = 1\n"),
            (
                "src/K.kt",
                "package p\nfun h(x: Int = t()) = x\nfun k(cb: () -> Int = { t() }) { val y = t() }\n",
            ),
        ],
        &[("src/T.kt", "package p\nfun t() = 2\n")],
    );
    let run = review_run(&r, "kotlin", &a, &b);
    let mut e = edges(&run);
    e.sort();
    // `k` calls `t` exactly in its body; the default-value lambda call is
    // only name-level, and the edge carries the strongest site.
    assert_eq!(
        e,
        [
            "src/T.kt#t <- src/K.kt#h [exact]",
            "src/T.kt#t <- src/K.kt#k [exact]"
        ]
    );
    let k = run["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["target_key"][1] == "src/K.kt#k")
        .unwrap();
    assert_eq!(k["subject"]["call_site_lines"], serde_json::json!([3]));
    let kinds = &k["subject"]["resolution"]["kinds"];
    assert!(kinds.as_array().unwrap().len() == 2, "{kinds}");
}

#[test]
fn n7_unknown_base_side_reports_unknown_change_details() {
    let (r, a, b) = two_commits(
        &[("src/lib.rs", "pub fn f( {}\npub fn old(a: u8) {}\n")],
        &[(
            "src/lib.rs",
            "pub fn f() {}\npub fn old(a: u16) {}\npub fn newf() {}\n",
        )],
    );
    let run = review_run(&r, "rust", &a, &b);
    for c in run["changed_callables"].as_array().unwrap() {
        assert_eq!(c["change"]["status"], "changed", "{c:#}");
        assert_eq!(c["change"]["base_analyzed"], false);
        assert!(c["change"]["signature_changed"].is_null());
    }
    assert_rows_contains(
        &run,
        "capability_gap.unanalyzed_source@1 src/lib.rs <- base",
    );
}

#[test]
fn n8_rust_impls_for_slices_and_tuples() {
    let (r, a, b) = two_commits(
        &[(
            "src/lib.rs",
            "pub trait Tr { fn go(&self) -> u8; }\npub fn helper() -> u8 { 1 }\nimpl Tr for [u8] { fn go(&self) -> u8 { helper() } }\nimpl<T> Tr for (T, T) { fn go(&self) -> u8 { 1 } }\n",
        )],
        &[(
            "src/lib.rs",
            "pub trait Tr { fn go(&self) -> u8; }\npub fn helper() -> u8 { 2 }\nimpl Tr for [u8] { fn go(&self) -> u8 { helper() } }\nimpl<T> Tr for (T, T) { fn go(&self) -> u8 { 2 } }\n",
        )],
    );
    let run = review_run(&r, "rust", &a, &b);
    assert_eq!(
        changed_names(&run),
        ["src/lib.rs#(T , T).go", "src/lib.rs#helper"]
    );
    assert_eq!(
        edges(&run),
        ["src/lib.rs#helper <- src/lib.rs#[u8].go [exact]"]
    );
}

#[test]
fn n9_n10_symlink_and_mode_only_changes() {
    let mut r = Repo::new();
    r.write("src/a.rs", "pub fn gone() {}\n");
    r.write("src/m.rs", "pub fn m() {}\n");
    let a = r.commit();
    r.remove("src/a.rs");
    std::os::unix::fs::symlink("m.rs", r.root().join("src/a.rs")).unwrap();
    git(r.root(), &["update-index", "--chmod=+x", "src/m.rs"]);
    let b = r.commit();
    let run = review_run(&r, "rust", &a, &b);
    assert_rows_contains(
        &run,
        "capability_gap.unanalyzed_source@1 src/a.rs <- target",
    );
    assert!(
        !rows(&run).iter().any(|x| x.contains("src/m.rs")),
        "{:?}",
        rows(&run)
    );
}

#[test]
fn n11_huge_expressions_do_not_crash() {
    let big = format!(
        "package p\nfun a() = 1\nfun x() = {}\n",
        vec!["a()"; 30_000].join(" + ")
    );
    let (r, a, b) = two_commits(&[("src/X.kt", "package p\n")], &[("src/X.kt", &big)]);
    let kt = review_run(&r, "kotlin", &a, &b);
    assert!(!kt["changed_callables"].as_array().unwrap().is_empty());
    let rbig = format!(
        "pub fn a() -> u8 {{ 1 }}\npub fn x() -> u32 {{ {} }}\n",
        vec!["a() as u32"; 30_000].join(" + ")
    );
    let (r, a, b) = two_commits(&[("src/lib.rs", "\n")], &[("src/lib.rs", &rbig)]);
    let rs = review_run(&r, "rust", &a, &b);
    assert!(!rs["changed_callables"].as_array().unwrap().is_empty());
}

#[test]
fn m4b_default_import_and_barrel_class() {
    let (r, a, b) = two_commits(
        &[
            (
                "src/impl.ts",
                "export default function dflt() { return 1; }\nexport class W { constructor(x: number) {} }\n",
            ),
            ("src/index.ts", "export * from \"./impl\";\n"),
            (
                "src/use.ts",
                "import d from \"./impl\";\nimport { W } from \"./index\";\nexport function user() { new W(1); return d(); }\n",
            ),
        ],
        &[(
            "src/impl.ts",
            "export default function dflt() { return 2; }\nexport class W { constructor(x: number) { void x; } }\n",
        )],
    );
    let run = review_run(&r, "typescript", &a, &b);
    let mut e = edges(&run);
    e.sort();
    assert_eq!(
        e,
        [
            "src/impl.ts#W.constructor <- src/use.ts#user [exact]",
            "src/impl.ts#dflt <- src/use.ts#user [exact]",
        ]
    );
}

#[test]
fn rust_type_qualified_method_calls_are_name_level() {
    let (r, a, b) = two_commits(
        &[
            ("src/lib.rs", "pub mod m;\npub mod n;\n"),
            (
                "src/m.rs",
                "pub struct T;\nimpl T { pub fn make() -> u8 { 1 } }\npub fn here() -> u8 { T::make() }\n",
            ),
            (
                "src/n.rs",
                "use crate::m::T;\npub fn there() -> u8 { T::make() }\n",
            ),
        ],
        &[(
            "src/m.rs",
            "pub struct T;\nimpl T { pub fn make() -> u8 { 2 } }\npub fn here() -> u8 { T::make() }\n",
        )],
    );
    let run = review_run(&r, "rust", &a, &b);
    let mut e = edges(&run);
    e.sort();
    assert_eq!(
        e,
        [
            "src/m.rs#T.make <- src/m.rs#here [name_only]",
            "src/m.rs#T.make <- src/n.rs#there [name_only]",
        ]
    );
}

// ------------------------------------------------- review round 3

#[test]
fn r3_rust_method_calls_are_never_exact() {
    let (r, a, b) = two_commits(
        &[
            ("src/lib.rs", "pub mod model;\npub mod ext;\npub mod gen;\n"),
            (
                "src/model.rs",
                "pub trait T { fn a(&self) -> u8; fn b(&self) -> u8; }\npub struct Foo;\nimpl T for Foo { fn a(&self) -> u8 { self.b() + Self::b(self) } fn b(&self) -> u8 { 1 } }\n",
            ),
            (
                "src/ext.rs",
                "impl crate::model::Foo { pub fn b(&self) -> u8 { 22 } }\n",
            ),
            (
                "src/gen.rs",
                "pub trait Load { fn load() -> u32; }\npub struct Config;\nimpl Config { pub fn load() -> u32 { 1 } }\npub fn process<Config: Load>() -> u32 { Config::load() }\n",
            ),
        ],
        &[
            (
                "src/model.rs",
                "pub trait T { fn a(&self) -> u8; fn b(&self) -> u8; }\npub struct Foo;\nimpl T for Foo { fn a(&self) -> u8 { self.b() + Self::b(self) } fn b(&self) -> u8 { 2 } }\n",
            ),
            (
                "src/ext.rs",
                "impl crate::model::Foo { pub fn b(&self) -> u8 { 23 } }\n",
            ),
            (
                "src/gen.rs",
                "pub trait Load { fn load() -> u32; }\npub struct Config;\nimpl Config { pub fn load() -> u32 { 2 } }\npub fn process<Config: Load>() -> u32 { Config::load() }\n",
            ),
        ],
    );
    let run = review_run(&r, "rust", &a, &b);
    assert!(!edges(&run).is_empty());
    for e in edges(&run) {
        assert!(!e.ends_with("[exact]"), "false exact: {e}");
    }
    // The inherent ext.rs#Foo.b is linked (as an ambiguous candidate).
    assert!(
        edges(&run)
            .iter()
            .any(|e| e.starts_with("src/ext.rs#Foo.b <- src/model.rs#Foo.a")),
        "{:?}",
        edges(&run)
    );
}

#[test]
fn r3_local_heads_do_not_resolve_as_imports() {
    let (r, a, b) = two_commits(
        &[
            ("src/lib.rs", "pub mod m;\npub mod other;\n"),
            (
                "src/m.rs",
                "pub mod a { pub fn f() -> u8 { 1 } }\npub fn g() -> u8 { use crate::other as a; a::f() }\n",
            ),
            ("src/other.rs", "pub fn f() -> u8 { 2 }\n"),
        ],
        &[(
            "src/m.rs",
            "pub mod a { pub fn f() -> u8 { 3 } }\npub fn g() -> u8 { use crate::other as a; a::f() }\n",
        )],
    );
    let run = review_run(&r, "rust", &a, &b);
    assert!(
        !edges(&run).iter().any(|e| e.contains("#g")),
        "{:?}",
        edges(&run)
    );

    let (t, a, b) = two_commits(
        &[
            ("src/cfg.ts", "export function get() { return 1; }\n"),
            (
                "src/main.ts",
                "import * as cfg from \"./cfg\";\nexport function c5(cfg: { get(): number }) { return cfg.get(); }\nexport function c6() { return cfg.get(); }\n",
            ),
        ],
        &[("src/cfg.ts", "export function get() { return 2; }\n")],
    );
    let run = review_run(&t, "typescript", &a, &b);
    assert_eq!(edges(&run), ["src/cfg.ts#get <- src/main.ts#c6 [exact]"]);
}

#[test]
fn r3_kotlin_labelled_this_is_not_own_receiver() {
    let (r, a, b) = two_commits(
        &[(
            "src/O.kt",
            "package p\nclass Outer {\n  fun m() = 1\n  inner class Inner { fun m() = 2\n    fun g() = this@Outer.m() }\n}\n",
        )],
        &[(
            "src/O.kt",
            "package p\nclass Outer {\n  fun m() = 1\n  inner class Inner { fun m() = 3\n    fun g() = this@Outer.m() }\n}\n",
        )],
    );
    let run = review_run(&r, "kotlin", &a, &b);
    for e in edges(&run) {
        assert!(!e.ends_with("[exact]"), "false exact: {e}");
    }
}

#[test]
fn r3_rust_paths_not_followed_fall_back_to_name_level() {
    let (r, a, b) = two_commits(
        &[
            (
                "src/lib.rs",
                "pub mod model;\npub mod ext;\npub mod real;\npub mod facade;\npub mod user;\n",
            ),
            ("src/model.rs", "pub struct Foo;\n"),
            (
                "src/ext.rs",
                "impl crate::model::Foo { pub fn helper() -> u8 { 1 } }\n",
            ),
            ("src/real.rs", "pub fn run() -> u8 { 1 }\n"),
            ("src/facade.rs", "pub use crate::real::run;\n"),
            (
                "src/user.rs",
                "use crate::model::Foo;\npub fn u1() -> u8 { Foo::helper() }\npub fn u2() -> u8 { crate::facade::run() }\n",
            ),
        ],
        &[
            (
                "src/ext.rs",
                "impl crate::model::Foo { pub fn helper() -> u8 { 2 } }\n",
            ),
            ("src/real.rs", "pub fn run() -> u8 { 2 }\n"),
        ],
    );
    let run = review_run(&r, "rust", &a, &b);
    let mut e = edges(&run);
    e.sort();
    assert_eq!(
        e,
        [
            "src/ext.rs#Foo.helper <- src/user.rs#u1 [name_only]",
            "src/real.rs#run <- src/user.rs#u2 [name_only]",
        ]
    );
}

#[test]
fn r3_typescript_local_export_aliases_and_default_class() {
    let (r, a, b) = two_commits(
        &[
            (
                "src/a.ts",
                "function inner() { return 1; }\nfunction helper2() { return 1; }\nexport { inner as renamed };\nexport default helper2;\n",
            ),
            (
                "src/klass.ts",
                "export default class Foo { static make() { return 1; } }\n",
            ),
            (
                "src/u.ts",
                "import d, { renamed } from \"./a\";\nimport Foo from \"./klass\";\nexport function u() { return renamed() + d() + Foo.make(); }\n",
            ),
        ],
        &[
            (
                "src/a.ts",
                "function inner() { return 2; }\nfunction helper2() { return 2; }\nexport { inner as renamed };\nexport default helper2;\n",
            ),
            (
                "src/klass.ts",
                "export default class Foo { static make() { return 2; } }\n",
            ),
        ],
    );
    let run = review_run(&r, "typescript", &a, &b);
    let mut e = edges(&run);
    e.sort();
    assert_eq!(
        e,
        [
            "src/a.ts#helper2 <- src/u.ts#u [exact]",
            "src/a.ts#inner <- src/u.ts#u [exact]",
            "src/klass.ts#Foo.make <- src/u.ts#u [exact]",
        ]
    );
}

// ------------------------------------------------- review round 4

#[test]
fn r4_typescript_loop_bindings_named_function_expressions_and_scripts() {
    let (r, a, b) = two_commits(
        &[
            (
                "src/loc.ts",
                "export function f() { return 1; }\nexport function a1(arr: any[]) { for (const f of arr) f(); }\nexport function a11() { const g = function f() { return f(); }; return g; }\nexport function a2() { return f(); }\n",
            ),
            (
                "src/n1.ts",
                "namespace M { export function f() { return 1; } }\n",
            ),
            (
                "src/n2.ts",
                "function f() { return 2; }\nnamespace M { export function g() { return f(); } }\n",
            ),
        ],
        &[
            (
                "src/loc.ts",
                "export function f() { return 3; }\nexport function a1(arr: any[]) { for (const f of arr) f(); }\nexport function a11() { const g = function f() { return f(); }; return g; }\nexport function a2() { return f(); }\n",
            ),
            (
                "src/n2.ts",
                "function f() { return 4; }\nnamespace M { export function g() { return f(); } }\n",
            ),
        ],
    );
    let run = review_run(&r, "typescript", &a, &b);
    let mut e = edges(&run);
    e.sort();
    assert_eq!(
        e,
        [
            "src/loc.ts#f <- src/loc.ts#a2 [exact]",
            "src/n2.ts#f <- src/n2.ts#g [name_only]",
        ]
    );
}

#[test]
fn r4_kotlin_companion_and_extension_property() {
    let (r, a, b) = two_commits(
        &[
            (
                "src/p/a.kt",
                "package p\nfun f() = 1\nclass Outer {\n    fun f() = 2\n    fun t() = 3\n    companion object Named {\n        val x = f()\n        fun h() = f()\n        fun k() = this.t()\n    }\n}\n",
            ),
            ("src/p/b.kt", "package p\nval Outer.z: Int get() = f()\n"),
        ],
        &[(
            "src/p/a.kt",
            "package p\nfun f() = 10\nclass Outer {\n    fun f() = 20\n    fun t() = 30\n    companion object Named {\n        val x = f()\n        fun h() = f()\n        fun k() = this.t()\n    }\n}\n",
        )],
    );
    let run = review_run(&r, "kotlin", &a, &b);
    for e in edges(&run) {
        assert!(!e.ends_with("[exact]"), "false exact: {e}");
    }
    // The companion's calls reach the top-level `f` (by name), and the
    // extension property getter is a caller too.
    let e = edges(&run);
    assert!(
        e.iter()
            .any(|x| x.starts_with("src/p/a.kt#f <- src/p/a.kt#Outer.h")),
        "{e:?}"
    );
    assert!(
        e.iter()
            .any(|x| x.starts_with("src/p/a.kt#f <- src/p/b.kt#Outer.<init>")),
        "{e:?}"
    );
}
