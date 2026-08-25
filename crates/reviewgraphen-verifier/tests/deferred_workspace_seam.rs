//! The deferred seam treats every repository-controlled Cargo execution path
//! as untrusted executable input. In particular, `--locked` and `--offline`
//! do not prevent `build.rs`, proc macros, tests, doctests, Cargo config,
//! wrappers, runners, linkers, or compiler inputs from executing code.
//! These tests therefore exercise only the typed no-I/O seam: they must never
//! resolve an executable or start a process.

use reviewgraphen_core::StableId;
use reviewgraphen_verifier::{
    DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID, DEFERRED_WORKSPACE_CARGO_TEST_OUTCOME,
    DEFERRED_WORKSPACE_CARGO_TEST_REASON, DeferredWorkspaceRequestField,
    DeferredWorkspaceUnsupportedRecord, DeferredWorkspaceUnsupportedRecordField,
    DeferredWorkspaceUnsupportedRecordInput, DeferredWorkspaceVerifierRequest,
    DeferredWorkspaceVerifierResolution, VerifierError, resolve_deferred_workspace_verifier,
};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn id(value: &str) -> StableId {
    StableId::parse(value).unwrap()
}

fn request<'a>(
    descriptor_id: Option<&'a str>,
    forbidden_fields: &'a [DeferredWorkspaceRequestField<'a>],
    request_id: &'a StableId,
    snapshot_id: &'a StableId,
    universe_id: &'a StableId,
) -> DeferredWorkspaceVerifierRequest<'a> {
    request_with_fixture(
        descriptor_id,
        forbidden_fields,
        request_id,
        snapshot_id,
        universe_id,
        &[],
    )
}

fn request_with_fixture<'a>(
    descriptor_id: Option<&'a str>,
    forbidden_fields: &'a [DeferredWorkspaceRequestField<'a>],
    request_id: &'a StableId,
    snapshot_id: &'a StableId,
    universe_id: &'a StableId,
    untrusted_fixture: &'a [u8],
) -> DeferredWorkspaceVerifierRequest<'a> {
    DeferredWorkspaceVerifierRequest {
        descriptor_id,
        request_id,
        snapshot_id,
        universe_id,
        untrusted_fixture,
        forbidden_fields,
    }
}

fn selected_record() -> (
    StableId,
    StableId,
    StableId,
    DeferredWorkspaceUnsupportedRecord,
) {
    let request_id = id("request:deferred-workspace");
    let snapshot_id = id("snapshot:deferred-workspace");
    let universe_id = id("universe:deferred-workspace");
    let resolution = resolve_deferred_workspace_verifier(request(
        Some(DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID),
        &[],
        &request_id,
        &snapshot_id,
        &universe_id,
    ))
    .unwrap();
    let DeferredWorkspaceVerifierResolution::Unsupported(record) = resolution else {
        panic!("selected deferred descriptor must produce exactly one record");
    };
    (request_id, snapshot_id, universe_id, record)
}

fn assert_hostile_fixture_no_io(fixture: &str) {
    // The fixture is passed as opaque untrusted bytes. The canary observes an
    // actual child process independently of the seam's returned record.
    assert!(!fixture.is_empty());
    let canary = ProcessCanary::install();
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "canary_child_runs_deferred_seam", "--nocapture"])
        .env("PATH", canary.path())
        .env("REVIEWGRAPHEN_CANARY_ROOT", &canary.root)
        .env("REVIEWGRAPHEN_CANARY_FIXTURE", fixture)
        .output()
        .expect("start canary child test");
    assert!(
        output.status.success(),
        "the child seam test failed under hostile fixture `{fixture}`:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !canary.was_invoked(),
        "the deferred seam spawned a process under hostile fixture: {fixture}"
    );
}

static CANARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// PATH-based process detector. Every shim creates `<shim>.invoked`, allowing
/// the test to observe a real child process rather than trusting a returned
/// boolean. The canary PATH is supplied only to child processes, so parallel
/// tests never mutate the parent process environment.
struct ProcessCanary {
    root: PathBuf,
}

impl ProcessCanary {
    fn install() -> Self {
        let sequence = CANARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "reviewgraphen-verifier-process-canary-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create canary directory");
        for command in ["cargo", "rustc", "sh", "true"] {
            let shim = root.join(command);
            fs::write(&shim, "#!/bin/sh\n: > \"${0}.invoked\"\nexit 0\n")
                .expect("write canary shim");
            let mut permissions = fs::metadata(&shim).expect("stat canary shim").permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&shim, permissions).expect("make canary shim executable");
        }
        Self { root }
    }

    fn path(&self) -> OsString {
        let mut canary_path = self.root.as_os_str().to_os_string();
        canary_path.push(":");
        if let Some(path) = std::env::var_os("PATH") {
            canary_path.push(path);
        }
        canary_path
    }

    fn was_invoked(&self) -> bool {
        ["cargo", "rustc", "sh", "true"]
            .iter()
            .any(|command| self.root.join(format!("{command}.invoked")).exists())
    }

    fn intentionally_start_process(&self) {
        let status = std::process::Command::new("true")
            .env("PATH", self.path())
            .status()
            .expect("canary true must start");
        assert!(status.success());
    }
}

impl Drop for ProcessCanary {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove canary directory");
    }
}

#[test]
fn process_canary_positive_control_observes_a_real_child_process() {
    let canary = ProcessCanary::install();
    assert!(!canary.was_invoked());
    canary.intentionally_start_process();
    assert!(canary.was_invoked());
}

#[test]
fn canary_child_runs_deferred_seam() {
    let Ok(fixture) = std::env::var("REVIEWGRAPHEN_CANARY_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(
        std::env::var_os("REVIEWGRAPHEN_CANARY_ROOT").expect("canary child root is present"),
    );
    let request_id = id("request:deferred-workspace");
    let snapshot_id = id("snapshot:deferred-workspace");
    let universe_id = id("universe:deferred-workspace");
    let (_, _, _, expected) = selected_record();
    let DeferredWorkspaceVerifierResolution::Unsupported(actual) =
        resolve_deferred_workspace_verifier(request_with_fixture(
            Some(DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID),
            &[],
            &request_id,
            &snapshot_id,
            &universe_id,
            fixture.as_bytes(),
        ))
        .unwrap()
    else {
        panic!("selected descriptor must remain unsupported");
    };
    assert_eq!(actual, expected);
    assert!(!actual.process_started());
    assert!(!actual.executable_resolved());
    assert!(actual.verifier_observed().is_empty());
    assert!(
        !["cargo", "rustc", "sh", "true"]
            .iter()
            .any(|command| root.join(format!("{command}.invoked")).exists()),
        "the deferred seam started a canary command"
    );
}

fn assert_field_is_schema_invalid(field: DeferredWorkspaceRequestField<'_>) {
    let request_id = id("request:deferred-workspace");
    let snapshot_id = id("snapshot:deferred-workspace");
    let universe_id = id("universe:deferred-workspace");
    assert!(matches!(
        resolve_deferred_workspace_verifier(request(
            Some(DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID),
            &[field],
            &request_id,
            &snapshot_id,
            &universe_id,
        )),
        Err(VerifierError::DeferredWorkspaceRequestSchema { .. })
    ));
}

macro_rules! hostile_fixture_no_io_test {
    ($name:ident, $fixture:expr) => {
        #[test]
        fn $name() {
            assert_hostile_fixture_no_io($fixture);
        }
    };
}

macro_rules! field_schema_invalid_test {
    ($name:ident, $field:expr) => {
        #[test]
        fn $name() {
            assert_field_is_schema_invalid($field);
        }
    };
}

hostile_fixture_no_io_test!(
    hostile_build_rs_is_never_executed,
    "workspace/build.rs: std::process::Command::new(\"sh\").arg(\"-c\").arg(\"payload\")"
);
hostile_fixture_no_io_test!(
    hostile_proc_macro_is_never_executed,
    "workspace/proc_macro/src/lib.rs: #[proc_macro] fn payload(_) { std::process::Command::new(\"sh\") }"
);
hostile_fixture_no_io_test!(
    hostile_test_binary_is_never_executed,
    "workspace/tests/payload.rs: #[test] fn payload() { std::process::Command::new(\"sh\") }"
);
hostile_fixture_no_io_test!(
    hostile_doctest_is_never_executed,
    "workspace/src/lib.rs: /// ```rust\n/// std::process::Command::new(\"sh\").spawn();\n/// ```"
);
hostile_fixture_no_io_test!(
    hostile_repository_cargo_config_is_never_read,
    "workspace/.cargo/config.toml: [build] rustc-wrapper = \"/tmp/wrapper\"\n[target.x86_64-unknown-linux-gnu] runner = \"/tmp/runner\""
);
hostile_fixture_no_io_test!(
    hostile_workspace_cargo_config_is_never_read,
    "workspace/member/.cargo/config.toml: [target.x86_64-unknown-linux-gnu] linker = \"/tmp/linker\""
);
hostile_fixture_no_io_test!(
    hostile_parent_cargo_config_is_never_read,
    "parent/.cargo/config: [build] rustc-wrapper = \"/tmp/parent-wrapper\""
);
hostile_fixture_no_io_test!(
    hostile_cargo_home_config_is_never_read,
    "CARGO_HOME/config.toml: [build] rustc-wrapper = \"/tmp/home-wrapper\""
);
hostile_fixture_no_io_test!(
    hostile_absolute_executable_is_never_resolved,
    "workspace/.cargo/config.toml: [alias] test = \"/absolute/path/to/cargo test\""
);
hostile_fixture_no_io_test!(
    hostile_fork_bomb_is_never_started,
    "workspace/tests/fork.rs: #[test] fn payload() { :(){ :|:& };: }"
);
hostile_fixture_no_io_test!(
    hostile_network_attempt_is_never_started,
    "workspace/build.rs: TcpStream::connect(\"198.51.100.1:443\")"
);
hostile_fixture_no_io_test!(
    hostile_mount_external_read_write_is_never_started,
    "workspace/build.rs: read(\"/etc/shadow\"); write(\"/host/out\")"
);
hostile_fixture_no_io_test!(
    hostile_escape_attempt_is_never_started,
    "workspace/build.rs: canonicalize(\"../../../../escape\")"
);
hostile_fixture_no_io_test!(
    hostile_file_inode_output_memory_exhaustion_is_never_started,
    "workspace/build.rs: loop { create_files(); allocate_memory(); emit_output(); }"
);

field_schema_invalid_test!(
    executable_field_is_schema_invalid,
    DeferredWorkspaceRequestField::Executable
);
field_schema_invalid_test!(
    argv_field_is_schema_invalid,
    DeferredWorkspaceRequestField::Argv
);
field_schema_invalid_test!(
    path_field_is_schema_invalid,
    DeferredWorkspaceRequestField::Path
);
field_schema_invalid_test!(
    cwd_field_is_schema_invalid,
    DeferredWorkspaceRequestField::Cwd
);
field_schema_invalid_test!(
    environment_field_is_schema_invalid,
    DeferredWorkspaceRequestField::Environment
);
field_schema_invalid_test!(
    mount_field_is_schema_invalid,
    DeferredWorkspaceRequestField::Mount
);
field_schema_invalid_test!(
    cache_field_is_schema_invalid,
    DeferredWorkspaceRequestField::Cache
);
field_schema_invalid_test!(
    credential_field_is_schema_invalid,
    DeferredWorkspaceRequestField::Credential
);
field_schema_invalid_test!(
    toolchain_field_is_schema_invalid,
    DeferredWorkspaceRequestField::Toolchain
);
field_schema_invalid_test!(
    resource_limit_field_is_schema_invalid,
    DeferredWorkspaceRequestField::ResourceLimit
);

#[test]
fn disabled_is_record_free_and_selected_is_one_closed_unsupported_record() {
    let request_id = id("request:deferred-workspace");
    let snapshot_id = id("snapshot:deferred-workspace");
    let universe_id = id("universe:deferred-workspace");
    assert_eq!(
        resolve_deferred_workspace_verifier(request(
            None,
            &[],
            &request_id,
            &snapshot_id,
            &universe_id,
        ))
        .unwrap(),
        DeferredWorkspaceVerifierResolution::Disabled
    );

    let (_, _, _, record) = selected_record();
    assert_eq!(
        record.descriptor_id(),
        DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID
    );
    assert_eq!(record.outcome(), DEFERRED_WORKSPACE_CARGO_TEST_OUTCOME);
    assert_eq!(record.reason(), DEFERRED_WORKSPACE_CARGO_TEST_REASON);
    assert!(!record.process_started());
    assert!(!record.executable_resolved());
    assert!(record.verifier_observed().is_empty());
    assert_eq!(
        record.id_preimage(),
        br#"{"descriptor_id":"workspace.cargo_test@1","executable_resolved":false,"outcome":"unsupported","process_started":false,"reason":"workspace_cargo_test_deferred","request_id":"request:deferred-workspace","snapshot_id":"snapshot:deferred-workspace","universe_id":"universe:deferred-workspace"}"#
    );
}

#[test]
fn exact_preimage_is_deterministic_and_binds_only_the_closed_fields() {
    let (_, _, _, first) = selected_record();
    let (_, _, _, second) = selected_record();
    assert_eq!(first.id(), second.id());
    assert_eq!(first.id_preimage(), second.id_preimage());

    let changed_request = id("request:deferred-workspace-mutated");
    let snapshot_id = id("snapshot:deferred-workspace");
    let universe_id = id("universe:deferred-workspace");
    let DeferredWorkspaceVerifierResolution::Unsupported(changed) =
        resolve_deferred_workspace_verifier(request(
            Some(DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID),
            &[],
            &changed_request,
            &snapshot_id,
            &universe_id,
        ))
        .unwrap()
    else {
        panic!("selected descriptor must resolve");
    };
    assert_ne!(first.id(), changed.id());
}

#[test]
fn resolver_rejects_a_non_request_request_id() {
    let request_id = id("snapshot:not-a-request");
    let snapshot_id = id("snapshot:deferred-workspace");
    let universe_id = id("universe:deferred-workspace");
    assert!(matches!(
        resolve_deferred_workspace_verifier(request(
            Some(DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID),
            &[],
            &request_id,
            &snapshot_id,
            &universe_id,
        )),
        Err(VerifierError::DeferredWorkspaceRequestSchema {
            field: "request_id"
        })
    ));
}

#[test]
fn resolver_rejects_a_non_snapshot_snapshot_id() {
    let request_id = id("request:deferred-workspace");
    let snapshot_id = id("universe:not-a-snapshot");
    let universe_id = id("universe:deferred-workspace");
    assert!(matches!(
        resolve_deferred_workspace_verifier(request(
            Some(DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID),
            &[],
            &request_id,
            &snapshot_id,
            &universe_id,
        )),
        Err(VerifierError::DeferredWorkspaceRequestSchema {
            field: "snapshot_id"
        })
    ));
}

#[test]
fn resolver_rejects_a_non_universe_universe_id() {
    let request_id = id("request:deferred-workspace");
    let snapshot_id = id("snapshot:deferred-workspace");
    let universe_id = id("request:not-a-universe");
    assert!(matches!(
        resolve_deferred_workspace_verifier(request(
            Some(DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID),
            &[],
            &request_id,
            &snapshot_id,
            &universe_id,
        )),
        Err(VerifierError::DeferredWorkspaceRequestSchema {
            field: "universe_id"
        })
    ));
}

#[test]
fn record_validator_rejects_each_wrong_identity_namespace() {
    let (request_id, snapshot_id, universe_id, record) = selected_record();
    let invalid_request_id = id("snapshot:not-a-request");
    let invalid_snapshot_id = id("universe:not-a-snapshot");
    let invalid_universe_id = id("request:not-a-universe");
    for (candidate_request_id, candidate_snapshot_id, candidate_universe_id, expected_field) in [
        (
            &invalid_request_id,
            &snapshot_id,
            &universe_id,
            "request_id",
        ),
        (
            &request_id,
            &invalid_snapshot_id,
            &universe_id,
            "snapshot_id",
        ),
        (
            &request_id,
            &snapshot_id,
            &invalid_universe_id,
            "universe_id",
        ),
    ] {
        assert!(matches!(
            DeferredWorkspaceUnsupportedRecord::validate(DeferredWorkspaceUnsupportedRecordInput {
                id: record.id(),
                descriptor_id: record.descriptor_id(),
                request_id: candidate_request_id,
                snapshot_id: candidate_snapshot_id,
                universe_id: candidate_universe_id,
                outcome: record.outcome(),
                reason: record.reason(),
                process_started: false,
                executable_resolved: false,
                verifier_observed: &[],
                forbidden_field: None,
            }),
            Err(VerifierError::DeferredWorkspaceRecordInvalid { field }) if field == expected_field
        ));
    }
}

#[test]
fn every_execution_control_field_is_schema_invalid_individually() {
    let request_id = id("request:deferred-workspace");
    let snapshot_id = id("snapshot:deferred-workspace");
    let universe_id = id("universe:deferred-workspace");
    let fields = [
        DeferredWorkspaceRequestField::Command,
        DeferredWorkspaceRequestField::Executable,
        DeferredWorkspaceRequestField::Argv,
        DeferredWorkspaceRequestField::Path,
        DeferredWorkspaceRequestField::Cwd,
        DeferredWorkspaceRequestField::Environment,
        DeferredWorkspaceRequestField::Mount,
        DeferredWorkspaceRequestField::Cache,
        DeferredWorkspaceRequestField::Credential,
        DeferredWorkspaceRequestField::Toolchain,
        DeferredWorkspaceRequestField::ResourceLimit,
        DeferredWorkspaceRequestField::Identity,
        DeferredWorkspaceRequestField::Other("unrecognized"),
    ];
    for field in fields {
        assert!(matches!(
            resolve_deferred_workspace_verifier(request(
                Some(DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID),
                &[field],
                &request_id,
                &snapshot_id,
                &universe_id,
            )),
            Err(VerifierError::DeferredWorkspaceRequestSchema { .. })
        ));
    }
    assert_eq!(
        resolve_deferred_workspace_verifier(request(
            Some("cargo test"),
            &[],
            &request_id,
            &snapshot_id,
            &universe_id,
        )),
        Err(VerifierError::DeferredWorkspaceDescriptorSchema)
    );
}

#[test]
fn record_validator_rejects_each_non_closed_or_execution_bearing_field() {
    let (request_id, snapshot_id, universe_id, record) = selected_record();
    let invalid = |input| DeferredWorkspaceUnsupportedRecord::validate(input);
    let valid = || DeferredWorkspaceUnsupportedRecordInput {
        id: record.id(),
        descriptor_id: record.descriptor_id(),
        request_id: &request_id,
        snapshot_id: &snapshot_id,
        universe_id: &universe_id,
        outcome: record.outcome(),
        reason: record.reason(),
        process_started: false,
        executable_resolved: false,
        verifier_observed: &[],
        forbidden_field: None,
    };
    assert_eq!(invalid(valid()).unwrap(), record);

    let observed = [id("verifier-observation:forbidden")];
    let obligation_id = id("obligation:forbidden");
    let cases = [
        DeferredWorkspaceUnsupportedRecordField::ProcessIdentity("pid:1"),
        DeferredWorkspaceUnsupportedRecordField::ExecutableIdentity("/bin/cargo"),
        DeferredWorkspaceUnsupportedRecordField::Output("output"),
        DeferredWorkspaceUnsupportedRecordField::ResourceObservation("rss"),
        DeferredWorkspaceUnsupportedRecordField::ObligationBinding(&obligation_id),
        DeferredWorkspaceUnsupportedRecordField::Other("extra"),
    ];
    for field in cases {
        let mut input = valid();
        input.forbidden_field = Some(field);
        assert!(matches!(
            invalid(input),
            Err(VerifierError::DeferredWorkspaceRecordInvalid { .. })
        ));
    }
    let mut started = valid();
    started.process_started = true;
    assert!(matches!(
        invalid(started),
        Err(VerifierError::DeferredWorkspaceRecordInvalid { .. })
    ));
    let mut resolved = valid();
    resolved.executable_resolved = true;
    assert!(matches!(
        invalid(resolved),
        Err(VerifierError::DeferredWorkspaceRecordInvalid { .. })
    ));
    let mut observed_input = valid();
    observed_input.verifier_observed = &observed;
    assert!(matches!(
        invalid(observed_input),
        Err(VerifierError::DeferredWorkspaceRecordInvalid { .. })
    ));
    let mut wrong_reason = valid();
    wrong_reason.reason = "another_reason";
    assert!(matches!(
        invalid(wrong_reason),
        Err(VerifierError::DeferredWorkspaceRecordInvalid { .. })
    ));
}

#[test]
fn malicious_cargo_inputs_share_the_same_no_io_typed_result() {
    let malicious_inputs = [
        "build.rs { std::process::Command::new(\"sh\") }",
        "proc_macro: emit arbitrary code",
        "#[test] { panic!(\"malicious test\") }",
        "//! doctest that executes a payload",
        "[build] rustc-wrapper = \"/absolute/wrapper\"; runner = \"fork-bomb\"; linker = \"/bin/linker\"",
        "/absolute/path/to/cargo",
        ":(){ :|:& };:",
        "connect(\"network\")",
        "read/write outside mount; ../../escape",
        "exhaust files inodes output memory",
    ];
    for untrusted_executable_input in malicious_inputs {
        assert_hostile_fixture_no_io(untrusted_executable_input);
    }
}
