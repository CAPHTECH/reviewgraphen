use super::*;

fn names(facts: &FileFacts) -> Vec<String> {
    facts
        .callables
        .iter()
        .filter(|c| c.kind != CallableKind::Initializer)
        .map(|c| {
            format!(
                "{}:{}:{}:{}-{}",
                c.qualified(),
                c.kind.as_str(),
                if c.public { "pub" } else { "priv" },
                c.start_line,
                c.end_line
            )
        })
        .collect()
}

fn calls(facts: &FileFacts) -> Vec<String> {
    facts
        .calls
        .iter()
        .map(|c| {
            format!(
                "{}->{}:{:?}@{}",
                facts.callables[c.caller].qualified(),
                c.name,
                c.form,
                c.line
            )
        })
        .collect()
}

#[test]
fn rust_methods_traits_and_calls() {
    let src = r#"use crate::util::helper as h;
pub struct A;
impl A {
    pub fn run(&self) -> u8 { self.step(); h(); crate::b::go(); A::make(); 1 }
    fn step(&self) { println!("{}", compute(2)); }
    pub fn make() -> A { A }
}
impl Clone for A { fn clone(&self) -> A { A } }
pub trait T { fn req(&self); fn def(&self) { self.req() } }
fn compute(x: u8) -> u8 { x }
mod inner { pub fn nested() {} }
"#;
    let f = rust_lang::extract("src/a.rs", src, |_| false).unwrap();
    assert_eq!(
        names(&f),
        [
            "A.run:method:pub:4-4",
            "A.step:method:priv:5-5",
            "A.make:method:pub:6-6",
            "A.clone:method:pub:8-8",
            "T.req:method:pub:9-9",
            "T.def:method:pub:9-9",
            "compute:function:priv:10-10",
            "nested:function:pub:11-11",
        ]
    );
    assert_eq!(f.callables[7].scope, "crate::a::inner");
    let c = calls(&f);
    assert!(
        c.contains(&"A.run->step:Method { self_receiver: true }@4".to_owned()),
        "{c:?}"
    );
    assert!(c.contains(&"A.run->h:Plain@4".to_owned()), "{c:?}");
    assert!(
        c.iter().any(|s| s.starts_with("A.run->go:Qualified")),
        "{c:?}"
    );
    assert!(
        c.iter()
            .any(|s| s.starts_with("A.run->make:Qualified([\"A\"])")),
        "{c:?}"
    );
    assert!(
        c.contains(&"A.step->compute:Plain@5".to_owned()),
        "macro args: {c:?}"
    );
    assert!(
        c.contains(&"T.def->req:Method { self_receiver: true }@9".to_owned()),
        "{c:?}"
    );
}

#[test]
fn rust_module_paths() {
    assert_eq!(rust_lang::module_of("src/lib.rs", |_| false), "crate");
    assert_eq!(rust_lang::module_of("src/a/mod.rs", |_| false), "crate::a");
    assert_eq!(
        rust_lang::module_of("crates/x/src/a/b.rs", |_| false),
        "crate@crates/x::a::b"
    );
    assert_eq!(
        rust_lang::module_of("crates/x/src/lib.rs", |_| false),
        "crate@crates/x"
    );
    assert_eq!(
        rust_lang::module_of("tests/it.rs", |_| false),
        "crate@tests/it"
    );
    assert_eq!(
        rust_lang::module_of("src/bin/tool.rs", |_| false),
        "crate#bin/tool"
    );
    assert_eq!(
        rust_lang::module_of("src/bin/tool/util.rs", |_| false),
        "crate#bin/tool::util"
    );
    assert_eq!(
        rust_lang::module_of("src/main.rs", |p| p == "src/lib.rs"),
        "crate#main"
    );
    assert_eq!(rust_lang::module_of("src/main.rs", |_| false), "crate");
}

#[test]
fn typescript_classes_exports_and_imports() {
    let src = r#"import { f as g, h } from "./m";
import * as ns from "../n";
export class A {
  run(): void { this.step(); g(); ns.k(); other.m(); B.stat(); }
  private step() {}
  #hidden() {}
  handler = () => { h(); };
}
class B { static stat() {} }
export const arrow = (x: number) => x + 1;
function local() { arrow(1); }
export { B };
"#;
    let f = typescript_lang::extract("src/a.ts", src).unwrap();
    assert_eq!(
        names(&f),
        [
            "A.run:method:pub:4-4",
            "A.step:method:priv:5-5",
            "A.#hidden:method:priv:6-6",
            "A.handler:method:pub:7-7",
            "B.stat:method:pub:9-9",
            "arrow:function:pub:10-10",
            "local:function:priv:11-11",
        ]
    );
    let c = calls(&f);
    for want in [
        "A.run->step:Method { self_receiver: true }@4",
        "A.run->g:Plain@4",
        "A.run->k:Qualified([\"ns\"])@4",
        "A.run->m:Qualified([\"other\"])@4",
        "A.run->stat:Qualified([\"B\"])@4",
        "A.handler->h:Plain@7",
        "local->arrow:Plain@11",
    ] {
        assert!(c.contains(&want.to_owned()), "missing {want}: {c:?}");
    }
    assert_eq!(f.imports.len(), 3);
}

#[test]
fn typescript_syntax_error_lines_are_reported_and_other_facts_kept() {
    let src = "export function ok() { g(); }\nconst x = sql<{ a: string }>`q`.run();\n";
    let f = typescript_lang::extract("a.ts", src).unwrap();
    assert_eq!(f.syntax_error_lines, vec![2]);
    assert_eq!(names(&f), ["ok:function:pub:1-1"]);
    let clean = typescript_lang::extract("a.ts", "export function ok() {}\n").unwrap();
    assert!(clean.syntax_error_lines.is_empty());
}

#[test]
fn kotlin_classes_extensions_and_calls() {
    let src = r#"package bank.core

import bank.util.fmt
import bank.util.Other as O

class Account(var balance: Long) {
    fun withdraw(amount: Long): Boolean {
        if (check(amount)) { log("x") }
        return this.apply(amount)
    }
    private fun check(a: Long) = a > 0
    internal fun apply(a: Long): Boolean = true
    companion object {
        fun create(): Account = Account(0)
    }
}
fun Account.describe(): String = fmt(balance) + O.x()
private fun hidden() = Account.create().withdraw(1)
"#;
    let f = kotlin::extract("src/main/kotlin/bank/core/Account.kt", src).unwrap();
    assert_eq!(
        names(&f),
        [
            "Account.withdraw:method:pub:7-10",
            "Account.check:method:priv:11-11",
            "Account.apply:method:priv:12-12",
            "Account.create:method:pub:14-14",
            "Account.describe:method:pub:17-17",
            "hidden:function:priv:18-18",
        ]
    );
    assert_eq!(f.callables[0].scope, "bank.core");
    let c = calls(&f);
    for want in [
        "Account.withdraw->check:Plain@8",
        "Account.withdraw->log:Plain@8",
        "Account.withdraw->apply:Method { self_receiver: true }@9",
        "Account.describe->fmt:Plain@17",
        "Account.describe->x:Qualified([\"O\"])@17",
        "hidden->create:Qualified([\"Account\"])@18",
        "hidden->withdraw:Method { self_receiver: false }@18",
    ] {
        assert!(c.contains(&want.to_owned()), "missing {want}: {c:?}");
    }
}

#[test]
fn diff_parsing_and_touch() {
    let raw = b"diff --git a/x.rs b/x.rs\nindex 1..2 100644\n--- a/x.rs\n+++ b/x.rs\n@@ -3 +3 @@\n-a\n+b\n@@ -9,2 +8,0 @@\n-c\n-d\ndiff --git a/n.rs b/n.rs\nnew file mode 100644\n--- /dev/null\n+++ b/n.rs\n@@ -0,0 +1,2 @@\n+x\n+y\ndiff --git a/g.rs b/g.rs\ndeleted file mode 100644\n--- a/g.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-z\n";
    let d = parse_diff(raw);
    let x = &d["x.rs"];
    assert_eq!(x.ranges, vec![(3, 3)]);
    assert_eq!(x.deletion_points, vec![8]);
    assert!(x.touches(1, 3));
    assert!(x.touches(8, 9), "deletion inside 8..9");
    assert!(!x.touches(9, 12), "deletion after line 8 is before 9..");
    assert!(
        !x.touches(4, 8),
        "deletion after the last line of 4..8 is outside"
    );
    assert!(d["n.rs"].added_file);
    assert!(d["g.rs"].deleted_file);
}

/// Diagnostic: `RG6_TS_FILE=<path> cargo test -- --ignored ts_error_sites`.
#[test]
#[ignore = "diagnostic helper"]
fn ts_error_sites() {
    let path = std::env::var("RG6_TS_FILE").unwrap();
    let src = std::fs::read_to_string(&path).unwrap();
    let mut parser = tree_sitter::Parser::new();
    let lang = if path.ends_with(".tsx") {
        tree_sitter_typescript::LANGUAGE_TSX
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT
    };
    parser.set_language(&lang.into()).unwrap();
    let tree = parser.parse(&src, None).unwrap();
    let mut stack = vec![tree.root_node()];
    while let Some(n) = stack.pop() {
        if n.is_error() || n.is_missing() {
            let line = src.lines().nth(n.start_position().row).unwrap_or("");
            eprintln!(
                "{} {}:{} {:?}",
                n.kind(),
                n.start_position().row + 1,
                n.start_position().column,
                line.trim()
            );
            continue;
        }
        for i in 0..n.child_count() {
            stack.push(n.child(i).unwrap());
        }
    }
}

#[test]
fn diff_paths_quoted_tabbed_and_content_lookalikes() {
    let raw = "diff --git a/a b.rs b/a b.rs\n--- a/a b.rs\t\n+++ b/a b.rs\t\n@@ -1 +1 @@\n--- a/fake.rs\n+++ b/fake.rs\ndiff --git \"a/\\303\\251.rs\" \"b/\\303\\251.rs\"\n--- \"a/\\303\\251.rs\"\n+++ \"b/\\303\\251.rs\"\n@@ -2,0 +3,2 @@\n+x\n+y\n";
    let d = parse_diff(raw.as_bytes());
    assert_eq!(d.keys().cloned().collect::<Vec<_>>(), ["a b.rs", "é.rs"]);
    assert_eq!(d["a b.rs"].ranges, vec![(1, 1)]);
    assert_eq!(d["é.rs"].ranges, vec![(3, 4)]);
    let ns = parse_name_status(b"M\0a b.rs\0A\0\xc3\xa9.rs\0");
    assert_eq!(ns["é.rs"], 'A');
}

#[test]
fn locals_shadow_and_nested_functions_attribute_to_enclosing() {
    let rs = rust_lang::extract(
        "src/a.rs",
        "pub fn outer(cb: fn()) { fn inner() { changed(); } let helper = 1; cb(); inner(); }\n",
        |_| false,
    )
    .unwrap();
    assert_eq!(names(&rs), ["outer:function:pub:1-1"]);
    let l = &rs.locals[0];
    for n in ["cb", "inner", "helper"] {
        assert!(l.contains(n), "{n} in {l:?}");
    }
    assert!(calls(&rs).contains(&"outer->changed:Plain@1".to_owned()));

    let kt = kotlin::extract(
        "a.kt",
        "package p\nclass User { fun q(x: Int): Int { fun local() = 7; return local() + x } }\nval top = compute()\n",
    )
    .unwrap();
    assert_eq!(names(&kt), ["User.q:method:pub:2-2"]);
    let q = kt.callables.iter().position(|c| c.name == "q").unwrap();
    assert!(kt.locals[q].contains("local") && kt.locals[q].contains("x"));
    let c = calls(&kt);
    assert!(c.contains(&"<init>->compute:Plain@3".to_owned()), "{c:?}");

    let ts = typescript_lang::extract(
        "a.ts",
        "export function f(cb: () => void) { function g() { h(); } const k = () => 1; cb(); g(); }\nexport const v = h();\nclass C { x = h(); static { h(); } }\n",
    )
    .unwrap();
    assert_eq!(names(&ts), ["f:function:pub:1-1"]);
    for n in ["cb", "g", "k"] {
        assert!(ts.locals[0].contains(n), "{n}");
    }
    let c = calls(&ts);
    for want in ["f->h:Plain@1", "<init>->h:Plain@2", "C.<init>->h:Plain@3"] {
        assert!(c.contains(&want.to_owned()), "missing {want}: {c:?}");
    }
}

#[test]
fn typescript_reexports_new_and_jsx() {
    let barrel = typescript_lang::extract(
        "index.ts",
        "export { target as t } from \"./impl\";\nexport * from \"./more\";\nexport * as ns from \"./x\";\n",
    )
    .unwrap();
    let re: Vec<String> = barrel
        .reexports
        .iter()
        .map(|r| format!("{}={}:{}", r.exported, r.module, r.imported))
        .collect();
    assert_eq!(re, ["t=./impl:target", "*=./more:*"]);
    let ui = typescript_lang::extract(
        "ui.tsx",
        "export function App() { const s = new Svc(1); return <Button a={1} />; }\n",
    )
    .unwrap();
    let c = calls(&ui);
    assert!(
        c.contains(&"App->constructor:Qualified([\"Svc\"])@1".to_owned()),
        "{c:?}"
    );
    assert!(c.contains(&"App->Button:Plain@1".to_owned()), "{c:?}");
}

#[test]
fn namespace_members_are_not_exported_by_the_namespace() {
    let f = typescript_lang::extract(
        "n.ts",
        "export namespace N { function priv() {} export function pub() {} }\n",
    )
    .unwrap();
    assert_eq!(
        names(&f),
        ["priv:function:priv:1-1", "pub:function:pub:1-1"]
    );
}

#[test]
fn kotlin_lambda_and_safe_call_flags() {
    let f = kotlin::extract(
        "a.kt",
        "package p\nclass S { fun make() = Builder().apply { name(\"x\") }\n fun name(s: String) {}\n fun g(a: A?) = a?.run() }\n",
    )
    .unwrap();
    let flagged: Vec<(String, bool)> = f
        .calls
        .iter()
        .map(|c| (c.name.clone(), c.in_lambda))
        .collect();
    assert!(flagged.contains(&("name".to_owned(), true)), "{flagged:?}");
    let c = calls(&f);
    assert!(
        c.contains(&"S.g->run:Method { self_receiver: false }@4".to_owned()),
        "{c:?}"
    );
}
