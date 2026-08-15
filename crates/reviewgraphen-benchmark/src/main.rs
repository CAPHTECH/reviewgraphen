use reviewgraphen_benchmark::{
    PrivateAdjudicationReconciliation, Score, TrialCollection, collect_trial,
    import_blind_adjudication, parse_candidate, parse_collection, parse_execution_config,
    parse_inventory, parse_manifest, parse_oracle, parse_score,
    real::{
        RealScore, blind_real_adjudication_export, parse_real_inventory, parse_real_oracle,
        parse_real_score, parse_real_unit, score_real, summarize_real_full_run, summarize_real_run,
    },
    score, summarize_run,
};
use reviewgraphen_reviewer::process::{
    NonAuthorityProcessRecord, ProcessReviewer, ProcessReviewerBackend, ProcessReviewerInput,
    ProcessSandbox,
};
use std::{env, fs, io::Write, path::Path, process::ExitCode};

fn read(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|error| error.to_string())
}

fn emit<T: serde::Serialize>(value: &T) -> ExitCode {
    match reviewgraphen_core::canonical_json(value) {
        Ok(bytes) => {
            println!("{}", String::from_utf8_lossy(&bytes));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [command, kind, path] if command == "validate" => validate(kind, path),
        [command, manifest, candidate, oracle] if command == "score" => {
            run_score(manifest, candidate, oracle)
        }
        [command, manifest, candidate, oracle, unit] if command == "score-real" => {
            run_score_real(manifest, candidate, oracle, unit)
        }
        [
            command,
            manifest,
            candidate,
            oracle,
            unit,
            public_output,
            private_output,
        ] if command == "export-real-adjudication" => run_export_real_adjudication(
            manifest,
            candidate,
            oracle,
            unit,
            Path::new(public_output),
            Path::new(private_output),
        ),
        [command, reconciliation, decision] if command == "validate-blind-adjudication" => {
            run_validate_blind_adjudication(reconciliation, decision)
        }
        [command, inventory, collections, scores] if command == "summarize-run" => {
            run_summarize(inventory, collections, scores)
        }
        [command, inventory, collections, scores] if command == "summarize-real-run" => {
            run_summarize_real(inventory, collections, scores)
        }
        [command, inventory, collections, scores] if command == "summarize-real-full-run" => {
            run_summarize_real_full(inventory, collections, scores)
        }
        [command, ..] if command == "summarize" => {
            eprintln!(
                "legacy score-only summarize is disabled; use summarize-run with inventory and collections"
            );
            ExitCode::from(2)
        }
        [
            command,
            public_dir,
            output_dir,
            execution_config,
            replicates,
        ] if command == "prepare-pilot" => run_prepare_pilot(
            Path::new(public_dir),
            Path::new(output_dir),
            execution_config,
            replicates,
        ),
        [
            command,
            public_dir,
            units_dir,
            output_dir,
            execution_config,
            replicates,
        ] if command == "prepare-real" => run_prepare_real(
            Path::new(public_dir),
            Path::new(units_dir),
            Path::new(output_dir),
            execution_config,
            replicates,
        ),
        [
            command,
            public_dir,
            units_dir,
            output_dir,
            execution_config,
            replicates,
        ] if command == "prepare-real-full" => run_prepare_real_full(
            Path::new(public_dir),
            Path::new(units_dir),
            Path::new(output_dir),
            execution_config,
            replicates,
        ),
        [command, manifest_path, candidate_path, output_path] if command == "collect" => {
            run_collect(manifest_path, candidate_path, Path::new(output_path))
        }
        [command, record] if command == "replay-process-reviewer" => {
            run_replay_process_reviewer(record)
        }
        [
            command,
            backend,
            input_root,
            output_schema,
            output_root,
            record_output,
            bwrap,
            credential_home,
            executable,
            model,
            effort,
        ] if command == "run-process-reviewer" => run_process_reviewer(
            backend,
            Path::new(input_root),
            output_schema,
            Path::new(output_root),
            Path::new(record_output),
            bwrap,
            credential_home,
            executable,
            model,
            effort,
            false,
        ),
        [
            command,
            backend,
            input_root,
            output_schema,
            output_root,
            record_output,
            bwrap,
            credential_home,
            executable,
            model,
            effort,
        ] if command == "run-process-reviewer-constrained" => run_process_reviewer(
            backend,
            Path::new(input_root),
            output_schema,
            Path::new(output_root),
            Path::new(record_output),
            bwrap,
            credential_home,
            executable,
            model,
            effort,
            true,
        ),
        [
            command,
            profile,
            environment_variable,
            input_root,
            output_schema,
            output_root,
            record_output,
            bwrap,
            credential_home,
            executable,
            model,
            effort,
        ] if command == "run-process-reviewer-codex-profile" => run_process_reviewer_with_profile(
            Path::new(input_root),
            output_schema,
            Path::new(output_root),
            Path::new(record_output),
            bwrap,
            credential_home,
            executable,
            model,
            effort,
            Some((profile, environment_variable)),
            false,
        ),
        [
            command,
            profile,
            environment_variable,
            input_root,
            output_schema,
            output_root,
            record_output,
            bwrap,
            credential_home,
            executable,
            model,
            effort,
        ] if command == "run-process-reviewer-codex-profile-constrained" => {
            run_process_reviewer_with_profile(
                Path::new(input_root),
                output_schema,
                Path::new(output_root),
                Path::new(record_output),
                bwrap,
                credential_home,
                executable,
                model,
                effort,
                Some((profile, environment_variable)),
                true,
            )
        }
        _ => {
            eprintln!(
                "usage: reviewgraphen-benchmark validate <execution|manifest|candidate|oracle|collection|inventory|score|real-unit|real-oracle|real-inventory|real-score> <file> | score <manifest> <candidate> <oracle> | score-real <manifest> <candidate> <real-oracle> <real-unit> | export-real-adjudication <manifest> <candidate> <real-oracle> <real-unit> <public-out> <private-out> | validate-blind-adjudication <reconciliation.json> <decision.json> | summarize-run <inventory.json> <collections.json> <scores.json> | summarize-real-run <real-inventory.json> <collections.json> <real-scores.json> | summarize-real-full-run <full-real-inventory.json> <collections.json> <real-scores.json> | prepare-pilot <absolute-public-dir> <absolute-output-dir> <execution-config.json> <replicates> | prepare-real <absolute-public-dir> <absolute-private-units-dir> <absolute-output-dir> <execution-config.json> <replicates> | prepare-real-full <absolute-public-dir> <absolute-private-units-dir> <absolute-output-dir> <execution-config.json> <replicates> | collect <manifest> <candidate.json> <out> | run-process-reviewer[-constrained] <codex|claude> <input-root> <output-schema-relative> <fresh-output-root> <record-output> <bwrap> <credential-home> <backend-executable> <model> <effort> | run-process-reviewer-codex-profile[-constrained] <profile> <environment-variable> <input-root> <output-schema-relative> <fresh-output-root> <record-output> <bwrap> <credential-home> <backend-executable> <model> <effort> | replay-process-reviewer <record.json>"
            );
            ExitCode::from(2)
        }
    }
}

fn run_replay_process_reviewer(record_path: &str) -> ExitCode {
    let result = (|| {
        let record: NonAuthorityProcessRecord =
            serde_json::from_slice(&read(record_path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        let bytes = record.replay().map_err(|error| error.to_string())?;
        std::io::stdout()
            .write_all(bytes)
            .map_err(|error| error.to_string())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_process_reviewer(
    backend_kind: &str,
    input_root: &Path,
    output_schema: &str,
    output_root: &Path,
    record_output: &Path,
    bwrap: &str,
    credential_home: &str,
    executable: &str,
    model: &str,
    effort: &str,
    provider_constrained: bool,
) -> ExitCode {
    if backend_kind == "codex" {
        return run_process_reviewer_with_profile(
            input_root,
            output_schema,
            output_root,
            record_output,
            bwrap,
            credential_home,
            executable,
            model,
            effort,
            None,
            provider_constrained,
        );
    }
    let result = (|| {
        if record_output.exists() {
            return Err("process reviewer record output already exists".to_owned());
        }
        let backend = match backend_kind {
            "claude" => ProcessReviewerBackend::claude_cli(executable, model, effort),
            _ => return Err("backend must be codex or claude".to_owned()),
        }
        .map_err(|error| error.to_string())?;
        let sandbox =
            ProcessSandbox::new(bwrap, credential_home).map_err(|error| error.to_string())?;
        let input =
            ProcessReviewerInput::admit_current(input_root).map_err(|error| error.to_string())?;
        let reviewer = ProcessReviewer::new(backend, sandbox).map_err(|error| error.to_string())?;
        let record = if provider_constrained {
            reviewer.run(&input, output_schema, output_root)
        } else {
            reviewer.run_downstream_validated(&input, output_schema, output_root)
        }
        .map_err(|error| error.to_string())?;
        let bytes =
            reviewgraphen_core::canonical_json(&record).map_err(|error| error.to_string())?;
        fs::write(record_output, bytes).map_err(|error| error.to_string())?;
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_process_reviewer_with_profile(
    input_root: &Path,
    output_schema: &str,
    output_root: &Path,
    record_output: &Path,
    bwrap: &str,
    credential_home: &str,
    executable: &str,
    model: &str,
    effort: &str,
    profile: Option<(&str, &str)>,
    provider_constrained: bool,
) -> ExitCode {
    let result = (|| {
        if record_output.exists() {
            return Err("process reviewer record output already exists".to_owned());
        }
        let backend = match profile {
            Some((profile, environment_variable)) => {
                ProcessReviewerBackend::codex_cli_with_profile(
                    executable,
                    model,
                    effort,
                    profile,
                    [environment_variable.to_owned()],
                )
            }
            None => ProcessReviewerBackend::codex_cli(executable, model, effort),
        }
        .map_err(|error| error.to_string())?;
        let sandbox =
            ProcessSandbox::new(bwrap, credential_home).map_err(|error| error.to_string())?;
        let input =
            ProcessReviewerInput::admit_current(input_root).map_err(|error| error.to_string())?;
        let reviewer = ProcessReviewer::new(backend, sandbox).map_err(|error| error.to_string())?;
        let record = if provider_constrained {
            reviewer.run(&input, output_schema, output_root)
        } else {
            reviewer.run_downstream_validated(&input, output_schema, output_root)
        }
        .map_err(|error| error.to_string())?;
        let bytes =
            reviewgraphen_core::canonical_json(&record).map_err(|error| error.to_string())?;
        fs::write(record_output, bytes).map_err(|error| error.to_string())?;
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run_prepare_real(
    public_dir: &Path,
    units_dir: &Path,
    output_dir: &Path,
    execution_config_path: &str,
    replicates: &str,
) -> ExitCode {
    let Ok(replicates) = replicates.parse::<u32>() else {
        eprintln!("replicates must be a positive integer");
        return ExitCode::from(2);
    };
    let execution = match read(execution_config_path)
        .and_then(|bytes| parse_execution_config(&bytes).map_err(|error| error.to_string()))
    {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    match reviewgraphen_benchmark::prepare::prepare_real(
        public_dir, units_dir, output_dir, &execution, replicates,
    ) {
        Ok(cases) => emit(&serde_json::json!({
            "schema":"reviewgraphen.benchmark.prepare_real.v1",
            "cases": cases.into_iter().map(|case| serde_json::json!({
                "trial_unit_id":case.trial_unit_id,
                "manifests":case.manifests,
            })).collect::<Vec<_>>(),
        })),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run_prepare_real_full(
    public_dir: &Path,
    units_dir: &Path,
    output_dir: &Path,
    execution_config_path: &str,
    replicates: &str,
) -> ExitCode {
    let Ok(replicates) = replicates.parse::<u32>() else {
        eprintln!("replicates must be a positive integer");
        return ExitCode::from(2);
    };
    let execution = match read(execution_config_path)
        .and_then(|bytes| parse_execution_config(&bytes).map_err(|error| error.to_string()))
    {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    match reviewgraphen_benchmark::prepare::prepare_real_full(
        public_dir, units_dir, output_dir, &execution, replicates,
    ) {
        Ok(cases) => emit(&serde_json::json!({
            "schema":"reviewgraphen.benchmark.prepare_real_full.v1",
            "cases": cases.into_iter().map(|case| serde_json::json!({
                "trial_unit_id":case.trial_unit_id,
                "manifests":case.manifests,
            })).collect::<Vec<_>>(),
        })),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run_prepare_pilot(
    public_dir: &Path,
    output_dir: &Path,
    execution_config_path: &str,
    replicates: &str,
) -> ExitCode {
    let Ok(replicates) = replicates.parse::<u32>() else {
        eprintln!("replicates must be a positive integer");
        return ExitCode::from(2);
    };
    let execution = match read(execution_config_path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| parse_execution_config(&bytes).map_err(|error| error.to_string()))
    {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    match reviewgraphen_benchmark::prepare::prepare_pilot(
        public_dir, output_dir, &execution, replicates,
    ) {
        Ok(cases) => emit(&serde_json::json!({
            "schema":"reviewgraphen.benchmark.prepare_pilot.v1",
            "cases": cases.into_iter().map(|case| serde_json::json!({
                "unit_id":case.unit_id,
                "manifests":case.manifests,
            })).collect::<Vec<_>>(),
        })),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run_collect(manifest_path: &str, candidate_path: &str, output_path: &Path) -> ExitCode {
    let result = (|| {
        if output_path.exists() {
            return Err("collection output already exists".to_owned());
        }
        let manifest = parse_manifest(&read(manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        let candidate = parse_candidate(&read(candidate_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        let collected = collect_trial(&manifest, &candidate).map_err(|error| error.to_string())?;
        let bytes =
            reviewgraphen_core::canonical_json(&collected).map_err(|error| error.to_string())?;
        fs::write(output_path, &bytes).map_err(|error| error.to_string())?;
        Ok(collected)
    })();
    match result {
        Ok(value) => emit(&value),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn validate(kind: &str, path: &str) -> ExitCode {
    let bytes = match read(path) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let result =
        match kind {
            "execution" => parse_execution_config(&bytes)
                .and_then(|value| reviewgraphen_benchmark::canonical_hash(&value)),
            "manifest" => parse_manifest(&bytes)
                .and_then(|value| reviewgraphen_benchmark::canonical_hash(&value)),
            "candidate" => parse_candidate(&bytes)
                .and_then(|value| reviewgraphen_benchmark::canonical_hash(&value)),
            "oracle" => parse_oracle(&bytes)
                .and_then(|value| reviewgraphen_benchmark::canonical_hash(&value)),
            "collection" => parse_collection(&bytes)
                .and_then(|value| reviewgraphen_benchmark::canonical_hash(&value)),
            "inventory" => parse_inventory(&bytes)
                .and_then(|value| reviewgraphen_benchmark::canonical_hash(&value)),
            "score" => parse_score(&bytes)
                .and_then(|value| reviewgraphen_benchmark::canonical_hash(&value)),
            "real-unit" => parse_real_unit(&bytes)
                .and_then(|value| reviewgraphen_benchmark::canonical_hash(&value)),
            "real-oracle" => parse_real_oracle(&bytes)
                .and_then(|value| reviewgraphen_benchmark::canonical_hash(&value)),
            "real-inventory" => parse_real_inventory(&bytes)
                .and_then(|value| reviewgraphen_benchmark::canonical_hash(&value)),
            "real-score" => parse_real_score(&bytes)
                .and_then(|value| reviewgraphen_benchmark::canonical_hash(&value)),
            _ => {
                eprintln!("unknown artifact kind");
                return ExitCode::from(2);
            }
        };
    match result {
        Ok(hash) => emit(&serde_json::json!({"valid":true,"hash":hash})),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run_score(manifest_path: &str, candidate_path: &str, oracle_path: &str) -> ExitCode {
    let result = (|| {
        let manifest = parse_manifest(&read(manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        let candidate = parse_candidate(&read(candidate_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        let oracle = parse_oracle(&read(oracle_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        score(&manifest, &candidate, &oracle).map_err(|error| error.to_string())
    })();
    match result {
        Ok(value) => emit(&value),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run_summarize(inventory_path: &str, collections_path: &str, scores_path: &str) -> ExitCode {
    let result = (|| {
        let inventory =
            parse_inventory(&read(inventory_path)?).map_err(|error| error.to_string())?;
        let collections: Vec<TrialCollection> =
            serde_json::from_slice(&read(collections_path)?).map_err(|error| error.to_string())?;
        let scores: Vec<Score> =
            serde_json::from_slice(&read(scores_path)?).map_err(|error| error.to_string())?;
        summarize_run(&inventory, &collections, &scores).map_err(|error| error.to_string())
    })();
    match result {
        Ok(value) => emit(&value),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run_score_real(
    manifest_path: &str,
    candidate_path: &str,
    oracle_path: &str,
    unit_path: &str,
) -> ExitCode {
    let result = (|| {
        let manifest = parse_manifest(&read(manifest_path)?).map_err(|error| error.to_string())?;
        let candidate =
            parse_candidate(&read(candidate_path)?).map_err(|error| error.to_string())?;
        let oracle = parse_real_oracle(&read(oracle_path)?).map_err(|error| error.to_string())?;
        let unit = parse_real_unit(&read(unit_path)?).map_err(|error| error.to_string())?;
        score_real(&manifest, &candidate, &oracle, &unit).map_err(|error| error.to_string())
    })();
    match result {
        Ok(value) => emit(&value),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run_export_real_adjudication(
    manifest_path: &str,
    candidate_path: &str,
    oracle_path: &str,
    unit_path: &str,
    public_output: &Path,
    private_output: &Path,
) -> ExitCode {
    let result = (|| {
        let manifest = parse_manifest(&read(manifest_path)?).map_err(|error| error.to_string())?;
        let candidate =
            parse_candidate(&read(candidate_path)?).map_err(|error| error.to_string())?;
        let oracle = parse_real_oracle(&read(oracle_path)?).map_err(|error| error.to_string())?;
        let unit = parse_real_unit(&read(unit_path)?).map_err(|error| error.to_string())?;
        let (public, private) =
            blind_real_adjudication_export(&manifest, &candidate, &oracle, &unit)
                .map_err(|error| error.to_string())?;
        let public_bytes =
            reviewgraphen_core::canonical_json(&public).map_err(|error| error.to_string())?;
        let private_bytes =
            reviewgraphen_core::canonical_json(&private).map_err(|error| error.to_string())?;
        fs::write(public_output, public_bytes).map_err(|error| error.to_string())?;
        fs::write(private_output, private_bytes).map_err(|error| error.to_string())?;
        Ok::<_, String>(serde_json::json!({
            "exported_items": public.len(),
            "private_mappings": private.len()
        }))
    })();
    match result {
        Ok(value) => emit(&value),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run_validate_blind_adjudication(reconciliation_path: &str, decision_path: &str) -> ExitCode {
    let result = (|| {
        let reconciliation: Vec<PrivateAdjudicationReconciliation> =
            serde_json::from_slice(&read(reconciliation_path)?)
                .map_err(|error| error.to_string())?;
        let (decision, mapping) = import_blind_adjudication(&reconciliation, &read(decision_path)?)
            .map_err(|error| error.to_string())?;
        Ok::<_, String>(serde_json::json!({
            "valid": true,
            "item_id": decision.item_id,
            "trial_id": mapping.trial_id,
            "finding_local_id": mapping.finding_local_id
        }))
    })();
    match result {
        Ok(value) => emit(&value),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run_summarize_real(inventory_path: &str, collections_path: &str, scores_path: &str) -> ExitCode {
    let result = (|| {
        let inventory =
            parse_real_inventory(&read(inventory_path)?).map_err(|error| error.to_string())?;
        let collections: Vec<TrialCollection> =
            serde_json::from_slice(&read(collections_path)?).map_err(|error| error.to_string())?;
        let scores: Vec<RealScore> =
            serde_json::from_slice(&read(scores_path)?).map_err(|error| error.to_string())?;
        summarize_real_run(&inventory, &collections, &scores).map_err(|error| error.to_string())
    })();
    match result {
        Ok(value) => emit(&value),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run_summarize_real_full(
    inventory_path: &str,
    collections_path: &str,
    scores_path: &str,
) -> ExitCode {
    let result = (|| {
        let inventory =
            parse_real_inventory(&read(inventory_path)?).map_err(|error| error.to_string())?;
        let collections: Vec<TrialCollection> =
            serde_json::from_slice(&read(collections_path)?).map_err(|error| error.to_string())?;
        let scores: Vec<RealScore> =
            serde_json::from_slice(&read(scores_path)?).map_err(|error| error.to_string())?;
        summarize_real_full_run(&inventory, &collections, &scores)
            .map_err(|error| error.to_string())
    })();
    match result {
        Ok(value) => emit(&value),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
