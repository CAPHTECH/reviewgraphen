import base64
import os
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from evaluator import pipeline
from evaluator.artifacts import verify_run
from evaluator.canonical import canonical_bytes, parse_json_bytes, sha256_bytes
from evaluator.pipeline import PipelineError, _packet_audit, _packet_v2, _union_specs, packet_v3_admitted_bytes, packet_v3_plan
from .support import FIXTURE_ROOT, Transport, fixture_hmac_key, fixture_run, launch

class PipelineTest(unittest.TestCase):
    def tearDown(self):
        if FIXTURE_ROOT.exists(): shutil.rmtree(FIXTURE_ROOT)
    def test_end_to_end_seals_two_primary_cells(self):
        selected,root=launch("pipeline"); old=pipeline._FIXTURE_EXECUTION_IDENTITY; calls=pipeline._PACKET_V3_ACCOUNT_CALLS; pipeline._FIXTURE_EXECUTION_IDENTITY="sha256:"+"d"*64
        try: result=fixture_run(selected,Transport(),root)
        finally: pipeline._FIXTURE_EXECUTION_IDENTITY=old
        self.assertEqual(pipeline._PACKET_V3_ACCOUNT_CALLS,calls+1); self.assertEqual(len(result["primary"]["scores"]),2); self.assertTrue(all(x["completed"] for x in result["primary"]["scores"]));
        with fixture_hmac_key(): self.assertTrue(verify_run(root)["ok"])

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

    def test_partial_source_overlap_is_merged_before_byte_accounting(self):
        class Repository:
            def blob_for(self, _trees, _side, _path, _oid): return b"".join(f"line-{index:02d}\n".encode() for index in range(1,16))
        def spec(start,end,role): return {"required_id":f"r-{start}-{end}","role":role,"snapshot_side":"head","path":"src/lib.rs","start_line":start,"end_line":end,"blob_oid":"a"*40}
        merged=_union_specs([spec(1,10,"changed")],[spec(5,15,"context")],lambda _:120)
        self.assertEqual([(row["start_line"],row["end_line"]) for row in merged],[(1,15)])
        self.assertEqual(packet_v3_admitted_bytes(Repository(),({},{}),merged),len(Repository().blob_for(None,None,None,None)))
        at_bound=_union_specs([spec(1,399,"changed")],[spec(400,400,"context")],lambda _:1)
        over_bound=_union_specs([spec(1,400,"changed")],[spec(401,401,"context")],lambda _:1)
        self.assertEqual([(row["start_line"],row["end_line"]) for row in at_bound],[(1,400)])
        self.assertEqual([(row["start_line"],row["end_line"]) for row in over_bound],[(1,400),(401,401)])
        exact_bytes=_union_specs([spec(1,1,"changed")],[spec(2,2,"context")],lambda _:262_144)
        over_bytes=_union_specs([spec(1,1,"changed")],[spec(2,2,"context")],lambda _:262_145)
        self.assertEqual([(row["start_line"],row["end_line"]) for row in exact_bytes],[(1,2)])
        self.assertEqual([(row["start_line"],row["end_line"]) for row in over_bytes],[(1,1),(2,2)])
        class ByteRepository:
            def __init__(self,total): self.raw=b"a"*131_071+b"\n"+b"b"*(total-131_073)+b"\n"
            def baseline_specs(self,_trees): return [spec(1,1,"changed")]
            def blob_for(self,*_): return self.raw
        callee=spec(2,2,"changed")
        exact_core,_=packet_v3_plan(ByteRepository(262_144),({},{}),callee,[])
        over_core,_=packet_v3_plan(ByteRepository(262_145),({},{}),callee,[])
        self.assertEqual([(row["start_line"],row["end_line"]) for row in exact_core],[(1,2)])
        self.assertEqual([(row["start_line"],row["end_line"]) for row in over_core],[(1,1),(2,2)])

    def test_response_hmac_precedes_inner_parse_and_key_identity_is_external(self):
        response={"schema":"m20.fixed-backend-response.v1","request_seal_sha256":"sha256:"+"1"*64,"effective_max_output_tokens":12000,"effective_timeout_seconds":900,"finish_reason":"stop","usage":{"input_tokens":1,"output_tokens":2,"cache_tokens":0},"raw_response_base64":base64.b64encode(b"{}").decode("ascii")}
        events=[]; verify=pipeline._verify_backend_response_hmac_bytes; parse=pipeline._parse_backend_response_bytes
        with fixture_hmac_key():
            sealed=pipeline._sealed_backend_response(response,pipeline._seal_backend_response(canonical_bytes(response)))
            with patch.object(pipeline,"_verify_backend_response_hmac_bytes",side_effect=lambda raw,seal:(events.append("hmac"),verify(raw,seal))[1]), patch.object(pipeline,"_parse_backend_response_bytes",side_effect=lambda raw:(events.append("parse"),parse(raw))[1]):
                self.assertEqual(pipeline._open_backend_response(sealed),response)
        self.assertEqual(events,["hmac","parse"])
        with tempfile.TemporaryDirectory() as value:
            key_path=Path(value)/"custodian-key"; key_path.write_bytes(os.urandom(32)); key_path.chmod(0o600)
            with self.assertRaisesRegex(PipelineError,"backend_response_hmac_key_identity_mismatch"):
                pipeline._read_pinned_response_hmac_key(key_path,sha256_bytes(os.urandom(32)))
            key_path.write_bytes(b"m20-contract-fixture-response-hmac-key-v1")
            with self.assertRaisesRegex(PipelineError,"backend_response_hmac_key_identity_mismatch"):
                pipeline._read_pinned_response_hmac_key(key_path,sha256_bytes(key_path.read_bytes()))

    def test_fixture_judge_dimensions_change_with_claim_content(self):
        selected,root=launch("content-judge"); old=pipeline._FIXTURE_EXECUTION_IDENTITY; pipeline._FIXTURE_EXECUTION_IDENTITY="sha256:"+"d"*64
        try: fixture_run(selected,Transport(),root)
        finally: pipeline._FIXTURE_EXECUTION_IDENTITY=old
        batch=parse_json_bytes((root/"judge/batch.json").read_bytes()); changed=parse_json_bytes(canonical_bytes(batch)); claim=next(candidate["mechanical_state"]["parsed_output"]["disposition"]["claims"][0] for candidate in changed["candidates"] if candidate["mechanical_state"]["kind"]=="judgeable"); claim["observations"][0]["end_line"]+=777; claim["mechanism"]={"trigger":"generic","observed_behavior":"generic","consequence":"generic"}
        with fixture_hmac_key():
            original=parse_json_bytes(Transport().judge(canonical_bytes(batch),b"rubric",90).raw_bytes); mutated=parse_json_bytes(Transport().judge(canonical_bytes(changed),b"rubric",90).raw_bytes)
        self.assertNotEqual(original["scores"],mutated["scores"])
