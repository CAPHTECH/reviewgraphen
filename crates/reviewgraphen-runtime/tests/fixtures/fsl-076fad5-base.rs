// Extracted verbatim from rust/fsl-runtime/src/lib.rs at
// 20f9139172a9072467c243e8e42e21af85fae809. Surrounding match arms and imports
// are intentionally omitted: this fixture preserves the changed recursive
// public-callee shape that caused m20 cluster 0d1c01ec to fail.
pub fn eval(
    expr: &Expr,
    state: &State,
    bindings: &mut Bindings,
    model: &KernelModel,
    old_state: Option<&State>,
) -> Result<Value, RuntimeError> {
    match expr {
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } if name == "rel_reachable" => relation_reachable(
            eval(first, state, bindings, model, old_state)?,
            eval(second, state, bindings, model, old_state)?,
            &eval(third, state, bindings, model, old_state)?,
        ),
        Expr::TernaryNamed { name, .. } => Err(runtime_error(format!(
            "unsupported ternary function '{name}'"
        ))),
    }
}
