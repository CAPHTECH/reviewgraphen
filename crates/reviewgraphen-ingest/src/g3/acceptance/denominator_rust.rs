//! G3-denominator-rust@1: immutable, source-bound, independently parsed oracle.

use crate::g3::g3_test_support::rust_denominator::{
    RustDenominatorError, RustDenominatorKeyV1, RustOracleRangeV1, rust_denominator,
};
use reviewgraphen_core::ContentHash;
use std::collections::BTreeSet;
use std::fs;
use std::process::Command;

#[test]
fn rust_denominator_is_the_five_literal_source_keys() {
    const PATH: &str = "src/structural.rs";
    const SHA256: &str = "e22111d84a1611a511f1c7bccaeca7abb03c058ab51fc753511c75e3f087eadd";
    const SOURCE: &str = concat!(
        "#[cfg(test)]\n",
        "mod structural {\n",
        "    fn callee() {}\n",
        "    #[test]\n",
        "    fn caller() {\n",
        "        let mut value = 0;\n",
        "        value = 1;\n",
        "        callee();\n",
        "    }\n",
        "}\n",
    );

    assert_eq!(SOURCE.len(), 151, "frozen ASCII source byte length");
    assert!(
        SOURCE.is_ascii(),
        "Rust coordinates use the frozen ASCII body"
    );
    let literal_hash = ContentHash::sha256(SOURCE.as_bytes());
    assert_eq!(literal_hash.as_str(), format!("sha256:{SHA256}"));

    // Ordinary Git admission binds this literal to the snapshot's one accepted
    // file artifact. The denominator helper receives ONLY the admitted bytes.
    let workspace = tempfile::tempdir().expect("fixture workspace");
    let root = workspace.path().join("fixture");
    fs::create_dir(&root).expect("fixture repository directory");
    let git = |args: &[&str]| -> String {
        let result = Command::new("git")
            .current_dir(&root)
            .env("HOME", &root)
            .env("XDG_CONFIG_HOME", root.join(".xdg-config"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
            .args(args)
            .output()
            .expect("git launches");
        assert!(
            result.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout)
            .expect("git stdout UTF-8")
            .trim()
            .to_owned()
    };
    git(&["init", "--quiet", "--object-format=sha1"]);
    git(&["config", "user.email", "reviewgraphen@example.test"]);
    git(&["config", "user.name", "ReviewGraphen test"]);
    fs::create_dir(root.join("src")).expect("fixture source directory");
    fs::write(root.join(PATH), b"fn baseline() {}\n").expect("base source");
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "base"]);
    let base = git(&["rev-parse", "HEAD"]);
    fs::write(root.join(PATH), SOURCE.as_bytes()).expect("target source");
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "structural source"]);
    let target = git(&["rev-parse", "HEAD"]);
    let request = crate::IngestRequest::new(
        workspace.path(),
        &root,
        "reviewgraphen.test/g3-denominator-rust@1",
        &base,
        &target,
    );
    let result = crate::ingest_with_sources_v2(&request, u64::MAX)
        .expect("source-retaining Rust snapshot admission");
    let bundle = &result.legacy.source_bundle;
    assert_eq!(
        bundle.snapshot_id(),
        result.legacy.program_space.snapshot_id()
    );
    assert_eq!(bundle.entries().len(), 1, "only one admitted Rust file");
    assert_eq!(bundle.total_bytes(), 151);
    let source = &bundle.entries()[0];
    assert_eq!(source.path(), PATH);
    assert_eq!(source.bytes(), SOURCE.as_bytes());
    assert_eq!(source.bytes().len(), 151);
    assert_eq!(source.content_hash(), &literal_hash);
    assert_eq!(source.cas_hash(), &literal_hash);
    assert_eq!(source.content_hash().as_str(), format!("sha256:{SHA256}"));
    let file = result
        .legacy
        .program_space
        .artifact(source.artifact_id())
        .expect("admitted source has an accepted file artifact");
    assert_eq!(file.kind, "file");
    assert_eq!(file.language.as_deref(), Some("rust"));
    assert_eq!(
        file.location
            .as_ref()
            .map(|location| location.path.as_str()),
        Some(PATH)
    );
    assert_eq!(file.content_hash.as_ref(), Some(&literal_hash));

    // The row variants are the five literal identities. Both inclusive ends
    // and both containment ranges are part of equality, not a row count.
    let expected = BTreeSet::from([
        RustDenominatorKeyV1::Declaration {
            range: RustOracleRangeV1::new(3, 5, 3, 18),
        },
        RustDenominatorKeyV1::Containment {
            parent: RustOracleRangeV1::new(2, 1, 10, 1),
            child: RustOracleRangeV1::new(5, 5, 9, 5),
        },
        RustDenominatorKeyV1::DirectCall {
            range: RustOracleRangeV1::new(8, 9, 8, 16),
        },
        RustDenominatorKeyV1::Write {
            range: RustOracleRangeV1::new(7, 9, 7, 17),
        },
        RustDenominatorKeyV1::TestMarker {
            range: RustOracleRangeV1::new(4, 5, 4, 11),
        },
    ]);
    assert_eq!(expected.len(), 5, "the literal identities are distinct");
    let observed = rust_denominator(source.bytes()).expect("independent syn visitor");
    assert_eq!(
        observed, expected,
        "no missing, extra, or shifted source key"
    );

    // These must fail before a parser may return keys from different bytes.
    assert!(
        matches!(
            rust_denominator(&source.bytes()[..150]),
            Err(RustDenominatorError::WrongLength { .. })
        ),
        "wrong length"
    );
    let mut altered = source.bytes().to_vec();
    assert_eq!(altered[122], b'1', "frozen value = 1 byte");
    altered[122] = b'2';
    assert_eq!(altered.len(), 151, "same-length wrong-hash input");
    syn::parse_file(std::str::from_utf8(&altered).expect("ASCII mutation"))
        .expect("wrong-hash input remains valid Rust");
    assert_ne!(ContentHash::sha256(&altered), literal_hash);
    assert!(
        matches!(
            rust_denominator(&altered),
            Err(RustDenominatorError::WrongHash { .. })
        ),
        "same length, syntax-valid wrong hash"
    );
}
