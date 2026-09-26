use reviewgraphen_core::typescript::profile::{
    TYPESCRIPT_LANGUAGE, TypeScriptPathClassification, TypeScriptPathError,
    classify_typescript_v1_path,
};

#[test]
fn typescript_language_is_the_frozen_v1_language() {
    assert_eq!(TYPESCRIPT_LANGUAGE, "typescript");
}

#[test]
fn vendor_matcher_excludes_every_frozen_component() {
    assert_eq!(
        classify_typescript_v1_path("node_modules/pkg/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.vendor@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("vendor/pkg/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.vendor@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("vendored/pkg/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.vendor@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("third_party/pkg/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.vendor@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("node_modules/__tests__/f.test.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.vendor@1"
        ))
    );
}

#[test]
fn generated_matcher_excludes_every_frozen_component_and_suffix() {
    assert_eq!(
        classify_typescript_v1_path("dist/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.generated@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("build/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.generated@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("out/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.generated@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("coverage/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.generated@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("generated/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.generated@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path(".next/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.generated@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("src/f.generated.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.generated@1"
        ))
    );
}

#[test]
fn generated_tsx_suffix_is_non_target_in_v1() {
    assert_eq!(
        classify_typescript_v1_path("src/f.generated.tsx"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
}

#[test]
fn declaration_matcher_excludes_its_frozen_suffix() {
    assert_eq!(
        classify_typescript_v1_path("src/f.d.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.declaration@1"
        ))
    );
}

#[test]
fn test_matcher_classifies_every_frozen_component_suffix_and_basename() {
    assert_eq!(
        classify_typescript_v1_path("tests/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.test@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("test/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.test@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("__tests__/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.test@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("__mocks__/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.test@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("benches/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.test@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("benchmarks/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.test@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("src/f.test.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.test@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("src/f.spec.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.test@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("src/test.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.test@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("src/tests.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.test@1"
        ))
    );
}

#[test]
fn test_tsx_suffix_is_non_target_in_v1() {
    assert_eq!(
        classify_typescript_v1_path("src/f.test.tsx"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
}

#[test]
fn spec_tsx_suffix_is_non_target_in_v1() {
    assert_eq!(
        classify_typescript_v1_path("src/f.spec.tsx"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
}

#[test]
fn test_tsx_basename_is_non_target_in_v1() {
    assert_eq!(
        classify_typescript_v1_path("src/test.tsx"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
}

#[test]
fn tests_tsx_basename_is_non_target_in_v1() {
    assert_eq!(
        classify_typescript_v1_path("src/tests.tsx"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
}

#[test]
fn example_and_docs_matchers_exclude_every_frozen_component() {
    assert_eq!(
        classify_typescript_v1_path("example/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.example@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("examples/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.example@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("doc/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.docs@1"
        ))
    );
    assert_eq!(
        classify_typescript_v1_path("docs/f.ts"),
        Ok(TypeScriptPathClassification::ProfileExcluded(
            "profile.exclude.docs@1"
        ))
    );
}

#[test]
fn non_target_js_has_precedence_over_vendor_component() {
    assert_eq!(
        classify_typescript_v1_path("node_modules/x.js"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
}

#[test]
fn non_target_md_has_precedence_over_docs_component() {
    assert_eq!(
        classify_typescript_v1_path("docs/readme.md"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
}

#[test]
fn non_target_tsx_has_precedence_over_test_component() {
    assert_eq!(
        classify_typescript_v1_path("tests/x.tsx"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
}

#[test]
fn target_extensions_and_exact_matchers_are_case_sensitive() {
    assert_eq!(
        classify_typescript_v1_path("src/f.ts"),
        Ok(TypeScriptPathClassification::Included)
    );
    assert_eq!(
        classify_typescript_v1_path("src/f.tsx"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
    assert_eq!(
        classify_typescript_v1_path("src/f.TS"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
    assert_eq!(
        classify_typescript_v1_path("src/f.js"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
    assert_eq!(
        classify_typescript_v1_path("src/test_helpers.ts"),
        Ok(TypeScriptPathClassification::Included)
    );
    assert_eq!(
        classify_typescript_v1_path("src/contest.ts"),
        Ok(TypeScriptPathClassification::Included)
    );
    assert_eq!(
        classify_typescript_v1_path("src/Tests/f.ts"),
        Ok(TypeScriptPathClassification::Included)
    );
    assert_eq!(
        classify_typescript_v1_path("src/f.mts"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
    assert_eq!(
        classify_typescript_v1_path("src/f.cts"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
    assert_eq!(
        classify_typescript_v1_path("src/f.jsx"),
        Ok(TypeScriptPathClassification::NonTargetExtension)
    );
}

#[test]
fn malformed_paths_return_the_typed_path_error() {
    assert_eq!(classify_typescript_v1_path(""), Err(TypeScriptPathError));
    assert_eq!(
        classify_typescript_v1_path("/src/f.ts"),
        Err(TypeScriptPathError)
    );
    assert_eq!(
        classify_typescript_v1_path("src\\f.ts"),
        Err(TypeScriptPathError)
    );
    assert_eq!(
        classify_typescript_v1_path("src/\0/f.ts"),
        Err(TypeScriptPathError)
    );
    assert_eq!(
        classify_typescript_v1_path("src//f.ts"),
        Err(TypeScriptPathError)
    );
    assert_eq!(
        classify_typescript_v1_path("src/./f.ts"),
        Err(TypeScriptPathError)
    );
    assert_eq!(
        classify_typescript_v1_path("src/../f.ts"),
        Err(TypeScriptPathError)
    );
}
