import os
import io
import hashlib
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import patch

from evaluator.canonical import canonical_bytes, parse_json_bytes, sha256_bytes
import evaluator.pipeline as packet_pipeline
from evaluator.pipeline import PipelineError, _primary
from evaluator.freeze import freeze_manifest, write_generated
from evaluator.semantic_acceptance import ALGORITHM_SOURCE_SHA256, reference_body
from evaluator.stage0_driver import Cluster, Stage0Error, _ordered_terminal_builds, build_payload_hash, commit_cluster_id, enumerate_clusters, run_stage0, worker_ceiling
from evaluator.stage0_production import _ingest_admission_reason, _ingest_exclusion_or_fatal, _packet_v3_context_budget, run_frozen_cluster_pipeline
from evaluator.tests import support


REFERENCE_CLUSTER_ID = "commit-cluster:sha256:f140a5acf366f5e1195aed47206d17c9c858c8d5a4821181f1588aa02261787e"
USE_CURRENT = object()


def corpus(count=3):
    return [{"repository_id":"example/r", "repository_root":"/synthetic/r", "commits":[{"base_commit_oid":f"b{i}", "head_commit_oid":f"h{i}"} for i in range(count)]}]


def pipeline(cluster, _root):
    obligation = "obligation:" + cluster.commit_cluster_id[-8:]
    frozen = canonical_bytes({"schema":"m20.synthetic-frozen-obligation.v1","unit_id":cluster.commit_cluster_id,"obligation_id":obligation})
    (_root / "frozen-obligation.v1.json").write_bytes(frozen)
    value = {"schema":"m20.stage0-cluster-build.v1", "commit_cluster_id":cluster.commit_cluster_id, "repository_root":cluster.repository_root, "base_commit_oid":cluster.base_commit_oid, "head_commit_oid":cluster.head_commit_oid, "applicable_obligation_ids":[obligation], "subject_retained_obligation_ids":[obligation], "deferred_obligation_ids":[], "subject_remainders":[], "selected_obligation_id":obligation, "frozen_obligation_path":"frozen-obligation.v1.json", "frozen_obligation_sha256":sha256_bytes(frozen), "admitted_source_bytes":10, "whole_changed_production_files_bytes":20, "model_eligible":True, "enumeration_honest":True, "ingest_exclusion":None}
    value["deterministic_payload_sha256"] = build_payload_hash(value)
    return value


def output_tree(root):
    return {path.relative_to(root).as_posix(): path.read_bytes() for path in root.rglob("*") if path.is_file()}


class Stage0AdmissionTest(unittest.TestCase):
    def test_all_ingest_admission_bounds_are_typed_but_core_failure_is_fatal(self):
        ingest_failed={"schema":"reviewgraphen.generic_review_diagnostics.v1","stages":[{"stage":"ingest","status":"failed"}]}
        cases=(
            ("generic review ingestion failed: Git tree contains more than the configured 20000 regular-file bound","max_files"),
            ("generic review ingestion failed: snapshot source bytes total 67108865, above the 67108864 byte bound","snapshot_bytes"),
            ("generic review ingestion failed: Git blob `vendor/large.bin` is 4194305 bytes, above the 4194304 byte bound","blob_bytes"),
        )
        for stderr,reason in cases:
            with self.subTest(reason=reason): self.assertEqual(_ingest_admission_reason(stderr,ingest_failed),reason)
        context_failed={"schema":"reviewgraphen.generic_review_diagnostics.v1","stages":[{"stage":"ingest","status":"completed"},{"stage":"context","status":"failed"}]}
        self.assertIsNone(_ingest_admission_reason(cases[1][0],context_failed))
        self.assertIsNone(_ingest_admission_reason("generic review context failed: core validation failed",context_failed))
        with self.assertRaisesRegex(Stage0Error,"frozen_cluster_pipeline_failed"): _ingest_exclusion_or_fatal(20,"generic review context failed: core validation failed",context_failed)

    def test_ingest_exclusion_remains_in_complete_300_cluster_denominator(self):
        excluded_id=enumerate_clusters(corpus(300))[0].commit_cluster_id
        def observed(cluster,root):
            if cluster.commit_cluster_id!=excluded_id: return pipeline(cluster,root)
            value={"schema":"m20.stage0-cluster-build.v1","commit_cluster_id":cluster.commit_cluster_id,"repository_root":cluster.repository_root,"base_commit_oid":cluster.base_commit_oid,"head_commit_oid":cluster.head_commit_oid,"applicable_obligation_ids":[],"subject_retained_obligation_ids":[],"deferred_obligation_ids":[],"subject_remainders":[],"selected_obligation_id":"","frozen_obligation_path":"","frozen_obligation_sha256":"","admitted_source_bytes":0,"whole_changed_production_files_bytes":0,"model_eligible":False,"enumeration_honest":True,"ingest_exclusion":{"code":"stage0_ingest_admission_rejected","reason":"snapshot_bytes"}}
            value["deterministic_payload_sha256"]=build_payload_hash(value); return value
        with tempfile.TemporaryDirectory() as parent:
            root=Path(parent)/"stage0"; result=run_stage0(root,corpus(300),observed,jobs=1); summary=parse_json_bytes((root/"stage0-result.v1.json").read_bytes())
            self.assertEqual(summary["cluster_count"],300); self.assertEqual(summary["ingest_exclusion_counts"],{"max_files":0,"snapshot_bytes":1,"blob_bytes":0}); self.assertEqual(len(list((root/"clusters").iterdir())),300); self.assertNotIn(excluded_id,result["selection"].value["eligible_cluster_ids"])

    def test_product_max_files_admission_returns_a_sealed_exclusion(self):
        repository=Path(os.environ.get("M20_TEST_SOURCE_WORKSPACE",Path(__file__).parents[4])); cluster=Cluster("github.com/CAPHTECH/reviewgraphen",str(repository),"a8b6b24d5ed704f53f721b25db42d5d631f946c7","8569a2261e8a62145228872a2fde9f4c48093d00")
        with tempfile.TemporaryDirectory() as parent:
            stage_root=Path(parent)/"stage0"; build_root=stage_root/"clusters"/cluster.commit_cluster_id.rsplit(":",1)[1]/"build-1"; build_root.mkdir(parents=True); diagnostic_root=Path(tempfile.gettempdir())/"m20-stage0-diagnostics"/hashlib.sha256(str(stage_root.resolve()).encode()).hexdigest()
            try:
                value=run_frozen_cluster_pipeline(cluster,build_root,_max_files=1); self.assertEqual(value["ingest_exclusion"],{"code":"stage0_ingest_admission_rejected","reason":"max_files"})
                failure=parse_json_bytes((diagnostic_root/"clusters"/cluster.commit_cluster_id.rsplit(":",1)[1]/"build-1/pipeline-failure.v1.json").read_bytes()); self.assertEqual(failure["typed_reason"],"max_files"); self.assertNotEqual(failure["product_exit_code"],0)
                execution=parse_json_bytes((build_root/"product-execution.v1.json").read_bytes()); self.assertEqual((execution["typed_code"],execution["typed_reason"]),("stage0_ingest_admission_rejected","max_files")); request=parse_json_bytes((build_root/"pipeline-request.v3.json").read_bytes()); self.assertEqual(request["ingest"]["max_files"],1); self.assertFalse((build_root/"pipeline-artifacts").exists())
            finally:
                if diagnostic_root.exists(): shutil.rmtree(diagnostic_root)


class Stage0DriverTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.freeze_workspace = tempfile.TemporaryDirectory()
        workspace = Path(cls.freeze_workspace.name)
        cls.previous_sweep_worker = os.environ.get("M20_SWEEP_WORKER")
        fixture_suffix = "-stage0-freeze-" + workspace.name
        os.environ["M20_SWEEP_WORKER"] = fixture_suffix
        cls.previous_fixture_root = support.FIXTURE_ROOT
        support.FIXTURE_ROOT = Path("/tmp/m20-evaluator-pipeline-fixture-v1" + fixture_suffix)
        source_workspace = Path(os.environ.get("M20_TEST_SOURCE_WORKSPACE", Path(__file__).parents[4]))
        cls.source_preregistration = Path(__file__).parents[2] / "preregistration.json"
        cls.freeze_root = workspace / "benchmarks" / "m20-changed-public-callee-utility-v1"
        cls.freeze_root.mkdir(parents=True)
        shutil.copytree(Path(__file__).parents[1], cls.freeze_root / "evaluator")
        production = cls.freeze_root / "evaluator" / "stage0_production.py"
        production.write_text(production.read_text().replace(
            'ROOTS = (Path("/home/rizumita/github"),)',
            'ROOTS = (Path("/stage0-test-no-repositories"),)',
        ).replace(
            'REVIEWGRAPHEN = Path("/home/rizumita/workspace/reviewgraphen")',
            'REVIEWGRAPHEN = Path("/stage0-test-no-reviewgraphen")',
        ))
        if support.FIXTURE_ROOT.exists():
            shutil.rmtree(support.FIXTURE_ROOT)
        write_generated(cls.freeze_root / "evaluator" / "generated")
        shutil.copy2(Path(__file__).parents[2] / "EVALUATOR_SPEC.md", cls.freeze_root / "EVALUATOR_SPEC.md")
        shutil.copy2(cls.source_preregistration, cls.freeze_root / "preregistration.json")
        for relative in (*ALGORITHM_SOURCE_SHA256, reference_body()["measurement_path"]):
            destination = workspace / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source_workspace / relative, destination)
        cls.freeze_value = freeze_manifest(cls.freeze_root / "evaluator", cls.freeze_root / "EVALUATOR_SPEC.md")
        (cls.freeze_root / "freeze-manifest.json").write_bytes(canonical_bytes(cls.freeze_value))

    @classmethod
    def tearDownClass(cls):
        support.FIXTURE_ROOT = cls.previous_fixture_root
        if cls.previous_sweep_worker is None:
            os.environ.pop("M20_SWEEP_WORKER", None)
        else:
            os.environ["M20_SWEEP_WORKER"] = cls.previous_sweep_worker
        cls.freeze_workspace.cleanup()

    def _write_freeze_registration(self, *, path="freeze-manifest.json", manifest_hash=USE_CURRENT, bundle_hash=USE_CURRENT, null_active=False):
        raw = (self.freeze_root / "freeze-manifest.json").read_bytes()
        preregistration = parse_json_bytes(self.source_preregistration.read_bytes())
        active = preregistration["arm_neutral_contracts"]["freeze_hashes"]
        active["freeze_manifest_path"] = path
        active["freeze_manifest_sha256"] = sha256_bytes(raw) if manifest_hash is USE_CURRENT else manifest_hash
        for key in ("evaluator_bundle_sha256", "evaluator_execution_sha256", "generated_fixture_inventory_sha256", "reference_vector_set_sha256", "mutation_manifest_sha256", "semantic_acceptance_reference_sha256", "measurement_record_sha256"):
            active[key] = self.freeze_value[key]
        if null_active:
            for key in ("freeze_manifest_sha256", "evaluator_bundle_sha256", "evaluator_execution_sha256", "generated_fixture_inventory_sha256", "reference_vector_set_sha256", "mutation_manifest_sha256", "semantic_acceptance_reference_sha256", "measurement_record_sha256"):
                active[key] = None
        if bundle_hash is not USE_CURRENT:
            active["evaluator_bundle_sha256"] = bundle_hash
        (self.freeze_root / "preregistration.json").write_bytes(canonical_bytes(preregistration))

    def _run_public_stage0(self):
        output = self.freeze_root / "stage0-output"
        if output.exists():
            shutil.rmtree(output)
        environment = {**os.environ, "PYTHONPATH":str(self.freeze_root), "PATH":os.environ.get("PATH", "")}
        result = subprocess.run(
            [sys.executable, "-m", "evaluator", "stage0", str(output)],
            cwd=self.freeze_root,
            env=environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=180,
        )
        self.assertEqual(result.stderr, b"")
        return result.returncode, parse_json_bytes(result.stdout), output

    def test_stage0_freeze_gate_rejects_current_null_preregistration(self):
        cases = (
            ("null", {"null_active":True}, (3, "stage0_freeze_input_invalid")),
            ("matching", {}, (2, "corpus_repository_count_invalid")),
            ("mismatch", {"manifest_hash":"sha256:" + "0" * 64}, (3, "stage0_active_freeze_hash_mismatch")),
        )
        for name, registration, expected in cases:
            with self.subTest(name=name):
                self._write_freeze_registration(**registration)
                exit_code, result, output = self._run_public_stage0()
                self.assertEqual((exit_code, result["code"]), expected)
                self.assertFalse(output.exists())

    def test_stage0_freeze_gate_rejects_replaced_manifest_path(self):
        replacement = self.freeze_root / "replacement-freeze-manifest.json"
        replacement.write_bytes(canonical_bytes({**self.freeze_value, "evaluator_bundle_sha256":"sha256:" + "1" * 64}))
        self._write_freeze_registration(path=replacement.name)
        exit_code, result, output = self._run_public_stage0()
        self.assertEqual((exit_code, result["code"]), (3, "stage0_active_freeze_hash_mismatch"))
        self.assertFalse(output.exists())

    def test_stage0_freeze_gate_rejects_real_bundle_byte_tamper(self):
        self._write_freeze_registration()
        target = self.freeze_root / "evaluator" / "canonical.py"
        original = target.read_bytes()
        target.write_bytes(original.replace(b"Restricted canonical", b"Restricted canonicaL", 1))
        try:
            exit_code, result, output = self._run_public_stage0()
            self.assertEqual((exit_code, result["code"]), (3, "stage0_freeze_verification_failed"))
            self.assertFalse(output.exists())
        finally:
            target.write_bytes(original)

    def test_reference_vector_is_literal_and_detects_identity_mutation(self):
        self.assertEqual(commit_cluster_id("github.com/example/repository", "0123456789abcdef0123456789abcdef01234567", "89abcdef0123456789abcdef0123456789abcdef"), REFERENCE_CLUSTER_ID)
        with patch("evaluator.stage0_driver.stable_id", return_value="commit-cluster:sha256:" + "0" * 64):
            self.assertNotEqual(commit_cluster_id("github.com/example/repository", "0123456789abcdef0123456789abcdef01234567", "89abcdef0123456789abcdef0123456789abcdef"), REFERENCE_CLUSTER_ID)

    def test_enumeration_is_deterministic_and_input_order_independent(self):
        first = enumerate_clusters(corpus(), 3)
        reversed_input = corpus(); reversed_input[0]["commits"].reverse()
        second = enumerate_clusters(reversed_input, 3)
        self.assertEqual(first, second)

    def test_incomplete_duplicate_and_existing_root_are_typed(self):
        with self.assertRaisesRegex(Stage0Error, "commit_cluster_count_invalid"): enumerate_clusters(corpus(2), 3)
        with self.assertRaisesRegex(Stage0Error, "commit_cluster_count_invalid"): enumerate_clusters(corpus(299))
        duplicate = corpus(3); duplicate[0]["commits"][1] = dict(duplicate[0]["commits"][0])
        with self.assertRaisesRegex(Stage0Error, "commit_cluster_duplicate"): enumerate_clusters(duplicate, 3)
        duplicate_300 = corpus(300); duplicate_300[0]["commits"][299] = dict(duplicate_300[0]["commits"][0])
        with self.assertRaisesRegex(Stage0Error, "commit_cluster_duplicate"): enumerate_clusters(duplicate_300)
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaisesRegex(Stage0Error, "output_exists"): run_stage0(root, corpus(), pipeline, 3)

    def test_selection_type_is_rejected_by_primary_scorer(self):
        with tempfile.TemporaryDirectory() as parent:
            result = run_stage0(Path(parent) / "new", corpus(), pipeline, 3)
            with self.assertRaisesRegex(PipelineError, "stage0_selection_not_score_input"): _primary(result["selection"], {"scores":[]})
            with self.assertRaisesRegex(PipelineError, "stage0_selection_not_score_input"): _primary([], result["selection"])

    def test_stage0_packet_v3_eligibility_includes_large_shared_core(self):
        class Repository:
            def baseline_specs(self, _trees): return [{"required_id":"core","role":"changed","snapshot_side":"head","path":"src/core.rs","start_line":1,"end_line":1,"blob_oid":"a"*40}]
            def blob_for(self, _trees, _side, path, _oid): return (b"x"*65536+b"\n") if path == "src/core.rs" else b"fn callee() {}\n"
        trees=({}, {"src/core.rs":("100644","a"*40),"src/callee.rs":("100644","b"*40)})
        context={"materialized_sources":[{"artifact_id":"file:callee","path":"src/callee.rs"}],"subject_outcomes":[{"role":"callee","state":"admitted","source_artifact_id":"file:callee","requested_range":{"start_line":1,"end_line":1}}],"windows":[{"source_artifact_id":"file:callee","range":{"start_line":1,"end_line":1},"roles":["callee"]}]}
        self.assertEqual(_packet_v3_context_budget(Repository(),trees,context),(65552,65552,False))

    def test_stage0_calls_the_shared_packet_v3_accounting_function_for_synthetic_corpus(self):
        calls = packet_pipeline._PACKET_V3_ACCOUNT_CALLS
        for core_bytes, window_bytes in ((8,4),(65536,4),(65537,1),(65530,16)):
            with self.subTest(core_bytes=core_bytes,window_bytes=window_bytes):
                class Repository:
                    def baseline_specs(self, _trees): return [{"required_id":"core","role":"changed","snapshot_side":"head","path":"src/core.rs","start_line":1,"end_line":1,"blob_oid":"a"*40}]
                    def blob_for(self, _trees, _side, path, _oid): return (b"c"*(core_bytes-1)+b"\n") if path == "src/core.rs" else (b"w"*(window_bytes-1)+b"\n")
                repository=Repository(); trees=({}, {"src/core.rs":("100644","a"*40),"src/window.rs":("100644","b"*40)})
                context={"materialized_sources":[{"artifact_id":"file:window","path":"src/window.rs"}],"subject_outcomes":[{"role":"callee","state":"admitted","source_artifact_id":"file:window","requested_range":{"start_line":1,"end_line":1}}],"windows":[{"source_artifact_id":"file:window","range":{"start_line":1,"end_line":1},"roles":["callee"]}]}
                stage0_counts = _packet_v3_context_budget(repository,trees,context)
                self.assertEqual(stage0_counts[2],max(stage0_counts[:2]) <= 65536)
        self.assertEqual(packet_pipeline._PACKET_V3_ACCOUNT_CALLS,calls+4)

    def test_three_cluster_end_to_end_closed_layout_and_no_models(self):
        calls = []
        def observed(cluster, root): calls.append((cluster.commit_cluster_id, root.name)); return pipeline(cluster, root)
        with tempfile.TemporaryDirectory() as parent:
            root = Path(parent) / "new"; run_stage0(root, corpus(), observed, 3)
            self.assertEqual(len(calls), 6)
            result = parse_json_bytes((root / "stage0-result.v1.json").read_bytes())
            self.assertEqual(result["model_calls"], 0)
            self.assertTrue(all(item["passed"] for item in result["gates"]))
            self.assertEqual(len(list((root / "clusters").glob("*/build-*"))), 6)

    def test_real_fixture_cluster_passes_product_cli_artifact_admission(self):
        repository = Path(os.environ.get("M20_TEST_SOURCE_WORKSPACE", Path(__file__).parents[4]))
        cluster = Cluster(
            "github.com/CAPHTECH/reviewgraphen",
            str(repository),
            "a8b6b24d5ed704f53f721b25db42d5d631f946c7",
            "8569a2261e8a62145228872a2fde9f4c48093d00",
        )
        with tempfile.TemporaryDirectory() as parent:
            stage_root = Path(parent) / "stage0"
            build_root = stage_root / "clusters" / cluster.commit_cluster_id.rsplit(":", 1)[1] / "build-1"
            build_root.mkdir(parents=True)
            diagnostic_root = Path(tempfile.gettempdir()) / "m20-stage0-diagnostics" / hashlib.sha256(str(stage_root.resolve()).encode()).hexdigest()
            try:
                value = run_frozen_cluster_pipeline(cluster, build_root)
                self.assertEqual(value["schema"], "m20.stage0-cluster-build.v1")
                self.assertEqual(value["commit_cluster_id"], cluster.commit_cluster_id)
                request = parse_json_bytes((build_root / "pipeline-request.v3.json").read_bytes())
                self.assertEqual(request["ingest"]["max_files"], 20000)
                self.assertTrue((build_root / "pipeline-artifacts" / "artifact-manifest.v1.json").is_file())
                diagnostic = parse_json_bytes((diagnostic_root / "clusters" / cluster.commit_cluster_id.rsplit(":", 1)[1] / "build-1" / "generic-review-diagnostics.v1.json").read_bytes())
                self.assertEqual(diagnostic["schema"], "reviewgraphen.generic_review_diagnostics.v1")
            finally:
                if diagnostic_root.exists(): shutil.rmtree(diagnostic_root)

    def test_product_cli_failure_is_external_and_identifies_cluster(self):
        repository = Path(os.environ.get("M20_TEST_SOURCE_WORKSPACE", Path(__file__).parents[4]))
        cluster = Cluster("github.com/CAPHTECH/reviewgraphen", str(repository), "a8b6b24d5ed704f53f721b25db42d5d631f946c7", "0" * 40)
        with tempfile.TemporaryDirectory() as parent:
            stage_root = Path(parent) / "stage0"
            build_root = stage_root / "clusters" / cluster.commit_cluster_id.rsplit(":", 1)[1] / "build-1"
            build_root.mkdir(parents=True)
            diagnostic_root = Path(tempfile.gettempdir()) / "m20-stage0-diagnostics" / hashlib.sha256(str(stage_root.resolve()).encode()).hexdigest()
            try:
                with self.assertRaisesRegex(Stage0Error, "frozen_cluster_pipeline_failed") as raised:
                    run_frozen_cluster_pipeline(cluster, build_root)
                self.assertEqual(raised.exception.diagnostic["commit_cluster_id"], cluster.commit_cluster_id)
                failure = parse_json_bytes((diagnostic_root / "clusters" / cluster.commit_cluster_id.rsplit(":", 1)[1] / "build-1" / "pipeline-failure.v1.json").read_bytes())
                self.assertEqual(failure["commit_cluster_id"], cluster.commit_cluster_id)
                self.assertNotEqual(failure["product_exit_code"], 0)
                self.assertEqual(failure["typed_reason"], "product_cli_error")
                self.assertTrue(failure["product_stderr"])
                self.assertFalse((build_root / "pipeline-artifacts").exists())
            finally:
                if diagnostic_root.exists(): shutil.rmtree(diagnostic_root)

    def test_product_cli_timeout_records_partial_failure_diagnostic(self):
        repository = Path(os.environ.get("M20_TEST_SOURCE_WORKSPACE", Path(__file__).parents[4]))
        cluster = Cluster("github.com/CAPHTECH/reviewgraphen", str(repository), "a8b6b24d5ed704f53f721b25db42d5d631f946c7", "8569a2261e8a62145228872a2fde9f4c48093d00")
        with tempfile.TemporaryDirectory() as parent:
            stage_root = Path(parent) / "stage0"
            build_root = stage_root / "clusters" / cluster.commit_cluster_id.rsplit(":", 1)[1] / "build-1"
            build_root.mkdir(parents=True)
            diagnostic_root = Path(tempfile.gettempdir()) / "m20-stage0-diagnostics" / hashlib.sha256(str(stage_root.resolve()).encode()).hexdigest()
            try:
                with self.assertRaisesRegex(Stage0Error, "frozen_cluster_pipeline_failed") as raised:
                    run_frozen_cluster_pipeline(cluster, build_root, _pipeline_timeout=0.000001)
                self.assertEqual(raised.exception.diagnostic["typed_reason"], "timeout")
                self.assertIsNone(raised.exception.diagnostic["product_exit_code"])
                failure = parse_json_bytes((diagnostic_root / "clusters" / cluster.commit_cluster_id.rsplit(":", 1)[1] / "build-1" / "pipeline-failure.v1.json").read_bytes())
                self.assertEqual(failure["commit_cluster_id"], cluster.commit_cluster_id)
                self.assertEqual(failure["typed_reason"], "timeout")
                self.assertIsNone(failure["product_exit_code"])
                self.assertIn("product_stderr", failure)
                self.assertGreater(float(failure["elapsed_seconds"]), 0)
                self.assertFalse((build_root / "pipeline-artifacts").exists())
            finally:
                if diagnostic_root.exists(): shutil.rmtree(diagnostic_root)

    def test_cli_stage0_failure_writes_cluster_json_to_stderr(self):
        from evaluator import cli
        diagnostic = {"schema":"m20.stage0-error-diagnostic.v1", "code":"frozen_cluster_pipeline_failed", "commit_cluster_id":"commit-cluster:sha256:" + "1" * 64, "product_exit_code":20}
        class Stream:
            def __init__(self): self.buffer = io.BytesIO()
        stdout, stderr = Stream(), Stream()
        with patch("evaluator.stage0_driver.production_stage0", side_effect=Stage0Error("frozen_cluster_pipeline_failed", diagnostic=diagnostic)), patch.object(sys, "stdout", stdout), patch.object(sys, "stderr", stderr):
            self.assertEqual(cli.main(["stage0", "/unused"]), 4)
        self.assertEqual(parse_json_bytes(stdout.buffer.getvalue()), {"schema":"m20.cli-error.v1", "code":"frozen_cluster_pipeline_failed"})
        self.assertEqual(parse_json_bytes(stderr.buffer.getvalue()), diagnostic)

    def test_worker_ceiling_contract_and_jobs_bounds(self):
        self.assertEqual(worker_ceiling(1), 1)
        self.assertEqual(worker_ceiling(2), 1)
        self.assertEqual(worker_ceiling(3), 1)
        self.assertEqual(worker_ceiling(18), 16)
        self.assertEqual(worker_ceiling(200), 16)
        self.assertEqual(worker_ceiling(None), max(1, min(16, (os.cpu_count() or 1) - 2)))
        with patch("evaluator.stage0_driver.os.cpu_count", return_value=None):
            self.assertEqual(worker_ceiling(), 1)
        with tempfile.TemporaryDirectory() as parent, patch("evaluator.stage0_driver.os.cpu_count", return_value=4):
            for name, jobs in (("zero", 0), ("above-ceiling", 3)):
                with self.subTest(jobs=jobs), self.assertRaisesRegex(Stage0Error, "stage0_jobs_invalid"):
                    run_stage0(Path(parent) / name, corpus(), pipeline, 3, jobs=jobs)

    def test_width_one_ceiling_and_reversed_completion_are_byte_identical(self):
        completed = []

        class ReverseCompletionExecutor:
            def __init__(self, **_kwargs):
                self.pool = ThreadPoolExecutor(max_workers=300)
                self.condition = threading.Condition()
                self.arrived = set()
                self.events = {}
                self.controller = None

            def __enter__(self):
                return self

            def submit(self, function, cluster, *args):
                event = self.events.setdefault(cluster.commit_cluster_id, threading.Event())
                cluster_root = args[0]

                def blocked():
                    result = function(cluster, *args)
                    with self.condition:
                        self.arrived.add(cluster.commit_cluster_id)
                        self.condition.notify_all()
                    event.wait()
                    return result

                def record_done(future):
                    future.result()
                    digest = cluster.commit_cluster_id.rsplit(":", 1)[1]
                    directory = cluster_root / digest
                    self.assert_cluster_outputs(directory)
                    with self.condition:
                        completed.append(cluster.commit_cluster_id)
                        self.condition.notify_all()

                future = self.pool.submit(blocked)
                future.add_done_callback(record_done)
                if len(self.events) == 300:
                    self.controller = threading.Thread(target=self.release_reversed)
                    self.controller.start()
                return future

            @staticmethod
            def assert_cluster_outputs(directory):
                for number in (1, 2):
                    if not (directory / f"build-{number}" / "cluster-result.v1.json").is_file():
                        raise AssertionError("completion_recorded_before_cluster_output")

            def release_reversed(self):
                order = sorted(self.events, key=str.encode, reverse=True)
                with self.condition:
                    self.condition.wait_for(lambda: len(self.arrived) == 300)
                for identity in order:
                    self.events[identity].set()
                    with self.condition:
                        observed = self.condition.wait_for(lambda: completed and completed[-1] == identity, timeout=5)
                    if not observed:
                        for event in self.events.values():
                            event.set()
                        return

            def __exit__(self, *_args):
                if self.controller is not None:
                    self.controller.join()
                self.pool.shutdown()

        with tempfile.TemporaryDirectory() as parent, patch("evaluator.stage0_driver.os.cpu_count", return_value=8):
            roots = [Path(parent) / name for name in ("one", "ceiling", "reversed")]
            first = run_stage0(roots[0], corpus(300), pipeline, jobs=1)
            second = run_stage0(roots[1], corpus(300), pipeline, jobs=worker_ceiling())
            third = run_stage0(roots[2], corpus(300), pipeline, jobs=worker_ceiling(), _executor_factory=ReverseCompletionExecutor)
            trees = [output_tree(root) for root in roots]
            self.assertEqual(trees[0], trees[1])
            self.assertEqual(trees[0], trees[2])
            self.assertEqual(first["selection"].value["selection_sha256"], second["selection"].value["selection_sha256"])
            self.assertEqual(first["selection"].value["selection_sha256"], third["selection"].value["selection_sha256"])
            expected_completion = sorted((cluster.commit_cluster_id for cluster in enumerate_clusters(corpus(300))), key=str.encode, reverse=True)
            self.assertEqual(completed, expected_completion)
            self.assertNotIn(b'"jobs"', b"".join(trees[2].values()))

    def test_terminal_reduction_ignores_reverse_completion_order(self):
        reverse_completed = {identity: {"commit_cluster_id": identity} for identity in ("c", "b", "a")}
        ordered = _ordered_terminal_builds(reverse_completed, {"a", "b", "c"}, 3)
        self.assertEqual([item["commit_cluster_id"] for item in ordered], ["a", "b", "c"])
