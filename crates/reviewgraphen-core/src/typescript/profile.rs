//! Path-only `typescript.production.v1`; filesystem behavior never participates.

use thiserror::Error;
pub const TYPESCRIPT_LANGUAGE: &str = "typescript";
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeScriptPathClassification {
    Included,
    ProfileExcluded(&'static str),
    NonTargetExtension,
}
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("invalid repository-relative TypeScript path")]
pub struct TypeScriptPathError;

pub fn classify_typescript_v1_path(
    path: &str,
) -> Result<TypeScriptPathClassification, TypeScriptPathError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.as_bytes().contains(&0)
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(TypeScriptPathError);
    }
    if !path.ends_with(".ts") {
        return Ok(TypeScriptPathClassification::NonTargetExtension);
    }
    let parts = path.split('/').collect::<Vec<_>>();
    let name = parts.last().expect("validated path");
    for (components, reason) in [
        (
            &["node_modules", "vendor", "vendored", "third_party"][..],
            "profile.exclude.vendor@1",
        ),
        (
            &["dist", "build", "out", "coverage", "generated", ".next"][..],
            "profile.exclude.generated@1",
        ),
    ] {
        if parts.iter().any(|part| components.contains(part)) {
            return Ok(TypeScriptPathClassification::ProfileExcluded(reason));
        }
    }
    if name.ends_with(".generated.ts") || name.ends_with(".generated.tsx") {
        return Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.generated@1",
        ));
    }
    if name.ends_with(".d.ts") {
        return Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.declaration@1",
        ));
    }
    if parts.iter().any(|part| {
        matches!(
            *part,
            "tests" | "test" | "__tests__" | "__mocks__" | "benches" | "benchmarks"
        )
    }) || matches!(*name, "test.ts" | "tests.ts" | "test.tsx" | "tests.tsx")
        || [".test.ts", ".spec.ts", ".test.tsx", ".spec.tsx"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
    {
        return Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.test@1",
        ));
    }
    if parts
        .iter()
        .any(|part| matches!(*part, "example" | "examples"))
    {
        return Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.example@1",
        ));
    }
    if parts.iter().any(|part| matches!(*part, "doc" | "docs")) {
        return Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.docs@1",
        ));
    }
    Ok(TypeScriptPathClassification::Included)
}
