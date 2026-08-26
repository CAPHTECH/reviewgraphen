import shutil
import unittest

from evaluator import pipeline
from evaluator.artifacts import verify_run
from evaluator.model_boundary import ModelResult
from evaluator.pipeline import _execution
from .support import FIXTURE_ROOT, Transport, fixture_hmac_key, fixture_run, launch


class CountingTransport(Transport):
    def __init__(self):
        super().__init__()
        self.calls = 0

    def review(self, request, slot, timeout):
        self.calls += 1
        return super().review(request, slot, timeout)

    def judge(self, request, instruction, timeout):
        self.calls += 1
        return super().judge(request, instruction, timeout)


class InputBudgetTest(unittest.TestCase):
    def tearDown(self):
        if FIXTURE_ROOT.exists():
            shutil.rmtree(FIXTURE_ROOT)

    def _run(self, size):
        selected, root = launch(f"budget-{size}", source_bytes=65_536, subject_bytes=None if size == 65_536 else 1)
        transport = CountingTransport()
        previous = pipeline._FIXTURE_EXECUTION_IDENTITY
        pipeline._FIXTURE_EXECUTION_IDENTITY = "sha256:" + "b" * 64
        try:
            result = fixture_run(selected, transport, root)
        finally:
            pipeline._FIXTURE_EXECUTION_IDENTITY = previous
        return result, root, transport

    def test_exact_ceiling_is_eligible_and_audited(self):
        result, root, transport = self._run(65_536)
        self.assertEqual(result["pipeline_terminal_state"], "sealed")
        self.assertEqual(transport.calls, 3)
        with fixture_hmac_key(): audit = verify_run(root)
        self.assertTrue(audit["ok"], audit)
        self.assertFalse(audit["token_observation_recomputable"])

    def test_plus_one_is_pair_wide_sealed_model_ineligible(self):
        result, root, transport = self._run(65_537)
        self.assertEqual(result, {
            "schema": "m20.pipeline_result.v1",
            "unit_id": "fixture-budget-65537",
            "pipeline_terminal_state": "model_ineligible",
            "model_ineligible_reason": "admitted_source_byte_ceiling_exceeded",
            "arm_results": {"A": None, "B": None},
            "model_call_count": 0,
            "run_seal_id": result["run_seal_id"],
        })
        self.assertEqual(transport.calls, 0)
        self.assertEqual({path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_file()}, {
            "launch.json", "repository.json", "obligation.json", "pair.json", "budget.json",
            "slots/0/packet.json", "slots/1/packet.json", "ledger.json", "seal.json",
        })
        with fixture_hmac_key(): self.assertTrue(verify_run(root)["ok"])

    def test_token_observation_is_closed_and_observation_only(self):
        absent = _execution(ModelResult(b"x"), None, "adapter")["token_observation"]
        reported = _execution(ModelResult(b"x", usage=(("input_tokens", 7), ("output_tokens", 3))), None, "adapter")["token_observation"]
        invalid = _execution(ModelResult(b"x", usage=(("provider_guess", 7),)), None, "adapter")["token_observation"]
        self.assertEqual(absent["unavailable_reason"], "backend_usage_absent")
        self.assertEqual(reported["source"], "backend_report")
        self.assertEqual(reported["input_tokens"], 7)
        self.assertEqual(invalid["unavailable_reason"], "backend_usage_invalid")
