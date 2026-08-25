import copy
import unittest

from evaluator.pipeline import PipelineError, _launch
from evaluator.stage0_contract import (
    CONTEXT_V2_HASH,
    CONTEXT_V3_HASH,
    Stage0ContractError,
    build_context_projection_v3,
    validate_context_projection_public,
    validate_occurrence_public,
    validate_occurrence_rebuild,
)
from .support import FIXTURE_ROOT, launch
from .vector_runner import _context_fixture, _occurrence_drafts, _occurrence_fixture


class AtomicStage0ContractTest(unittest.TestCase):
    def tearDown(self):
        if FIXTURE_ROOT.exists():
            import shutil
            shutil.rmtree(FIXTURE_ROOT)

    def test_v2_is_immutable_but_never_active(self):
        self.assertEqual(CONTEXT_V2_HASH, "sha256:7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26")
        selected,_=launch("v2-cross-decode")
        selected["context_policy_id"]="context.subject_windows@2"; selected["context_policy_sha256"]=CONTEXT_V2_HASH
        with self.assertRaisesRegex(PipelineError,"launch_context_policy_invalid"):
            _launch(selected)

    def test_occurrence_public_and_exact_rebuild_pass(self):
        drafts=_occurrence_drafts(); identity,files,_,public=_occurrence_fixture(drafts)
        self.assertEqual(validate_occurrence_public(public),public)
        self.assertEqual(validate_occurrence_rebuild(identity,files,drafts,["file:a","file:b"],public),public)

    def test_occurrence_hidden_row_and_count_tamper_fail(self):
        drafts=_occurrence_drafts(); identity,files,_,public=_occurrence_fixture(drafts)
        hidden=copy.deepcopy(public); hidden["ingestion_report_v2"]["observed_occurrence_count"]-=1
        with self.assertRaises(Stage0ContractError): validate_occurrence_rebuild(identity,files,drafts,["file:a","file:b"],hidden)
        _,_,_,truncated=_occurrence_fixture(drafts[:-1])
        with self.assertRaises(Stage0ContractError): validate_occurrence_rebuild(identity,files,drafts,["file:a","file:b"],truncated)

    def test_occurrence_bool_bucket_count_and_old_shape_fail(self):
        drafts=_occurrence_drafts(); identity,files,_,public=_occurrence_fixture(drafts)
        malformed=copy.deepcopy(public); malformed["ingestion_report_v2"]["source_occurrence_summaries"][0]["buckets"][0]["observed_occurrence_count"]=True
        with self.assertRaises(Stage0ContractError): validate_occurrence_public(malformed)
        old=copy.deepcopy(public); old["ingestion_report_v2"]["located_call_occurrences"]=[]
        with self.assertRaises(Stage0ContractError): validate_occurrence_rebuild(identity,files,drafts,["file:a","file:b"],old)

    def test_context_full_rebuild_and_public_hash_pass(self):
        projection=build_context_projection_v3(**_context_fixture())
        self.assertEqual(projection["policy_sha256"],CONTEXT_V3_HASH)
        self.assertEqual(validate_context_projection_public(projection),projection)

    def test_context_hash_and_commitment_tamper_fail(self):
        projection=build_context_projection_v3(**_context_fixture())
        changed=copy.deepcopy(projection); changed["accepted_file_denominator"]["observed_count"]+=1
        with self.assertRaisesRegex(Stage0ContractError,"context_projection_hash_invalid"):
            validate_context_projection_public(changed)
        changed=copy.deepcopy(projection); changed["canonical_sha256"]="sha256:"+"0"*64
        with self.assertRaisesRegex(Stage0ContractError,"context_projection_hash_invalid"):
            validate_context_projection_public(changed)

    def test_context_subject_order_partition_and_latent_are_closed(self):
        args=_context_fixture(); args["subject_outcomes"].reverse()
        with self.assertRaisesRegex(Stage0ContractError,"subject_outcome_order_invalid"):
            build_context_projection_v3(**args)
        args=_context_fixture(); args["support_loss_partitions"]=[]
        with self.assertRaisesRegex(Stage0ContractError,"support_anchor_partition_invalid"):
            build_context_projection_v3(**args)
        args=_context_fixture(); args["latent_cardinality"]={"state":"unknown","capability_states":{"direct_calls":"partial"},"qualification_ids":[]}
        with self.assertRaisesRegex(Stage0ContractError,"latent_cardinality_invalid"):
            build_context_projection_v3(**args)

    def test_context_materialized_4097_fails_before_rows_are_built(self):
        args=_context_fixture(); ids=[f"file:{index:04d}" for index in range(4097)]
        args["accepted_file_ids"]=ids; args["reached_file_ids"]=ids
        args["subject_outcomes"]=[{"role":"callee","status":"admitted","endpoint_id":"endpoint:callee","source_artifact_id":ids[0],"start_line":1,"end_line":1,"window_id":"window:0"},{"role":"caller","status":"admitted","endpoint_id":"endpoint:caller","source_artifact_id":ids[1],"start_line":1,"end_line":1,"window_id":"window:1"}]
        with self.assertRaisesRegex(Stage0ContractError,"materialized_source_overflow"):
            build_context_projection_v3(**args)


if __name__ == "__main__":
    unittest.main()
