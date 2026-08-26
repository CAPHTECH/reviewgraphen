"""Atomic model-stage entrance tests; no caller-authored score path is used."""
import os
import base64
import json
import hashlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from evaluator.canonical import canonical_bytes, hash_json, parse_json_bytes, sha256_bytes, stable_id
from evaluator.cli import COMMANDS, FixedProcessTransport, _parser
from evaluator.pipeline import PipelineError, _launch, _stage
from evaluator.stage0_driver import run_stage0
from evaluator.stage_driver import REVIEWER_OUTPUT_TOKENS, REVIEWER_TIMEOUT_SECONDS, _controls, _git_pinned_registration, _reduce, _stage0_root, seal_controls
from evaluator.tests import support
from evaluator.tests.support import fixture_file, launch
from evaluator.tests.test_stage0_driver import corpus, pipeline


class StageContractTest(unittest.TestCase):
    PRODUCT_SCRIPT=b'''#!/usr/bin/python3
import hashlib,json,pathlib,sys
def raw(value): return json.dumps(value,sort_keys=True,separators=(",",":"),ensure_ascii=False).encode()
def digest(value): return "sha256:"+hashlib.sha256(raw(value)).hexdigest()
def sid(prefix,value): return prefix+":"+digest(value)
args=sys.argv[1:]
if args[:2]==["schema","validate"]:
 value=json.loads(pathlib.Path(args[2]).read_bytes()); required={"schema","run_id","request_id","legacy_ingestion","ingestion_report_v2","obligation_contract","plan","contexts","observations","provider_free_packet_bindings","coverage","verifier","authority"}; ok=set(value)==required and value.get("schema")=="reviewgraphen.generic_review_run.v3"; sys.stdout.buffer.write(raw({"schema":value.get("schema"),"valid":True}) if ok else raw({"valid":False,"reason":"schema_invalid"})); raise SystemExit(0 if ok else 3)
if len(args)!=7 or args[0]!="review" or args[1]!="--request" or args[3]!="--artifacts" or args[5]!="--diagnostics": raise SystemExit(2)
request_path=pathlib.Path(args[2]); request=json.loads(request_path.read_bytes()); identity={key:request[key] for key in ("schema","repository_identity","base_revision","target_revision","ingest","plan","observer","verifier_descriptor_id","context_policy_id")}; request_id=sid("request",{"schema":request["schema"],"request_sha256":digest(identity)}); snapshot=sid("snapshot",identity); universe=sid("obligation-universe",identity); plan_id=sid("review-plan",{"request_id":request_id,"universe_id":universe}); run_id=sid("run",{"kind":"generic-review-v3","plan_id":plan_id})
run={"schema":"reviewgraphen.generic_review_run.v3","run_id":run_id,"request_id":request_id,"legacy_ingestion":{"repository_identity":request["repository_identity"],"program_space_id":snapshot,"snapshot_id":snapshot,"base_commit_oid":request["base_revision"],"base_tree_hash":"git:"+"0"*40,"target_commit_oid":request["target_revision"],"target_tree_hash":"git:"+"1"*40},"ingestion_report_v2":{},"obligation_contract":[],"plan":{"id":plan_id,"universe_id":universe,"waves":[],"deferred_obligation_ids":[]},"contexts":[],"observations":[],"provider_free_packet_bindings":[],"coverage":{},"verifier":None,"authority":{"classification":"non_authority","trusted_pass":False,"result_status":"incomplete","incomplete_reasons":["candidate_space_enumeration_incomplete","human_decision_not_recorded","model_observer_non_authority"]}}
run_bytes=raw(run); artifact_root=pathlib.Path(args[4]); artifact_root.mkdir(); (artifact_root/"audit.run.v3.json").write_bytes(run_bytes); manifest={"schema":"reviewgraphen.generic_review_artifact_manifest.v1","request_sha256":"sha256:"+hashlib.sha256(request_path.read_bytes()).hexdigest(),"request_id":request_id,"run_id":run_id,"snapshot_id":snapshot,"universe_id":universe,"artifacts":[{"path":"audit.run.v3.json","role":"audit","byte_length":len(run_bytes),"sha256":"sha256:"+hashlib.sha256(run_bytes).hexdigest()}]}; (artifact_root/"artifact-manifest.v1.json").write_bytes(raw(manifest)); sys.stdout.buffer.write(run_bytes)
'''

    @staticmethod
    def _stage0_verification():
        return {"schema":"m20.stage0-result.v1","experiment_id":"m20-changed-public-callee-utility-v1","cluster_count":300,"ingest_exclusion_counts":{"max_files":0,"snapshot_bytes":0,"blob_bytes":0},"model_calls":0,"gates":[{"id":identifier,"passed":True} for identifier in ("prevalence","subject_retention","bounded_context","determinism","enumeration_honesty","fan_out","deferred_fraction")]}

    @classmethod
    def _product_proof(cls, cluster, build_root):
        product=build_root.parents[3]/"m20-product-cli-fixture"; product.write_bytes(cls.PRODUCT_SCRIPT) if not product.exists() else None; product.chmod(0o500)
        request={"schema":"reviewgraphen.generic_review_request.v3","workspace_admission_root":".","repository_admission_root":".","repository_identity":cluster.repository_id,"base_revision":cluster.base_commit_oid,"target_revision":cluster.head_commit_oid,"ingest":{"profile_id":"rust.production.v1","profile_version":"1","rule_set_hash":"sha256:"+"0"*64,"max_files":20000,"max_file_bytes":4194304,"max_total_source_bytes":67108864},"plan":{"max_waves":1024,"max_obligations_per_wave":1024},"observer":{"kind":"deterministic_abstain"},"verifier_descriptor_id":"workspace.cargo_test@1","context_policy_id":"context.subject_windows@3"}
        request_raw=canonical_bytes(request); (build_root/"pipeline-request.v3.json").write_bytes(request_raw)
        completed=subprocess.run([str(product),"review","--request","pipeline-request.v3.json","--artifacts","pipeline-artifacts","--diagnostics","generic-review-diagnostics.v1.json"],cwd=build_root,env={"PATH":"","LC_ALL":"C"},stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=False,timeout=30); assert completed.returncode==0 and not completed.stderr
        run_raw=completed.stdout; run=parse_json_bytes(run_raw); (build_root/"product-run.v1.json").write_bytes(run_raw); manifest_raw=(build_root/"pipeline-artifacts/artifact-manifest.v1.json").read_bytes(); request_id=run["request_id"]; run_id=run["run_id"]; executable_sha=sha256_bytes(product.read_bytes()); invocation={"product_executable_sha256":executable_sha,"argv":["review","--request","pipeline-request.v3.json","--artifacts","pipeline-artifacts","--diagnostics","generic-review-diagnostics.v1.json"],"cwd_scope":"isolated-repository-copy","request_sha256":sha256_bytes(request_raw)}
        execution={"schema":"m20.stage0-product-execution.v2","product_executable_path":str(product),"product_executable_sha256":executable_sha,"invocation_sha256":hash_json(invocation),"request_sha256":sha256_bytes(request_raw),"run_sha256":sha256_bytes(run_raw),"artifact_manifest_sha256":sha256_bytes(manifest_raw),"request_id":request_id,"run_id":run_id}
        (build_root/"product-execution.v1.json").write_bytes(canonical_bytes(execution))

    @classmethod
    def _contract(cls, root, registration=None):
        corpus=parse_json_bytes((root/"corpus-manifest.v1.json").read_bytes())
        rows=[{"commit_cluster_id":row["commit_cluster_id"],"repository_id":row["repository_id"],"base_commit_oid":row["base_commit_oid"],"head_commit_oid":row["head_commit_oid"]} for row in corpus["clusters"]]
        repositories=[]
        for repository_id in sorted({row["repository_id"] for row in rows},key=str.encode):
            repositories.append({"url":"https://"+repository_id,"repository_id":repository_id,"pinned_commit":next(row["head_commit_oid"] for row in rows if row["repository_id"]==repository_id)})
        first=next((root/"clusters").iterdir())/"build-1/product-execution.v1.json"; execution=parse_json_bytes(first.read_bytes())
        registration=registration if registration is not None else parse_json_bytes((Path(__file__).parents[2]/"preregistration.json").read_bytes()); freeze_sha=registration["arm_neutral_contracts"]["freeze_hashes"]["freeze_manifest_sha256"]
        private=Ed25519PrivateKey.generate(); public=private.public_key().public_bytes(serialization.Encoding.Raw,serialization.PublicFormat.Raw); anchor_path=root.parent/(root.name+"-custodian-anchor.v2.json"); contract={"schema":"m20.corpus-identity.v1","repositories":repositories,"cluster_count":300,"cluster_manifest_sha256":hash_json({"schema":"m20.corpus-cluster-manifest.v1","clusters":rows}),"product_cli_path":execution["product_executable_path"],"product_cli_sha256":execution["product_executable_sha256"],"replay_verification":{"schema":"m20.stage0-product-replay-sample.v1","seed":"m20-stage0-product-replay-v1","sampled_build_count":12,"timeout_seconds":1800},"custodian_anchor_path":str(anchor_path.resolve()),"custodian_ed25519_public_key_base64":base64.b64encode(public).decode("ascii"),"custodian_ed25519_public_key_sha256":sha256_bytes(public)}; registration["corpus_identity"]=contract; preregistration_sha256=sha256_bytes(canonical_bytes(registration))
        manifest_sha=sha256_bytes((root/"artifact-manifest.v1.json").read_bytes()); body={"schema":"m20.custodian-stage0-anchor.v2","experiment_id":"m20-changed-public-callee-utility-v1","stage":"stage0","stage0_root":str(root),"stage0_artifact_manifest_sha256":manifest_sha,"signed_at_utc":"2026-08-26T00:00:00Z","preregistration_freeze_sha256":freeze_sha,"preregistration_sha256":preregistration_sha256}; anchor={**body,"custodian_ed25519_signature_base64":base64.b64encode(private.sign(canonical_bytes(body))).decode("ascii")}; anchor_path.write_bytes(canonical_bytes(anchor)); anchor_path.chmod(0o600)
        return contract

    def _registration_patch(self, root):
        registration=parse_json_bytes((Path(__file__).parents[2]/"preregistration.json").read_bytes()); self._contract(root,registration)
        return patch("evaluator.stage_driver._registration",return_value=(registration,sha256_bytes(canonical_bytes(registration))))

    @staticmethod
    def _resign_stage0(root):
        manifest=root/"artifact-manifest.v1.json"; manifest.unlink()
        rows=[{"path":path.relative_to(root).as_posix(),"sha256":sha256_bytes(path.read_bytes())} for path in sorted(root.rglob("*")) if path.is_file() and not path.is_symlink()]
        rows.append({"path":"artifact-manifest.v1.json","sha256":"self-described-by-manifest-bytes"}); manifest.write_bytes(canonical_bytes({"schema":"m20.stage0-artifact-manifest.v1","files":rows}))

    def _stage0(self, parent: Path):
        root = parent / "stage0"
        launch("stage0-corpus-repository")
        repositories = []
        for repository in range(3):
            repository_root=parent/f"repository-{repository}"; shutil.copytree(support.FIXTURE_ROOT/"repository",repository_root)
            repositories.append({"repository_id":f"synthetic.invalid/repository-{repository}", "repository_root":str(repository_root), "commits":[{"base_commit_oid":hashlib.sha1(f"b-{repository}-{index}".encode()).hexdigest(), "head_commit_oid":hashlib.sha1(f"h-{repository}-{index}".encode()).hexdigest()} for index in range(100)]})
        def admitted(cluster,build_root):
            self._product_proof(cluster,build_root); return pipeline(cluster,build_root)
        result = run_stage0(root, repositories, admitted, 300, jobs=1)
        return root, result["selection"].value

    @staticmethod
    def _labeler(path: Path, selection: dict, identity: str, label: str | set[str] = "not_control"):
        labels = [{"commit_cluster_id":unit_id,"label":("clean_refactor_control" if isinstance(label,set) and unit_id in label else "not_control" if isinstance(label,set) else label),"source_citations":[{"path":"src/synthetic.rs","start_line":1,"end_line":1}]} for unit_id in selection["eligible_cluster_ids"]]
        path.write_bytes(canonical_bytes({"schema":"m20.control_labeler_record.v1","experiment_id":"m20-changed-public-callee-utility-v1","selection_sha256":selection["selection_sha256"],"labeler_identity":identity,"did_not_implement_slice":True,"labels":labels}))

    def test_selection_hash_tamper_and_controls_absence_are_typed(self):
        with tempfile.TemporaryDirectory() as value:
            parent=Path(value); root,selection=self._stage0(parent); selection_path=root/"stage0-selection.v1.json"
            with self.assertRaisesRegex(PipelineError,"stage0_custodian_anchor_missing"): _stage0_root(selection_path.resolve())
            registration=parse_json_bytes((Path(__file__).parents[2]/"preregistration.json").read_bytes()); registration["corpus_identity"]=self._contract(root); anchor=Path(registration["corpus_identity"]["custodian_anchor_path"]); anchor.unlink()
            with patch("evaluator.stage_driver._registration",return_value=(registration,sha256_bytes(canonical_bytes(registration)))), self.assertRaisesRegex(PipelineError,"stage0_custodian_anchor_missing"): _stage0_root(selection_path.resolve())
            registration["corpus_identity"]=self._contract(root); anchor=Path(registration["corpus_identity"]["custodian_anchor_path"]); anchor_value=parse_json_bytes(anchor.read_bytes()); anchor_value["stage0_artifact_manifest_sha256"]="sha256:"+"0"*64; anchor.write_bytes(canonical_bytes(anchor_value))
            with patch("evaluator.stage_driver._registration",return_value=(registration,sha256_bytes(canonical_bytes(registration)))), self.assertRaisesRegex(PipelineError,"stage0_custodian_signature_invalid"): _stage0_root(selection_path.resolve())
            registration["corpus_identity"]=self._contract(root); anchor=Path(registration["corpus_identity"]["custodian_anchor_path"]); anchor_value=parse_json_bytes(anchor.read_bytes()); del anchor_value["custodian_ed25519_signature_base64"]; anchor.write_bytes(canonical_bytes(anchor_value))
            with patch("evaluator.stage_driver._registration",return_value=(registration,sha256_bytes(canonical_bytes(registration)))), self.assertRaisesRegex(PipelineError,"stage0_custodian_signature_missing"): _stage0_root(selection_path.resolve())
            registration["corpus_identity"]=self._contract(root); anchor=Path(registration["corpus_identity"]["custodian_anchor_path"]); anchor_value=parse_json_bytes(anchor.read_bytes()); body={key:item for key,item in anchor_value.items() if key!="custodian_ed25519_signature_base64"}; anchor_value["custodian_ed25519_signature_base64"]=base64.b64encode(Ed25519PrivateKey.generate().sign(canonical_bytes(body))).decode("ascii"); anchor.write_bytes(canonical_bytes(anchor_value))
            with patch("evaluator.stage_driver._registration",return_value=(registration,sha256_bytes(canonical_bytes(registration)))), self.assertRaisesRegex(PipelineError,"stage0_custodian_signature_invalid"): _stage0_root(selection_path.resolve())
            registration["corpus_identity"]=self._contract(root); registration["corpus_identity"]["custodian_ed25519_public_key_sha256"]="sha256:"+"0"*64
            with patch("evaluator.stage_driver._registration",return_value=(registration,sha256_bytes(canonical_bytes(registration)))), self.assertRaisesRegex(PipelineError,"stage0_custodian_public_key_mismatch"): _stage0_root(selection_path.resolve())
            fixture_contract=self._contract(root); production=parse_json_bytes((Path(__file__).parents[2]/"preregistration.json").read_bytes()); production["corpus_identity"]={**fixture_contract,"custodian_ed25519_public_key_base64":None,"custodian_ed25519_public_key_sha256":None}
            with patch("evaluator.stage_driver._registration",return_value=(production,sha256_bytes(canonical_bytes(production)))), self.assertRaisesRegex(PipelineError,"stage0_custodian_public_key_unsealed"): _stage0_root(selection_path.resolve())
            outside=parent/"outside-sample"; shutil.copytree(root,outside); registration["corpus_identity"]=self._contract(outside); corpus_value=parse_json_bytes((outside/"corpus-manifest.v1.json").read_bytes()); seed="m20-stage0-product-replay-v1"; keys=[(row["commit_cluster_id"],number) for row in corpus_value["clusters"] for number in (1,2)]; sampled=set(sorted(keys,key=lambda item:hashlib.sha256((seed+"\0"+item[0]+"\0"+str(item[1])).encode()).digest())[:12]); unit_id,number=next(item for item in reversed(keys) if item not in sampled); run_path=outside/"clusters"/unit_id.rsplit(":",1)[-1]/f"build-{number}/product-run.v1.json"; run=parse_json_bytes(run_path.read_bytes()); run["authority"]["incomplete_reasons"].append("sample_outside_anchor_mutation"); run_path.write_bytes(canonical_bytes(run)); self._resign_stage0(outside)
            with patch("evaluator.stage_driver._registration",return_value=(registration,sha256_bytes(canonical_bytes(registration)))), self.assertRaisesRegex(PipelineError,"stage0_custodian_anchor_mismatch"): _stage0_root((outside/"stage0-selection.v1.json").resolve())
            left=parent/"left.json"; right=parent/"right.json"; self._labeler(left,selection,"labeler-one"); self._labeler(right,selection,"labeler-two")
            controls_root=parent/"controls"
            with self._registration_patch(root): seal_controls(selection_path,left,right,controls_root)
            with self._registration_patch(root): self.assertEqual(_controls((controls_root/"control-labels.v1.json").resolve(),selection)[1]["selection_sha256"],selection["selection_sha256"])
            forged_root=parent/"forged"; shutil.copytree(root,forged_root); build=next((forged_root/"clusters").iterdir())/"build-1"; run_path=build/"product-run.v1.json"; run=parse_json_bytes(run_path.read_bytes()); run["schema"]="forged.not-product"; forged_raw=canonical_bytes(run); run_path.write_bytes(forged_raw); audit=build/"pipeline-artifacts/audit.run.v3.json"; audit.write_bytes(forged_raw); manifest_path=build/"pipeline-artifacts/artifact-manifest.v1.json"; product_manifest=parse_json_bytes(manifest_path.read_bytes()); product_manifest["artifacts"][0].update(byte_length=len(forged_raw),sha256=sha256_bytes(forged_raw)); manifest_path.write_bytes(canonical_bytes(product_manifest)); execution_path=build/"product-execution.v1.json"; execution=parse_json_bytes(execution_path.read_bytes()); execution.update(run_sha256=sha256_bytes(forged_raw),artifact_manifest_sha256=sha256_bytes(manifest_path.read_bytes())); execution_path.write_bytes(canonical_bytes(execution)); twin=build.parent/"build-2"; shutil.copy2(run_path,twin/"product-run.v1.json"); shutil.copy2(audit,twin/"pipeline-artifacts/audit.run.v3.json"); shutil.copy2(manifest_path,twin/"pipeline-artifacts/artifact-manifest.v1.json"); shutil.copy2(execution_path,twin/"product-execution.v1.json"); self._resign_stage0(forged_root)
            with self._registration_patch(forged_root), self.assertRaisesRegex(PipelineError,"stage0_product_run_schema_invalid"): _stage0_root((forged_root/"stage0-selection.v1.json").resolve())
            hash_root=parent/"forged-hash"; shutil.copytree(root,hash_root); build=next((hash_root/"clusters").iterdir())/"build-1"; execution_path=build/"product-execution.v1.json"; execution=parse_json_bytes(execution_path.read_bytes()); execution["run_sha256"]="sha256:"+"0"*64; execution_path.write_bytes(canonical_bytes(execution)); shutil.copy2(execution_path,build.parent/"build-2/product-execution.v1.json"); self._resign_stage0(hash_root)
            with self._registration_patch(hash_root), self.assertRaisesRegex(PipelineError,"stage0_product_execution_invalid"): _stage0_root((hash_root/"stage0-selection.v1.json").resolve())
            replay_root=parent/"forged-provenance"; shutil.copytree(root,replay_root); corpus_value=parse_json_bytes((replay_root/"corpus-manifest.v1.json").read_bytes()); seed="m20-stage0-product-replay-v1"; keys=[(row["commit_cluster_id"],number) for row in corpus_value["clusters"] for number in (1,2)]; unit_id,number=sorted(keys,key=lambda item:hashlib.sha256((seed+"\0"+item[0]+"\0"+str(item[1])).encode()).digest())[0]; build=replay_root/"clusters"/unit_id.rsplit(":",1)[-1]/f"build-{number}"; run_path=build/"product-run.v1.json"; run=parse_json_bytes(run_path.read_bytes()); run["authority"]["incomplete_reasons"].append("verifier_unsupported"); forged_raw=canonical_bytes(run); run_path.write_bytes(forged_raw); audit=build/"pipeline-artifacts/audit.run.v3.json"; audit.write_bytes(forged_raw); manifest_path=build/"pipeline-artifacts/artifact-manifest.v1.json"; product_manifest=parse_json_bytes(manifest_path.read_bytes()); product_manifest["artifacts"][0].update(byte_length=len(forged_raw),sha256=sha256_bytes(forged_raw)); manifest_path.write_bytes(canonical_bytes(product_manifest)); execution_path=build/"product-execution.v1.json"; execution=parse_json_bytes(execution_path.read_bytes()); execution.update(run_sha256=sha256_bytes(forged_raw),artifact_manifest_sha256=sha256_bytes(manifest_path.read_bytes())); execution_path.write_bytes(canonical_bytes(execution)); self._resign_stage0(replay_root)
            with self._registration_patch(replay_root), self.assertRaisesRegex(PipelineError,"stage0_product_replay_mismatch"): _stage0_root((replay_root/"stage0-selection.v1.json").resolve())
            with self.assertRaisesRegex(PipelineError,"control_root_invalid"): _controls((parent/"missing/control-labels.v1.json").resolve(),selection)
            gate_root=parent/"gate"; shutil.copytree(root,gate_root); result=parse_json_bytes((gate_root/"stage0-result.v1.json").read_bytes()); result["gates"][0]["passed"]=False; (gate_root/"stage0-result.v1.json").write_bytes(canonical_bytes(result)); self._resign_stage0(gate_root)
            with self._registration_patch(gate_root), self.assertRaisesRegex(PipelineError,"stage0_result_invalid"): _stage0_root((gate_root/"stage0-selection.v1.json").resolve())
            deleted_root=parent/"deleted"; shutil.copytree(root,deleted_root); shutil.rmtree(next((deleted_root/"clusters").iterdir())); self._resign_stage0(deleted_root)
            with self._registration_patch(deleted_root), self.assertRaisesRegex(PipelineError,"stage0_build_layout_invalid"): _stage0_root((deleted_root/"stage0-selection.v1.json").resolve())
            foreign_root=parent/"foreign"; shutil.copytree(root,foreign_root); foreign_path=foreign_root/"stage0-selection.v1.json"; foreign=parse_json_bytes(foreign_path.read_bytes()); foreign["stage1_cluster_ids"][0]="commit-cluster:"+"f"*64; foreign["selection_sha256"]=hash_json({key:item for key,item in foreign.items() if key!="selection_sha256"}); foreign_path.write_bytes(canonical_bytes(foreign)); self._resign_stage0(foreign_root)
            with self._registration_patch(foreign_root), self.assertRaisesRegex(PipelineError,"selection_membership_invalid"): _stage0_root(foreign_path.resolve())
            mutated=dict(selection); mutated["stage1_cluster_ids"]=list(reversed(mutated["stage1_cluster_ids"])); selection_path.write_bytes(canonical_bytes(mutated))
            with self._registration_patch(root), self.assertRaisesRegex(PipelineError,"artifact_manifest_mismatch"): _stage0_root(selection_path.resolve())

    def test_forty_cluster_stage0_root_is_rejected_by_control_ingress(self):
        with tempfile.TemporaryDirectory() as value:
            parent=Path(value); root=parent/"stage0"; result=run_stage0(root,corpus(40),pipeline,40,jobs=1); selection=result["selection"].value
            left=parent/"left.json"; right=parent/"right.json"; self._labeler(left,selection,"labeler-one"); self._labeler(right,selection,"labeler-two")
            with self.assertRaisesRegex(PipelineError,"stage0_cluster_count_invalid"):
                seal_controls((root/"stage0-selection.v1.json").resolve(),left.resolve(),right.resolve(),parent/"controls")

    def test_advance_requires_every_registered_reducer_condition(self):
        labels=[{"commit_cluster_id":f"u{index}","resolution":"clean_refactor_control"} for index in range(10)]
        controls={"labels":labels}
        base=[]
        for index in range(10):
            base.append({"unit_id":f"u{index}","repository_root":f"/repo/{index%3}","A":0,"B":1,"run_seal_id":f"seal-{index}","backend_integrity":True,"leakage_integrity":True,"judge_complete":True,"judge_positive_A":False,"judge_positive_B":False})
        stage0=self._stage0_verification(); self.assertEqual(_reduce("stage1",base,controls,{"reviewer":20,"judge":10},"1.000000",stage0)["decision"],"advance")
        cases={"primary_rectangle":lambda rows:[rows[index].update(A=1,B=0) for index in range(2)],"sensitivity_tables":lambda rows:[row.update(repository_root="/repo/0") for row in rows],"backend_integrity":lambda rows:rows[0].update(backend_integrity=False),"leakage_integrity":lambda rows:rows[0].update(leakage_integrity=False),"judge_completeness":lambda rows:rows[0].update(judge_complete=False),"safety":lambda rows:[rows[index].update(judge_positive_B=True) for index in range(2)]}
        for gate,mutate in cases.items():
            with self.subTest(gate=gate):
                rows=[dict(row) for row in base]; mutate(rows); result=_reduce("stage1",rows,controls,{"reviewer":20,"judge":10},"1.000000",stage0); self.assertFalse(result["gates"][gate]); self.assertEqual(result["decision"],"stop")
        inadequate={"labels":[{**row,"resolution":"not_control"} for row in labels]}; result=_reduce("stage1",base,inadequate,{"reviewer":20,"judge":10},"1.000000",stage0); self.assertFalse(result["gates"]["control_adequacy"]); self.assertEqual(result["decision"],"stop")
        missing=_reduce("stage1",base[:-1],controls,{"reviewer":20,"judge":10},"1.000000",stage0); self.assertFalse(missing["gates"]["sensitivity_tables"]); self.assertEqual(missing["decision"],"stop")
        corrupted={**stage0,"gates":[dict(row) for row in stage0["gates"]]}; corrupted["gates"][0]["passed"]=False; result=_reduce("stage1",base,controls,{"reviewer":20,"judge":10},"1.000000",corrupted); self.assertFalse(result["gates"]["stage0"]); self.assertEqual(result["decision"],"stop")

    def test_launch_v1_and_rank_or_selection_mutation_refuse(self):
        selected,_=launch("stage-binding")
        with self.assertRaisesRegex(PipelineError,"launch_schema_invalid"): _launch({**selected,"schema":"m20.pipeline_launch.v1"})
        stage=parse_json_bytes(fixture_file(selected["stage_manifest_path"]).read_bytes())
        bad={**selected,"selection_membership":{"stage":"stage1","cumulative_rank":2}}
        with self.assertRaisesRegex(PipelineError,"stage_membership_mismatch"): _stage(stage,bad)
        bad={**selected,"selection_manifest_sha256":"sha256:"+"0"*64}
        with self.assertRaisesRegex(PipelineError,"stage_contract_invalid"): _stage(stage,bad)

    def test_fixed_backend_missing_fails_closed_and_budget_is_not_mutable(self):
        with tempfile.TemporaryDirectory() as value, patch.object(FixedProcessTransport,"REVIEWER",Path(value)/"absent-reviewer"), patch.object(FixedProcessTransport,"JUDGE",Path(value)/"absent-judge"):
            with self.assertRaisesRegex(PipelineError,"backend_adapter_unavailable"): FixedProcessTransport()
        with tempfile.TemporaryDirectory() as value:
            reviewer=Path(value)/"reviewer"; judge=Path(value)/"judge"; reviewer.write_bytes(b"fixture"); judge.write_bytes(b"fixture"); reviewer.chmod(0o500); judge.chmod(0o500)
            response=subprocess.CompletedProcess([],0,canonical_bytes({"schema":"m20.backend_identity_health.v1","listing":[],"health":{}}),b"")
            with patch.object(FixedProcessTransport,"REVIEWER",reviewer), patch.object(FixedProcessTransport,"JUDGE",judge), patch("evaluator.cli.subprocess.run",return_value=response), self.assertRaisesRegex(PipelineError,"backend_executable_identity_mismatch"):
                FixedProcessTransport()
        self.assertEqual((REVIEWER_OUTPUT_TOKENS,REVIEWER_TIMEOUT_SECONDS),(12_000,900))
        self.assertEqual(set(vars(_parser().parse_args(["stage1","/selection","/new","--controls","/controls"]))),{"command","stage0_selection","new_output_root","controls"})
        self.assertTrue(_parser().parse_args(["verify-stage","/stage","--reexecute-all"]).reexecute_all)

    def test_external_aggregate_resume_and_single_pair_types_are_absent(self):
        self.assertFalse(set(COMMANDS)&{"run","resume","append","stage2b","aggregate"})

    def test_custodian_signer_is_external_and_emits_verifiable_anchor(self):
        self.assertNotIn("sign",COMMANDS)
        with tempfile.TemporaryDirectory() as value:
            parent=Path(value); private=Ed25519PrivateKey.generate(); private_path=parent/"custodian.pem"; private_path.write_bytes(private.private_bytes(serialization.Encoding.PEM,serialization.PrivateFormat.PKCS8,serialization.NoEncryption())); private_path.chmod(0o600); stage0=parent/"stage0"; stage0.mkdir(); output=parent/"anchor.json"; freeze="sha256:"+"1"*64; manifest="sha256:"+"2"*64
            completed=subprocess.run([sys.executable,str(Path(__file__).parents[4]/"scripts/custodian_sign_anchor.py"),"--private-key",str(private_path),"--stage0-root",str(stage0),"--stage0-manifest-sha256",manifest,"--preregistration-freeze-sha256",freeze,"--preregistration-sha256","sha256:"+"3"*64,"--signed-at-utc","2026-08-26T00:00:00Z","--output",str(output)],stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=False)
            self.assertEqual((completed.returncode,completed.stderr),(0,b"")); self.assertEqual(output.stat().st_mode & 0o777,0o600); result=parse_json_bytes(completed.stdout); public=private.public_key().public_bytes(serialization.Encoding.Raw,serialization.PublicFormat.Raw); self.assertEqual((result["custodian_ed25519_public_key_base64"],result["custodian_ed25519_public_key_sha256"]),(base64.b64encode(public).decode("ascii"),sha256_bytes(public)))
            anchor=parse_json_bytes(output.read_bytes()); signature=base64.b64decode(anchor.pop("custodian_ed25519_signature_base64")); private.public_key().verify(signature,canonical_bytes(anchor))

    def test_public_key_replacement_and_coherent_resign_cannot_replace_git_pin(self):
        with tempfile.TemporaryDirectory() as value:
            root=Path(value); path=root/"benchmarks/m20/preregistration.json"; path.parent.mkdir(parents=True); original=parse_json_bytes((Path(__file__).parents[2]/"preregistration.json").read_bytes()); path.write_bytes(canonical_bytes(original)); subprocess.run(["/usr/bin/git","init",str(root)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,check=True); subprocess.run(["/usr/bin/git","-C",str(root),"add","benchmarks/m20/preregistration.json"],check=True); subprocess.run(["/usr/bin/git","-C",str(root),"-c","user.name=Fixture","-c","user.email=fixture@invalid","commit","-m","pin prereg"],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,check=True)
            replacement=Ed25519PrivateKey.generate(); public=replacement.public_key().public_bytes(serialization.Encoding.Raw,serialization.PublicFormat.Raw); modified=parse_json_bytes(canonical_bytes(original)); modified["corpus_identity"]["custodian_ed25519_public_key_base64"]=base64.b64encode(public).decode("ascii"); modified["corpus_identity"]["custodian_ed25519_public_key_sha256"]=sha256_bytes(public); modified_raw=canonical_bytes(modified); path.write_bytes(modified_raw); body={"schema":"m20.custodian-stage0-anchor.v2","experiment_id":"m20-changed-public-callee-utility-v1","stage":"stage0","stage0_root":"/tmp/forged-stage0","stage0_artifact_manifest_sha256":"sha256:"+"1"*64,"signed_at_utc":"2026-08-26T00:00:00Z","preregistration_freeze_sha256":modified["arm_neutral_contracts"]["freeze_hashes"]["freeze_manifest_sha256"],"preregistration_sha256":sha256_bytes(modified_raw)}; signature=replacement.sign(canonical_bytes(body)); replacement.public_key().verify(signature,canonical_bytes(body))
            with self.assertRaisesRegex(PipelineError,"preregistration_git_pin_mismatch"): _git_pinned_registration(path)

    def test_stage0_driver_to_public_stage1_and_stage2a_cli(self):
        """Real CLI/process boundaries with contract fixture executables, never transport stubs."""
        source_workspace=Path(os.environ.get("M20_TEST_SOURCE_WORKSPACE",Path(__file__).parents[4]))
        bwrap=Path("/home/rizumita/.local/share/mise/installs/codex/latest/codex-resources/bwrap")
        if not bwrap.is_file(): self.skipTest("fixed mount-namespace runner unavailable")
        with tempfile.TemporaryDirectory(prefix="m20-stage-cli-e2e-") as value:
            parent=Path(value); workspace=parent/"workspace"; benchmark=workspace/"benchmarks/m20-changed-public-callee-utility-v1"; benchmark.parent.mkdir(parents=True)
            shutil.copytree(Path(__file__).parents[1],benchmark/"evaluator")
            for name in ("EVALUATOR_SPEC.md","preregistration.json"): shutil.copy2(Path(__file__).parents[2]/name,benchmark/name)
            from evaluator.semantic_acceptance import ALGORITHM_SOURCE_SHA256,reference_body
            for relative in (*ALGORITHM_SOURCE_SHA256,reference_body()["measurement_path"]):
                target=workspace/relative; target.parent.mkdir(parents=True,exist_ok=True); shutil.copy2(source_workspace/relative,target)
            env={"PYTHONPATH":str(benchmark),"PATH":os.environ.get("PATH","")}
            freeze_path=benchmark/"freeze-manifest.json"
            frozen=subprocess.run([sys.executable,"-m","evaluator","freeze-manifest",str(freeze_path)],cwd=benchmark,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=False,timeout=180)
            self.assertEqual((frozen.returncode,frozen.stderr),(0,b""),frozen.stdout.decode("utf-8","replace"))
            manifest=parse_json_bytes(freeze_path.read_bytes()); registration=parse_json_bytes((benchmark/"preregistration.json").read_bytes()); active=registration["arm_neutral_contracts"]["freeze_hashes"]
            active["freeze_manifest_path"]="freeze-manifest.json"; active["freeze_manifest_sha256"]=sha256_bytes(freeze_path.read_bytes())
            for key in ("evaluator_bundle_sha256","evaluator_execution_sha256","generated_fixture_inventory_sha256","reference_vector_set_sha256","mutation_manifest_sha256","semantic_acceptance_reference_sha256","measurement_record_sha256"): active[key]=manifest[key]
            (benchmark/"preregistration.json").write_bytes(canonical_bytes(registration))

            selected,unused=launch("closed-stage-e2e",subject_bytes=32)
            fixture_root=support.FIXTURE_ROOT; repository=parent/"repository"; shutil.copytree(fixture_root/"repository",repository)
            template=parse_json_bytes(fixture_file(selected["frozen_obligation_path"]).read_bytes()); base=selected["base_commit_oid"]; head=selected["head_commit_oid"]
            head_tree=subprocess.run(["/usr/bin/git","rev-parse",f"{head}^{{tree}}"],cwd=repository,stdout=subprocess.PIPE,check=True).stdout.decode().strip()
            from evaluator.tests.support import _object
            repositories=[]
            for repository_index in range(3):
                commits=[]
                for index in range(100):
                    body=f"tree {head_tree}\nparent {base}\nauthor Synthetic <fixture@invalid> {repository_index*100+index+2} +0000\ncommitter Synthetic <fixture@invalid> {repository_index*100+index+2} +0000\n\nfixture {repository_index}-{index}\n".encode()
                    synthetic_head=_object(repository/".git","commit",body); commits.append({"base_commit_oid":base,"head_commit_oid":synthetic_head})
                repository_root=parent/f"repository-{repository_index}"; shutil.copytree(repository,repository_root)
                repositories.append({"repository_id":f"synthetic.invalid/corpus-{repository_index:02d}","repository_root":str(repository_root),"commits":commits})
            def cluster_pipeline(cluster,build_root):
                self._product_proof(cluster,build_root)
                obligation_id="obligation:"+cluster.commit_cluster_id.rsplit(":",1)[-1]
                projection={**template["projection"],"obligation_ids":[obligation_id]}; projection["canonical_sha256"]=sha256_bytes(canonical_bytes({key:value for key,value in projection.items() if key!="canonical_sha256"}))
                obligation={**template,"unit_id":cluster.commit_cluster_id,"obligation_ids":[obligation_id],"projection":projection}; raw=canonical_bytes(obligation); (build_root/"frozen-obligation.v1.json").write_bytes(raw)
                result={"schema":"m20.stage0-cluster-build.v1","commit_cluster_id":cluster.commit_cluster_id,"repository_root":cluster.repository_root,"base_commit_oid":cluster.base_commit_oid,"head_commit_oid":cluster.head_commit_oid,"applicable_obligation_ids":[obligation_id],"subject_retained_obligation_ids":[obligation_id],"deferred_obligation_ids":[],"subject_remainders":[],"selected_obligation_id":obligation_id,"frozen_obligation_path":"frozen-obligation.v1.json","frozen_obligation_sha256":sha256_bytes(raw),"admitted_source_bytes":48,"whole_changed_production_files_bytes":96,"ignored_symlink_count":0,"model_eligible":True,"enumeration_honest":True,"ingest_exclusion":None}
                from evaluator.stage0_driver import build_payload_hash
                result["deterministic_payload_sha256"]=build_payload_hash(result); return result
            stage0=parent/"stage0"; stage0_result=run_stage0(stage0,repositories,cluster_pipeline,300,jobs=1); selection=stage0_result["selection"].value
            cumulative=selection["stage2a_cumulative_cluster_ids"]
            task_by_unit={unit_id:stable_id("review-task",{"comparison_contract":"m20.changed-public-callee.paired@1","task_contract":"m20.callee-contract-review@1","unit_id":unit_id}) for unit_id in cumulative}
            predicate=None
            for modulus in range(3,33):
                for residue in range(modulus):
                    selected_by_input=[unit_id for unit_id in cumulative if int(sha256_bytes(task_by_unit[unit_id].encode())[7:],16)%modulus==residue]
                    if len(set(selected_by_input)&set(cumulative[:10]))==2 and 8<=len(selected_by_input)<=22: predicate=(modulus,residue,selected_by_input); break
                if predicate: break
            self.assertIsNotNone(predicate); modulus,residue,input_selected=predicate
            clean_controls=set(input_selected[:8]); self.assertEqual((len(clean_controls),len(clean_controls&set(cumulative[:10]))),(8,2))
            left=parent/"labeler-one.json"; right=parent/"labeler-two.json"; self._labeler(left,selection,"synthetic-labeler-one",clean_controls); self._labeler(right,selection,"synthetic-labeler-two",clean_controls)

            reviewer=parent/"contract-fixture-reviewer"; judge=parent/"contract-fixture-judge"
            reviewer.write_text("""#!/usr/bin/python3
import base64,hashlib,json,re,sys
modulus,residue=__MODULUS__,__RESIDUE__
listing=json.loads('[{"drafter":null,"id":"Qwen3.8-27B-MLX-4bit","mode":null,"owned_by":"omlx","target":null},{"drafter":null,"id":"Qwen3.8-27B-MLX-8bit","mode":null,"owned_by":"omlx","target":null},{"drafter":null,"id":"RadixArk--Qwen3.8-27B-DSpark","mode":null,"owned_by":"omlx","target":null},{"drafter":null,"id":"mlx-community--Qwen3.8-27B-8bit","mode":null,"owned_by":"omlx","target":null},{"drafter":null,"id":"z-lab--Qwen3.8-27B-DFlash2","mode":null,"owned_by":"omlx","target":null}]')
health=json.loads('{"confidence_threshold":null,"context_window":null,"drafter":null,"lookup_drafts":null,"max_draft":null,"max_output_tokens":null,"mode":null,"model":null,"reasoning_effort":null,"supports_reasoning_effort":null,"target":null}')
if sys.argv[1:]==['--gate']:
 sys.stdout.write(json.dumps({'schema':'m20.backend_identity_health.v1','listing':listing,'health':health},sort_keys=True,separators=(',',':'))); raise SystemExit(0)
if sys.argv[1:]!=['--max-output-tokens','12000','--timeout-seconds','900']: raise SystemExit(2)
r=json.load(sys.stdin); p=r['payload']; sources=p['source_inventory']['admitted_sources']; selected=int(hashlib.sha256(p['task_id'].encode()).hexdigest(),16)%modulus==residue
if len(sources)<=2 and not selected: raw=b'{'
else:
 s=next(x for x in sources if x['role']=='changed'); body=next(x['text'] for x in p['payloads'] if x['payload_id']==s['payload_id']); names=[x for x in re.findall(r'[A-Za-z_][A-Za-z0-9_]*',body) if x not in {'pub','fn','let','mut','return'}]; symbol=names[0] if names else s['path'].rsplit('/',1)[-1].split('.')[0]; literal=(re.findall(r'\b[0-9]+\b',body) or ['none'])[-1]; alternate='counterfactual' in p['instruction']; o={'source_id':s['source_id'],'start_line':s['range']['start_line'],'end_line':s['range']['end_line']}; d={'kind':'claim','claims':[{'conclusion':'issue_absent' if alternate else 'issue_present','summary':symbol+' body deterministically exposes literal '+literal+' to its caller','observations':[o],'mechanism':{'trigger':'A direct caller reaches '+symbol+' through the admitted changed body','observed_behavior':symbol+' returns the body-derived literal '+literal,'consequence':symbol+' changes the caller-visible result from the admitted hunk'+(' under counterfactual review' if alternate else ' under ordinary review')}}],'abstention':None}; raw=json.dumps({'schema':'arm-neutral.source-grounded-disposition@1','task_id':p['task_id'],'source_inventory_id':p['source_inventory']['source_inventory_id'],'disposition':d},sort_keys=True,separators=(',',':')).encode()
usage={'input_tokens':len(json.dumps(r,sort_keys=True,separators=(',',':'))),'output_tokens':len(raw),'cache_tokens':0}; out={'schema':'m20.fixed-backend-response.v1','request_seal_sha256':r['request_seal_sha256'],'effective_max_output_tokens':r['max_output_tokens'],'effective_timeout_seconds':r['timeout_seconds'],'finish_reason':'stop','usage':usage,'raw_response_base64':base64.b64encode(raw).decode()}; sys.stdout.write(json.dumps(out,sort_keys=True,separators=(',',':')))
""".replace("__MODULUS__",str(modulus)).replace("__RESIDUE__",str(residue)),encoding="utf-8",newline="\n")
            judge.write_text("""#!/usr/bin/python3
import base64,json,sys
if sys.argv[1:]!=['--timeout-seconds','90']: raise SystemExit(2)
r=json.load(sys.stdin); b=r['payload']; scores=[]
for c in b['candidates']:
 state=c['mechanical_state']; forced=state['kind']=='mechanical_forced_zero'; packet=c['packet']; admitted={(x['source_id'],x['range']['start_line'],x['range']['end_line']) for x in packet['source_inventory']['admitted_sources']}; admitted_ids={x[0] for x in admitted}; parsed=state.get('parsed_output') or {}; disposition=parsed.get('disposition',{}); claims=disposition.get('claims',[]); observations=[o for claim in claims for o in claim.get('observations',[])]; exact=bool(observations) and all((o.get('source_id'),o.get('start_line'),o.get('end_line')) in admitted for o in observations); cited=bool(observations) and all(o.get('source_id') in admitted_ids for o in observations); source_score=2 if exact else 1 if cited else 0; task_exact=parsed.get('task_id')==packet.get('task_id') and parsed.get('source_inventory_id')==packet.get('source_inventory',{}).get('source_inventory_id') and c.get('binding_view',{}).get('task_id')==packet.get('task_id') and bool(c.get('binding_view',{}).get('obligation_ids')); task_score=2 if task_exact and exact else 1 if task_exact else 0; text=' '.join(x.get('text','') for x in packet.get('payloads',[])); names=[x for x in __import__('re').findall(r'[A-Za-z_][A-Za-z0-9_]*',text) if x not in {'pub','fn','let','mut','return'}]; symbol=names[0] if names else ''; mechanisms=[claim.get('mechanism',{}) for claim in claims]; fields=[str(m.get(name,'')) for m in mechanisms for name in ('trigger','observed_behavior','consequence')]; mechanism_score=2 if fields and all(fields) and symbol and sum(symbol in value for value in fields)>=2 else 1 if fields and all(fields) else 0; summaries=' '.join(str(claim.get('summary','')) for claim in claims); actionable=exact and bool(summaries) and any(token in summaries for token in __import__('re').findall(r'\b[0-9]+\b',text)); action_score=2 if actionable and mechanism_score==2 else 1 if exact else 0; dimensions={'source_specificity':source_score,'hidden_task_relevance':task_score,'mechanism_or_blocker_specificity':mechanism_score,'audit_actionability':action_score}; dimensions={name:(0 if forced else value) for name,value in dimensions.items()}; total=sum(dimensions.values()); scores.append({'candidate_id':c['candidate_id'],'packet_sha256':c['packet_sha256'],'output_artifact_sha256':c['output_artifact_sha256'],'binding_view_sha256':c['binding_view_sha256'],'mechanical_score_sha256':c['mechanical_score_sha256'],'score_source':'mechanical_forced_zero' if forced else 'judge','dimensions':dimensions,'total':total,'verdict':'usable' if total>=6 and min(dimensions.values())>=1 else 'not_usable'})
raw=json.dumps({'schema':'m20.utility_judge_batch_output.v1','batch_id':b['batch_id'],'scores':scores},sort_keys=True,separators=(',',':')).encode(); usage={'input_tokens':len(json.dumps(r,sort_keys=True,separators=(',',':'))),'output_tokens':len(raw),'cache_tokens':0}; out={'schema':'m20.fixed-backend-response.v1','request_seal_sha256':r['request_seal_sha256'],'effective_max_output_tokens':r['max_output_tokens'],'effective_timeout_seconds':r['timeout_seconds'],'finish_reason':'stop','usage':usage,'raw_response_base64':base64.b64encode(raw).decode()}; sys.stdout.write(json.dumps(out,sort_keys=True,separators=(',',':')))
""",encoding="utf-8",newline="\n")
            reviewer.chmod(0o500); judge.chmod(0o500)
            from evaluator.pipeline import _backend_request
            source={"source_id":"source:probe","role":"changed","path":"src/probe.rs","range":{"start_line":1,"end_line":1},"payload_id":"payload:probe"}; context=[{"source_id":f"source:context-{index}","role":"context","path":f"src/context_{index}.rs","range":{"start_line":1,"end_line":1},"payload_id":f"payload:context-{index}"} for index in (1,2)]; probe={"task_id":"probe-task","instruction":"ordinary review","source_inventory":{"source_inventory_id":"probe-inventory","admitted_sources":[source,*context]},"payloads":[{"payload_id":"payload:probe","text":"pub fn probe_callee() -> i32 { 7 }"}]}; first_request=_backend_request("reviewer",probe,None,12_000,900); alternate=parse_json_bytes(canonical_bytes(probe)); alternate["instruction"]="counterfactual review"; second_request=_backend_request("reviewer",alternate,None,12_000,900); responses=[]
            for request in (first_request,second_request): responses.append(parse_json_bytes(subprocess.run([str(reviewer),"--max-output-tokens","12000","--timeout-seconds","900"],input=canonical_bytes(request),stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=True).stdout))
            self.assertNotEqual(first_request["request_seal_sha256"],second_request["request_seal_sha256"]); self.assertNotEqual(responses[0]["raw_response_base64"],responses[1]["raw_response_base64"])
            gate=parse_json_bytes(subprocess.run([str(reviewer),"--gate"],stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=True).stdout)
            registration=parse_json_bytes((benchmark/"preregistration.json").read_bytes()); registration["backend_gate"]["reviewer_transport_sha256"]=sha256_bytes(reviewer.read_bytes()); registration["backend_gate"]["judge_transport_sha256"]=sha256_bytes(judge.read_bytes()); registration["backend_gate"]["pinned_listing_sha256"]=sha256_bytes(canonical_bytes(gate["listing"]))[7:]; registration["backend_gate"]["pinned_health_sha256"]=sha256_bytes(canonical_bytes(gate["health"]))[7:]
            secret_root=parent/"secrets"; secret_root.mkdir(); response_key=secret_root/"m20-evaluator-response-hmac-key"; response_key.write_bytes(os.urandom(32)); response_key.chmod(0o600); registration["backend_gate"]["response_hmac_key_sha256"]=sha256_bytes(response_key.read_bytes()); self._contract(stage0,registration); (benchmark/"preregistration.json").write_bytes(canonical_bytes(registration))
            subprocess.run(["/usr/bin/git","init",str(workspace)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,check=True); subprocess.run(["/usr/bin/git","-C",str(workspace),"add","benchmarks/m20-changed-public-callee-utility-v1/preregistration.json"],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,check=True); subprocess.run(["/usr/bin/git","-C",str(workspace),"-c","user.name=Fixture","-c","user.email=fixture@invalid","commit","-m","pin test preregistration"],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,check=True)
            fixed_bin=parent/"fixed-bin"; shutil.copytree("/usr/local/bin",fixed_bin,symlinks=True); shutil.copy2(reviewer,fixed_bin/"m20-reviewer-backend"); shutil.copy2(judge,fixed_bin/"m20-judge-backend"); (fixed_bin/"m20-reviewer-backend").chmod(0o500); (fixed_bin/"m20-judge-backend").chmod(0o500)
            def cli(arguments):
                command=[str(bwrap),"--die-with-parent","--ro-bind","/","/","--dev","/dev","--bind","/tmp","/tmp","--tmpfs","/run","--ro-bind",str(secret_root),"/run/secrets","--ro-bind",str(fixed_bin),"/usr/local/bin","--chdir",str(benchmark),sys.executable,"-m","evaluator",*map(str,arguments)]
                return subprocess.run(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=False,timeout=180,env=env)
            controls=parent/"controls"; sealed=cli(["seal-controls",stage0/"stage0-selection.v1.json",left,right,controls]); self.assertEqual((sealed.returncode,sealed.stderr),(0,b""))
            stage1_root=parent/"stage1"; first=cli(["stage1",stage0/"stage0-selection.v1.json",stage1_root,"--controls",controls/"control-labels.v1.json"]); self.assertEqual((first.returncode,first.stderr),(0,b""),first.stdout.decode("utf-8","replace")); self.assertEqual(parse_json_bytes(first.stdout)["decision"],"advance",first.stdout.decode("utf-8","replace"))
            batch_path=next((stage1_root/"units").iterdir())/"judge/batch.json"; original_batch=parse_json_bytes(batch_path.read_bytes()); changed_batch=parse_json_bytes(canonical_bytes(original_batch)); claim=next(candidate["mechanical_state"]["parsed_output"]["disposition"]["claims"][0] for candidate in changed_batch["candidates"] if candidate["mechanical_state"]["kind"]=="judgeable"); claim["observations"][0]["end_line"]+=777; claim["mechanism"]={"trigger":"generic","observed_behavior":"generic","consequence":"generic"}
            def judge_scores(batch):
                request=_backend_request("judge",batch,"fixture content-derived rubric",12_000,90); envelope=parse_json_bytes(subprocess.run([str(judge),"--timeout-seconds","90"],input=canonical_bytes(request),stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=True).stdout); return parse_json_bytes(base64.b64decode(envelope["raw_response_base64"]))["scores"]
            self.assertNotEqual(judge_scores(original_batch),judge_scores(changed_batch))
            stage2_root=parent/"stage2a"; second=cli(["stage2a",stage1_root,stage2_root]); self.assertEqual((second.returncode,second.stderr),(0,b""),second.stdout.decode("utf-8","replace")); self.assertEqual((parse_json_bytes(second.stdout)["decision"],parse_json_bytes(second.stdout)["n"]),("success",40))
            verified=cli(["verify-stage",stage2_root,"--reexecute-all"]); self.assertEqual(verified.returncode,0,verified.stdout.decode("utf-8","replace"))
            if fixture_root.exists(): shutil.rmtree(fixture_root)


if __name__ == "__main__":
    unittest.main()
