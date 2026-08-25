import copy
import tempfile
import unittest
from pathlib import Path

from evaluator.canonical import canonical_bytes
from evaluator.freeze import file_records
from evaluator.model_boundary import ModelResult, TypedError, decode_model
from evaluator.pipeline import PipelineError, _batch, _mechanical, _obligation, _observations, _result
from evaluator.repository import GitRepository, PreflightError
from .attack_probe import _judge_batch, _judge_response, _reviewer_response, _valid_obligation


class H6RegressionTest(unittest.TestCase):
    def assert_typed(self, operation, error_type):
        with self.assertRaises(error_type):
            operation()

    def test_y01_y02_y03_reviewer_cross_field_contracts(self):
        value = _reviewer_response()
        value["disposition"]["claims"][0]["observations"][0].update(start_line=2, end_line=1)
        self.assert_typed(lambda: decode_model(canonical_bytes(value), "reviewer", {"task_id":"t", "source_inventory_id":"i"}), TypedError)
        for field in ("task_id", "source_inventory_id"):
            value = _reviewer_response()
            value[field] = "foreign"
            self.assert_typed(lambda value=value: decode_model(canonical_bytes(value), "reviewer", {"task_id":"t", "source_inventory_id":"i"}), TypedError)

    def test_y05_y06_judge_conjunctions(self):
        batch = _judge_batch()
        value = _judge_response(batch)
        value["scores"][0]["total"] -= 1
        self.assert_typed(lambda: decode_model(canonical_bytes(value), "judge", {"batch":batch}), TypedError)
        batch = _judge_batch(forced=True)
        value = _judge_response(batch)
        value["scores"][1]["verdict"] = "usable"
        self.assert_typed(lambda: decode_model(canonical_bytes(value), "judge", {"batch":batch}), TypedError)

    def test_y08_y09_y10_y11_y12_policy_and_observation(self):
        packet = {"task_id":"t", "source_inventory":{"admitted_sources":[{"source_id":"s", "role":"changed", "range":{"start_line":2, "end_line":3}}], "declared_losses":[]}}
        self.assertIn("observation_range_invalid", _observations([{"source_id":"s", "start_line":1, "end_line":3}], packet))
        arm = {"hidden_arm_id":"h"}
        opportunity = {"comparable":True}
        for result, code in ((ModelResult(b"x", tool_calls=("tool",)), "tool_violation"), (ModelResult(b"x", provider_truncation=True), "provider_truncation")):
            score = _mechanical(packet, arm, opportunity, result, None, [], "u", "b")
            self.assertIn(code, score["failure_codes"])
            self.assertFalse(score["policy_valid"])
        self.assert_typed(lambda: _result(ModelResult(b"x", process_exit=True)), PipelineError)
        loss = {"loss_id":"l", "reason":"task_blocking_reference_unresolved", "primary_abstention_eligible":True}
        packet["source_inventory"]["declared_losses"] = [loss]
        decoded = type("Decoded", (), {"value":{"disposition":{"kind":"abstention", "abstention":{"reason":"task_blocking_source_unavailable", "basis_loss_ids":["l"], "observations":[{"source_id":"s", "start_line":2, "end_line":3}], "blocked_question":"alpha beta gamma delta", "needed_evidence":"alpha beta gamma delta"}}}, "parsed_sha256":"p"})()
        self.assertIn("loss_question_mismatch", _mechanical(packet, arm, opportunity, ModelResult(b"x"), decoded, [], "u", "b")["failure_codes"])

    def test_y13_batch_id_binds_output_hashes(self):
        candidates = []
        for index in range(2):
            public = {"candidate_id":f"c{index}", "packet_sha256":f"p{index}", "output_artifact_sha256":f"o{index}", "binding_view_sha256":f"b{index}", "mechanical_score_sha256":f"m{index}"}
            candidates.append((public, {"candidate_id":f"c{index}", "hidden_arm_id":f"h{index}"}))
        before = _batch(candidates, "t", "seed")[0]["batch_id"]
        candidates[0][0]["output_artifact_sha256"] = "changed"
        self.assertNotEqual(before, _batch(candidates, "t", "seed")[0]["batch_id"])

    def test_y14_raw_tree_component_rejects_slash(self):
        child = "0" * 40
        body = b"100644 a/b\0" + bytes.fromhex(child)
        tree = "1" * 40
        repository = object.__new__(GitRepository)
        repository.object_format = "sha1"
        repository.oid_bytes = 20
        repository._cache = {tree:("tree", body), child:("blob", b"")}
        self.assert_typed(lambda: repository.tree(tree), PreflightError)

    def test_all_previously_silent_freeze_inputs_are_rejected(self):
        cases = (
            'from builtins import eval as e\ne("1")\n',
            'import builtins\ngetattr(builtins,"eval")("1")\n',
            '__builtins__["eval"]("1")\n',
            'import subprocess\nsubprocess.run(["x"])\n',
            'import os\nos.system("x")\n',
        )
        for index, source in enumerate(cases):
            with self.subTest(index=index), tempfile.TemporaryDirectory() as value:
                root = Path(value)
                (root / "hostile.py").write_text(source, encoding="utf-8", newline="\n")
                self.assert_typed(lambda root=root: file_records(root), ValueError)

    def test_transport_flags_are_exact_booleans(self):
        for field, value in (("timeout", "false"), ("client_truncation", 1), ("provider_truncation", [])):
            result = ModelResult(b"x")
            object.__setattr__(result, field, value)
            self.assert_typed(lambda result=result: _result(result), PipelineError)

    def test_authenticated_obligation_fields_are_closed_and_typed(self):
        def rejected(mutator):
            value = _valid_obligation()
            mutator(value)
            self.assert_typed(lambda: _obligation(value, "u"), PipelineError)
        cases = (
            lambda value: value["required_references"].append({"reference_id":1,"snapshot_side":"head","path":"src/lib.rs","blob_oid":"a"*40}),
            lambda value: value["projection"].__setitem__("projection_id", 1),
            lambda value: value["sources"][0].__setitem__("required_id", ""),
            lambda value: value["subject_windows"][0].__setitem__("role", "observer"),
            lambda value: value["projection"].__setitem__("canonical_sha256", "sha256:"+"0"*64),
            lambda value: value["required_references"].append({"reference_id":"","snapshot_side":"head","path":"src/lib.rs","blob_oid":"a"*40}),
            lambda value: (value["sources"][0].__setitem__("required_id", 1), value["projection"].__setitem__("source_required_ids", [1])),
        )
        for index, mutator in enumerate(cases):
            with self.subTest(index=index):
                rejected(mutator)
