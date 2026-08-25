import inspect
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).parents[2]
sys.path.insert(0, str(ROOT))

from evaluator.budget import BudgetExceeded, enforce
from evaluator.canonical import canonical_bytes, load, sha256
import evaluator.harness as harness_module
from evaluator.harness import HarnessError, TreatmentSource, arm_c, arm_workspace, dry_run, request
from evaluator.oracle import OracleError, derive
from evaluator.packet import build
from evaluator.product import (
    PRODUCT_CLI_SHA256,
    ProductError,
    _request,
    _validate_product_output,
    expected_snapshot_id,
    run_product_review,
)
from evaluator.scoring import ScoreError, score

V = ROOT / "evaluator/reference_vectors"
FIXTURES = ROOT / "evaluator/tests/fixtures"


def _git(repo, *args, env=None):
    merged = os.environ.copy()
    merged.update(env or {})
    return subprocess.run(
        ["/usr/bin/git", "-C", str(repo), *args],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=merged,
    ).stdout.strip()


class Contract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="m21-contract-")
        cls.repo = pathlib.Path(cls.temporary.name) / "repository"
        cls.repo.mkdir()
        _git(cls.repo, "init", "-q")
        dates = ["2001-01-01T00:00:00+0000", "2001-01-02T00:00:00+0000", "2001-01-03T00:00:00+0000"]
        for index, (name, message, path) in enumerate((
            ("seed.rs", "seed", "legacy.rs"),
            ("base.rs", "base", "legacy.rs"),
            ("fix.rs", "fix", "current.rs"),
        )):
            if message == "fix":
                (cls.repo / "legacy.rs").unlink()
            shutil.copyfile(FIXTURES / name, cls.repo / path)
            _git(cls.repo, "add", "-A")
            identity = {
                "GIT_AUTHOR_NAME":"m21 fixture",
                "GIT_AUTHOR_EMAIL":"m21@example.invalid",
                "GIT_COMMITTER_NAME":"m21 fixture",
                "GIT_COMMITTER_EMAIL":"m21@example.invalid",
                "GIT_AUTHOR_DATE":dates[index],
                "GIT_COMMITTER_DATE":dates[index],
            }
            _git(cls.repo, "commit", "-q", "-m", message, env=identity)
            setattr(cls, message + "_oid", _git(cls.repo, "rev-parse", "HEAD"))
        cls.base_tree_oid = _git(cls.repo, "rev-parse", cls.base_oid + "^{tree}")
        cls.snapshot_id = expected_snapshot_id("m21-fixture", cls.base_oid, cls.base_tree_oid)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def test_oracle_api_has_no_cli_audit_or_artifact_injection(self):
        self.assertEqual(list(inspect.signature(derive).parameters), ["repository", "repository_id", "base_oid", "fix_oid"])

    def test_fake_cli_is_typed_rejection(self):
        fake = pathlib.Path(self.temporary.name) / "fake-reviewgraphen"
        fake.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        fake.chmod(0o700)
        with mock.patch("evaluator.product.PRODUCT_CLI", fake):
            with self.assertRaises(OracleError) as raised:
                derive(self.repo, "m21-fixture", self.base_oid, self.fix_oid)
        self.assertEqual(raised.exception.record["code"], "product_cli_identity_mismatch")

    def test_tampered_product_audit_is_typed_rejection(self):
        product = run_product_review(self.repo, "m21-fixture", self.base_oid, self.fix_oid)
        audit = dict(product.audit)
        valid_audit_bytes = canonical_bytes(audit)
        request_bytes = canonical_bytes(_request("m21-fixture", self.base_oid, self.fix_oid))
        manifest = {
            "schema":"reviewgraphen.generic_review_artifact_manifest.v1",
            "artifacts":[{"path":"audit.run.v3.json","sha256":sha256(valid_audit_bytes),"byte_length":len(valid_audit_bytes)}],
            "request_sha256":sha256(request_bytes),
            "snapshot_id":product.snapshot_id,
            "universe_id":product.universe_id,
            "request_id":audit["request_id"],
            "run_id":audit["run_id"],
        }
        diagnostics_bytes = canonical_bytes(product.diagnostics)
        audit["authority"] = {**audit["authority"], "trusted_pass":True}
        with self.assertRaises(ProductError) as raised:
            _validate_product_output(
                self.repo,
                "m21-fixture",
                self.base_oid,
                self.fix_oid,
                request_bytes,
                canonical_bytes(audit),
                canonical_bytes(manifest),
                diagnostics_bytes,
            )
        self.assertEqual(raised.exception.record["code"], "product_audit_hash_mismatch")

    def test_real_fixture_derives_old_match_and_nonlocal_reference(self):
        first = derive(self.repo, "m21-fixture", self.base_oid, self.fix_oid)
        second = derive(self.repo, "m21-fixture", self.base_oid, self.fix_oid)
        self.assertEqual(first, second)
        vector = load(V / "oracle.v1.json")
        self.assertEqual({key:first[key] for key in vector["expected"]}, vector["expected"])
        self.assertEqual(self.base_oid, vector["input"]["base_oid"])
        self.assertEqual(self.fix_oid, vector["input"]["fix_oid"])
        self.assertTrue(set(first["reference_symbol_ids"]) <= set(first["symbol_ids"]))
        self.assertTrue(set(first["reference_symbol_ids"]).isdisjoint(first["t_match_symbol_ids"]))
        self.assertTrue(set(first["reference_symbol_ids"]).isdisjoint(first["t_old_symbol_ids"]))
        self.assertNotEqual(set(first["t_old_symbol_ids"]), set(first["t_match_symbol_ids"]))
        retired = next(row for row in first["symbol_sources"] if row["name"] == "retired")
        self.assertEqual(retired["path"], "legacy.rs")
        self.assertEqual(retired["memberships"], ["T_old"])
        self.assertGreater(len(first["t_old_symbol_ids"]), len(first["t_match_symbol_ids"]))
        self.assertGreater(first["evaluation_line_count"], 0)
        self.assertEqual(first["product_cli_sha256"], PRODUCT_CLI_SHA256)

    def test_line_f1_and_subject_removal(self):
        context = {"context_items":[{"path":"a","start_line":1,"end_line":2}],"coverage_claim":"complete","declared_losses":[]}
        result = dry_run(load(V / "task.v1.json"))
        got = result.score(context, {("a",2),("a",3)}, {("a",1)})
        self.assertEqual(got["f1"], {"numerator":2,"denominator":3})
        self.assertTrue(got["false_complete"])

    def test_zero_coverage_cost_is_infinite(self):
        context = {"context_items":[],"coverage_claim":"unknown","declared_losses":[]}
        got = dry_run(load(V / "task.v1.json")).score(context, {("a",1)}, set())
        self.assertEqual(got["tokens_per_covered_line"], {"infinite":True})

    def test_forged_measurement_is_rejected_by_type_and_run_key(self):
        context = {"context_items":[],"coverage_claim":"unknown","declared_losses":[]}
        forged = {"schema":"m21.harness_measurement.v1","input_tokens":-1,"output_tokens":-2,"elapsed_ns":-3}
        with self.assertRaises(ScoreError) as raised:
            score(context, {("a",1)}, set(), forged)
        self.assertEqual(str(raised.exception), "harness_scoring_required")
        self.assertFalse(hasattr(harness_module, "_HarnessMeasurement"))
        self.assertFalse(hasattr(harness_module, "_MEASUREMENT_TOKEN"))
        result = dry_run(load(V / "task.v1.json"))
        measurement = result.measurement
        with self.assertRaises(HarnessError) as raised:
            type(measurement)(object(), {}, b"forged")
        self.assertEqual(str(raised.exception), "harness_measurement_private")
        with self.assertRaises(AttributeError):
            measurement.input_tokens = -1
        object.__setattr__(measurement, "_tag", b"\x00" * 32)
        with self.assertRaises(HarnessError) as raised:
            result.score(context, {("a",1)}, set())
        self.assertEqual(str(raised.exception), "harness_measurement_authentication_failed")

    def test_every_budget_dimension_is_typed(self):
        for name, limit in (("input_tokens",65536),("output_tokens",24000),("wall_seconds",1800),("tool_calls",32),("tool_response_bytes",16384)):
            enforce(name, limit)
            with self.assertRaises(BudgetExceeded) as raised:
                enforce(name, limit + 1)
            self.assertEqual(raised.exception.record["dimension"], name)

    def test_arm_a_builds_bytes_without_http(self):
        result = dry_run(
            load(V / "task.v1.json"),
            TreatmentSource(self.repo, "m21-fixture", self.base_oid),
            arm="A",
        )
        record = result.record()
        self.assertFalse(record["http_sent"])
        self.assertGreater(record["request_bytes"], 0)
        self.assertEqual(result.measurement.product_ingest_operations, 0)

    def test_arms_share_base_only_filesystem_without_fix_or_history(self):
        source = TreatmentSource(self.repo, "m21-fixture", self.base_oid)
        inventories = []
        for arm in ("A", "B"):
            with arm_workspace(source, arm) as workspace:
                history = _git(workspace, "rev-list", "--all", "--parents").splitlines()
                self.assertEqual(history, [self.base_oid])
                self.assertEqual((workspace / ".git/shallow").read_text().strip(), self.base_oid)
                for forbidden in (self.seed_oid, self.fix_oid):
                    probe = subprocess.run(
                        ["/usr/bin/git", "-C", str(workspace), "cat-file", "-e", forbidden + "^{commit}"],
                        stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE,
                    )
                    self.assertNotEqual(probe.returncode, 0)
                files = [path for path in workspace.rglob("*") if path.is_file()]
                self.assertFalse(any("oracle" in path.name.lower() for path in files))
                self.assertFalse(any(self.fix_oid.encode() in path.read_bytes() for path in files))
                self.assertFalse(any(str(self.repo).encode() in path.read_bytes() for path in files))
                inventory = _git(workspace, "cat-file", "--batch-check", "--batch-all-objects")
                inventories.append(inventory)
        self.assertEqual(inventories[0], inventories[1])

    def test_arm_b_waits_for_task_subject_product_entry(self):
        with self.assertRaises(HarnessError) as raised:
            dry_run(
                load(V / "task.v1.json"),
                TreatmentSource(self.repo, "m21-fixture", self.base_oid),
                arm="B",
            )
        self.assertEqual(str(raised.exception), "task_subject_projection_pending")

    def test_huge_public_task_is_typed_input_budget_failure(self):
        task = {"task_id":"t","snapshot_id":"s","task_kind":"symptom_fix","title":"x" * 300000}
        with self.assertRaises(BudgetExceeded) as raised:
            dry_run(task)
        self.assertEqual(raised.exception.record["dimension"], "input_tokens")

    def test_leakage_fields_cannot_enter_either_arm(self):
        secret = self.fix_oid
        hostile = {"task_id":"t","snapshot_id":"s","task_kind":"symptom_fix","title":"repair","fix_oid":secret}
        with self.assertRaises(HarnessError):
            request(hostile)
        hidden_in_title = {"task_id":"t","snapshot_id":"s","task_kind":"symptom_fix","title":"repair " + secret}
        with self.assertRaises(HarnessError):
            request(hidden_in_title)
        leaking_packet = {"oracle_id":"oracle:secret","context_items":[],"coverage_claim":"unknown","declared_losses":[]}
        public = {"task_id":"t","snapshot_id":"s","task_kind":"symptom_fix","title":"repair"}
        with self.assertRaises(HarnessError):
            request(public, leaking_packet)
        with self.assertRaises(HarnessError):
            arm_c(public, leaking_packet)
        leaking_value = {"other":secret,"context_items":[],"coverage_claim":"unknown","declared_losses":[]}
        with self.assertRaises(HarnessError):
            request(public, leaking_value)
        self.assertNotIn("fix", TreatmentSource.__dataclass_fields__)
        subject_id = load(V / "oracle.v1.json")["expected"]["t_match_symbol_ids"][0]
        task = {"task_id":"task:treatment","snapshot_id":self.snapshot_id,"task_kind":"symbol_change","title":"Improve normalization","subject_symbol_id":subject_id}
        result = dry_run(task, TreatmentSource(self.repo, "m21-fixture", self.base_oid), arm="A")
        wire = canonical_bytes(result.record())
        self.assertNotIn(secret.encode(), wire)
        self.assertNotIn(b"oracle", wire)
        self.assertNotIn(b"history", wire)

    def test_packet_is_reorder_and_incremental_invariant(self):
        task = {"task_id":"t","snapshot_id":"s"}
        facts = [
            {"fact_id":"2","distance":1,"path":"b.rs","start_line":4,"end_line":5,"symbol_id":"b","relation":"callee","binding":"resolved","source_id":"src:b"},
            {"fact_id":"1","distance":0,"path":"a.rs","start_line":1,"end_line":2,"symbol_id":"a","relation":"subject","binding":"resolved","source_id":"src:a"},
        ]
        clean = build(task, facts)
        incremental = build(task, [facts[1], facts[0]])
        self.assertEqual(clean, incremental)
        self.assertEqual(clean["context_items"][0]["reason"], "subject")


if __name__ == "__main__":
    unittest.main()
