use super::containment::directed;
use super::declaration::{Decision, select};
use super::types::{
    RawKey, RustFactRefV1, RustFileBindingV1, RustG3ReasonV1 as Reason,
    RustInclusiveLineColumnRange as Range,
};
use reviewgraphen_core::ProgramSpace;
use serde_json::Value;

pub(super) fn observe(
    key: &RawKey,
    full: Option<Range>,
    source: &RustFileBindingV1,
    program: &ProgramSpace,
) -> Decision {
    let RawKey::TestAttribute {
        owner_logical_name, ..
    } = key
    else {
        unreachable!("test key")
    };
    let full = full.ok_or((Reason::AcceptedLocationMismatch, Value::Null))?;
    let short = owner_logical_name
        .rsplit("::")
        .next()
        .expect("nonempty logical name");
    let accepted = select(program, source, "test", short, Some(full))?;
    let (module, _) = owner_logical_name
        .rsplit_once("::")
        .expect("qualified free function");
    let placeholder = Range::new(1, 1, 1, 1).expect("fixed placeholder");
    let parent = select(program, source, "module", module, Some(placeholder))?;
    directed(program, "contains", &parent.id, &accepted.id)?;
    Ok(RustFactRefV1::Artifact {
        id: accepted.id.clone(),
    })
}
