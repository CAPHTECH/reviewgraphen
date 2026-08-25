"""Atomic model-stage entrance tests; no caller-authored score path is used."""
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from evaluator.canonical import canonical_bytes, parse_json_bytes, sha256_bytes
from evaluator.cli import COMMANDS, FixedProcessTransport, _parser
from evaluator.pipeline import PipelineError, _launch, _stage
from evaluator.stage0_driver import run_stage0
from evaluator.stage_driver import REVIEWER_OUTPUT_TOKENS, REVIEWER_TIMEOUT_SECONDS, _controls, _stage0_root, seal_controls
from evaluator.tests import support
from evaluator.tests.support import fixture_file, launch
from evaluator.tests.test_stage0_driver import corpus, pipeline


class StageContractTest(unittest.TestCase):
    def _stage0(self, parent: Path):
        root = parent / "stage0"
        result = run_stage0(root, corpus(40), pipeline, 40, jobs=1)
        return root, result["selection"].value

    @staticmethod
    def _labeler(path: Path, selection: dict, identity: str):
        labels = [{"commit_cluster_id":unit_id,"label":"not_control","source_citations":[{"path":"src/synthetic.rs","start_line":1,"end_line":1}]} for unit_id in selection["eligible_cluster_ids"]]
        path.write_bytes(canonical_bytes({"schema":"m20.control_labeler_record.v1","experiment_id":"m20-changed-public-callee-utility-v1","selection_sha256":selection["selection_sha256"],"labeler_identity":identity,"did_not_implement_slice":True,"labels":labels}))

    def test_selection_hash_tamper_and_controls_absence_are_typed(self):
        with tempfile.TemporaryDirectory() as value:
            parent=Path(value); root,selection=self._stage0(parent); selection_path=root/"stage0-selection.v1.json"
            left=parent/"left.json"; right=parent/"right.json"; self._labeler(left,selection,"labeler-one"); self._labeler(right,selection,"labeler-two")
            controls_root=parent/"controls"; seal_controls(selection_path,left,right,controls_root)
            self.assertEqual(_controls((controls_root/"control-labels.v1.json").resolve(),selection)[1]["selection_sha256"],selection["selection_sha256"])
            with self.assertRaisesRegex(PipelineError,"control_root_invalid"): _controls((parent/"missing/control-labels.v1.json").resolve(),selection)
            mutated=dict(selection); mutated["stage1_cluster_ids"]=list(reversed(mutated["stage1_cluster_ids"])); selection_path.write_bytes(canonical_bytes(mutated))
            with self.assertRaisesRegex(PipelineError,"artifact_manifest_mismatch"): _stage0_root(selection_path.resolve())

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
        self.assertEqual((REVIEWER_OUTPUT_TOKENS,REVIEWER_TIMEOUT_SECONDS),(12_000,900))
        self.assertEqual(set(vars(_parser().parse_args(["stage1","/selection","/new","--controls","/controls"]))),{"command","stage0_selection","new_output_root","controls"})

    def test_external_aggregate_resume_and_single_pair_types_are_absent(self):
        self.assertFalse(set(COMMANDS)&{"run","resume","append","stage2b","aggregate"})

    def test_stage0_driver_to_public_stage1_and_stage2a_cli(self):
        """Real CLI/process boundaries; no RUN/transport monkeypatch or caller aggregate."""
        source_workspace=Path(__file__).parents[4]
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
            self.assertEqual((frozen.returncode,frozen.stderr),(0,b""))
            manifest=parse_json_bytes(freeze_path.read_bytes()); registration=parse_json_bytes((benchmark/"preregistration.json").read_bytes()); active=registration["arm_neutral_contracts"]["freeze_hashes"]
            active["freeze_manifest_path"]="freeze-manifest.json"; active["freeze_manifest_sha256"]=sha256_bytes(freeze_path.read_bytes())
            for key in ("evaluator_bundle_sha256","evaluator_execution_sha256","generated_fixture_inventory_sha256","reference_vector_set_sha256","mutation_manifest_sha256","semantic_acceptance_reference_sha256","measurement_record_sha256"): active[key]=manifest[key]
            (benchmark/"preregistration.json").write_bytes(canonical_bytes(registration))

            selected,unused=launch("closed-stage-e2e",subject_bytes=32)
            fixture_root=support.FIXTURE_ROOT; repository=parent/"repository"; shutil.copytree(fixture_root/"repository",repository)
            template=parse_json_bytes(fixture_file(selected["frozen_obligation_path"]).read_bytes()); base=selected["base_commit_oid"]; head=selected["head_commit_oid"]
            repositories=[{"repository_id":f"synthetic.invalid/corpus-{index:02d}","repository_root":str(repository),"commits":[{"base_commit_oid":base,"head_commit_oid":head}]} for index in range(40)]
            def cluster_pipeline(cluster,build_root):
                obligation={**template,"unit_id":cluster.commit_cluster_id}; raw=canonical_bytes(obligation); (build_root/"frozen-obligation.v1.json").write_bytes(raw); obligation_id=obligation["obligation_ids"][0]
                result={"schema":"m20.stage0-cluster-build.v1","commit_cluster_id":cluster.commit_cluster_id,"repository_root":str(repository),"base_commit_oid":base,"head_commit_oid":head,"applicable_obligation_ids":[obligation_id],"subject_retained_obligation_ids":[obligation_id],"deferred_obligation_ids":[],"subject_remainders":[],"selected_obligation_id":obligation_id,"frozen_obligation_path":"frozen-obligation.v1.json","frozen_obligation_sha256":sha256_bytes(raw),"admitted_source_bytes":48,"whole_changed_production_files_bytes":96,"model_eligible":True,"enumeration_honest":True}
                from evaluator.stage0_driver import build_payload_hash
                result["deterministic_payload_sha256"]=build_payload_hash(result); return result
            stage0=parent/"stage0"; stage0_result=run_stage0(stage0,repositories,cluster_pipeline,40,jobs=1); selection=stage0_result["selection"].value
            left=parent/"labeler-one.json"; right=parent/"labeler-two.json"; self._labeler(left,selection,"synthetic-labeler-one"); self._labeler(right,selection,"synthetic-labeler-two")

            reviewer=parent/"reviewer"; judge=parent/"judge"
            reviewer.write_text("""#!/usr/bin/python3
import json,sys
p=json.load(sys.stdin); sources=p['source_inventory']['admitted_sources']
if len(sources)<=2: sys.stdout.write('{'); raise SystemExit(0)
s=next(x for x in sources if x['role']=='changed'); o={'source_id':s['source_id'],'start_line':s['range']['start_line'],'end_line':s['range']['end_line']}; d={'kind':'claim','claims':[{'conclusion':'issue_present','summary':'Changed behavior produces a concrete caller-visible result','observations':[o],'mechanism':{'trigger':'A direct call reaches the modified function branch','observed_behavior':'The same invocation returns a different integer result','consequence':'Callers can observe the changed return contract'}}],'abstention':None}; sys.stdout.write(json.dumps({'schema':'arm-neutral.source-grounded-disposition@1','task_id':p['task_id'],'source_inventory_id':p['source_inventory']['source_inventory_id'],'disposition':d},sort_keys=True,separators=(',',':')))
""",encoding="utf-8",newline="\n")
            judge.write_text("""#!/usr/bin/python3
import json,sys
b=json.loads(sys.stdin.buffer.read().rsplit(b'\\n',1)[-1]); scores=[]
for c in b['candidates']:
 f=c['mechanical_state']['kind']=='mechanical_forced_zero'; v=0 if f else 2; scores.append({'candidate_id':c['candidate_id'],'packet_sha256':c['packet_sha256'],'output_artifact_sha256':c['output_artifact_sha256'],'binding_view_sha256':c['binding_view_sha256'],'mechanical_score_sha256':c['mechanical_score_sha256'],'score_source':'mechanical_forced_zero' if f else 'judge','dimensions':{'source_specificity':v,'mechanism_or_blocker_specificity':v,'audit_actionability':v,'hidden_task_relevance':v},'total':0 if f else 8,'verdict':'not_usable' if f else 'usable'})
sys.stdout.write(json.dumps({'schema':'m20.utility_judge_batch_output.v1','batch_id':b['batch_id'],'scores':scores},sort_keys=True,separators=(',',':')))
""",encoding="utf-8",newline="\n")
            reviewer.chmod(0o500); judge.chmod(0o500)
            fixed_bin=parent/"fixed-bin"; shutil.copytree("/usr/local/bin",fixed_bin,symlinks=True); shutil.copy2(reviewer,fixed_bin/"m20-reviewer-backend"); shutil.copy2(judge,fixed_bin/"m20-judge-backend"); (fixed_bin/"m20-reviewer-backend").chmod(0o500); (fixed_bin/"m20-judge-backend").chmod(0o500)
            def cli(arguments):
                command=[str(bwrap),"--die-with-parent","--ro-bind","/","/","--dev","/dev","--bind","/tmp","/tmp","--ro-bind",str(fixed_bin),"/usr/local/bin","--chdir",str(benchmark),sys.executable,"-m","evaluator",*map(str,arguments)]
                return subprocess.run(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=False,timeout=180,env=env)
            controls=parent/"controls"; sealed=cli(["seal-controls",stage0/"stage0-selection.v1.json",left,right,controls]); self.assertEqual((sealed.returncode,sealed.stderr),(0,b""))
            stage1_root=parent/"stage1"; first=cli(["stage1",stage0/"stage0-selection.v1.json",stage1_root,"--controls",controls/"control-labels.v1.json"]); self.assertEqual((first.returncode,first.stderr),(0,b""),first.stdout.decode("utf-8","replace")); self.assertEqual(parse_json_bytes(first.stdout)["decision"],"advance",first.stdout.decode("utf-8","replace"))
            stage2_root=parent/"stage2a"; second=cli(["stage2a",stage1_root,stage2_root]); self.assertEqual((second.returncode,second.stderr),(0,b""),second.stdout.decode("utf-8","replace")); self.assertEqual((parse_json_bytes(second.stdout)["decision"],parse_json_bytes(second.stdout)["n"]),("success",40))
            verified=cli(["verify-stage",stage2_root]); self.assertEqual(verified.returncode,0,verified.stdout.decode("utf-8","replace"))
            if fixture_root.exists(): shutil.rmtree(fixture_root)


if __name__ == "__main__":
    unittest.main()
