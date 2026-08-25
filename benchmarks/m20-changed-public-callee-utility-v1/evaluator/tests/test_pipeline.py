import shutil
import unittest
from evaluator import pipeline
from evaluator.artifacts import verify_run
from evaluator.canonical import canonical_bytes, parse_json_bytes, sha256_bytes
from evaluator.pipeline import PipelineError, _packet_audit, _packet_v2
from .support import FIXTURE_ROOT, Transport, fixture_run, launch

class PipelineTest(unittest.TestCase):
    def tearDown(self):
        if FIXTURE_ROOT.exists(): shutil.rmtree(FIXTURE_ROOT)
    def test_end_to_end_seals_two_primary_cells(self):
        selected,root=launch("pipeline"); old=pipeline._FIXTURE_EXECUTION_IDENTITY; pipeline._FIXTURE_EXECUTION_IDENTITY="sha256:"+"d"*64
        try: result=fixture_run(selected,Transport(),root)
        finally: pipeline._FIXTURE_EXECUTION_IDENTITY=old
        self.assertEqual(len(result["primary"]["scores"]),2); self.assertTrue(all(x["completed"] for x in result["primary"]["scores"])); self.assertTrue(verify_run(root)["ok"])

    def test_shared_core_is_identical_and_only_b_adds_windows(self):
        selected,root=launch("shared-core",subject_bytes=32); old=pipeline._FIXTURE_EXECUTION_IDENTITY; pipeline._FIXTURE_EXECUTION_IDENTITY="sha256:"+"d"*64
        try: fixture_run(selected,Transport(),root)
        finally: pipeline._FIXTURE_EXECUTION_IDENTITY=old
        packets=[parse_json_bytes((root/f"slots/{slot}/packet.json").read_bytes()) for slot in range(2)]
        self.assertTrue(all(packet["schema"]=="arm-neutral.source-grounded-packet@3" for packet in packets))
        smaller,larger=sorted(packets,key=lambda packet:len(packet["source_inventory"]["admitted_sources"]))
        core={canonical_bytes(source) for source in smaller["source_inventory"]["admitted_sources"]}
        treatment={canonical_bytes(source) for source in larger["source_inventory"]["admitted_sources"]}
        self.assertTrue(core)
        self.assertLess(len(core),len(treatment))
        self.assertEqual(core & treatment,core)
        self.assertEqual({source["path"] for source in larger["source_inventory"]["admitted_sources"] if canonical_bytes(source) in treatment-core},{"src/subject.rs"})
        for field in ("task_id","instruction","response_schema"): self.assertEqual(smaller[field],larger[field])

    def test_packet_v2_bytes_are_stable_and_cross_decode_is_rejected(self):
        arm={"hidden_arm_id":"h","sources":[],"payloads":[],"task_losses":[],"routine_loss":{"loss_id":"l","reason":"routine_scope_omission","omitted_scope":"bounded_context","recovery_reference":"r","primary_abstention_eligible":False,"undecidable_question_id":None}}
        packet=_packet_v2(arm,"review-task:legacy")
        self.assertEqual(len(canonical_bytes(packet)),3375)
        self.assertEqual(sha256_bytes(canonical_bytes(packet)),"sha256:6e11066715318cdcca91dc2e8669d5631449853fab23b6ba477f0fa7011c087c")
        with self.assertRaisesRegex(PipelineError,"packet_constants_invalid"): _packet_audit(packet)
