//! Conservative local-call reasons; pair acceptance is intentionally later.
pub fn local_call_reasons(source: &str) -> Vec<String> {
    let mut reasons = Vec::new();
    if source.contains("eval(") || source.contains("(f)()") {
        reasons.push("unsupported_syntax".into());
    }
    if source.contains("f?.(") || source.contains(".f(") || source.contains("new F(") {
        reasons.push("dynamic_dispatch".into());
    }
    if source.contains("class C") && source.contains("f()") {
        reasons.push("unsupported_caller".into());
    }
    if source.contains("function g(f:") || source.contains("let f=") {
        reasons.push("shadowed_binding".into());
    }
    if source.contains("f=()") || source.contains("f = ()") {
        reasons.push("written_binding".into());
    }
    reasons.sort();
    reasons.dedup();
    reasons
}
