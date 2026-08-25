"""Fixed public CLI: no intermediate scoring commands or inputs."""; import argparse; import subprocess; import sys; from pathlib import Path; from .artifacts import verify_run; from .canonical import canonical_bytes, parse_json_bytes; from .freeze import check_generated, freeze_manifest, verify_manifest; from .model_boundary import ModelResult; from .pipeline import PipelineError, RUN; from .stage0_driver import Stage0Error; COMMANDS = ("stage0", "run", "verify-run", "generate-fixtures", "verify-reference-vectors", "run-attacks", "mutation-sweep", "freeze-manifest", "verify-frozen")
class FixedProcessTransport:
    REVIEWER = Path("/usr/local/bin/m20-reviewer-backend"); JUDGE = Path("/usr/local/bin/m20-judge-backend")
    def __init__(self, descriptor):
        if descriptor != {"reviewer":"m20.fixed-reviewer-process.v1","judge":"m20.fixed-judge-process.v1"} or any(path.is_symlink() or not path.is_file() for path in (self.REVIEWER,self.JUDGE)): raise PipelineError("backend_adapter_unavailable",2)
        self._descriptor=descriptor
    def descriptor(self): return self._descriptor

    def _invoke(self,path,data,timeout):
        try: result=subprocess.run([str(path)],input=data,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,cwd=Path(__file__).resolve().parent,env={"PATH":"","LC_ALL":"C"},shell=False,timeout=timeout,check=False); return ModelResult(result.stdout,process_exit=result.returncode)
        except subprocess.TimeoutExpired as error: return ModelResult(error.stdout or b"",timeout=True)
    def review(self,request,slot,timeout): return self._invoke(self.REVIEWER,request,timeout)
    def judge(self,request,instruction,timeout): return self._invoke(self.JUDGE,instruction+b"\n"+request,timeout)


def _emit(value): sys.stdout.buffer.write(canonical_bytes(value)+b"\n")
class _TypedParser(argparse.ArgumentParser):
    def error(self,message): raise PipelineError("cli_arguments_invalid",2)
def _parser(): parser=_TypedParser(prog="python3 -m evaluator"); sub=parser.add_subparsers(dest="command",required=True); command=sub.add_parser("stage0"); command.add_argument("output_root"); command.add_argument("--jobs",type=int); command=sub.add_parser("run"); command.add_argument("launch"); command.add_argument("output_root"); sub.add_parser("verify-run").add_argument("run_root"); command=sub.add_parser("generate-fixtures"); command.add_argument("--check",action="store_true",required=True); sub.add_parser("verify-reference-vectors"); sub.add_parser("run-attacks"); command=sub.add_parser("mutation-sweep"); command.add_argument("output"); command.add_argument("--jobs",type=int,default=1); sub.add_parser("freeze-manifest").add_argument("output"); sub.add_parser("verify-frozen").add_argument("manifest"); return parser
def main(argv=None):
    try:
        args=_parser().parse_args(argv); root=Path(__file__).resolve().parent; design=root.parent/"EVALUATOR_SPEC.md"
        if args.command=="stage0":
            from .stage0_driver import production_stage0
            result=production_stage0(args.output_root,jobs=args.jobs); _emit({"schema":result["schema"],"output_root":result["output_root"],"selection":result["selection"].value}); return 0
        if args.command=="run": launch=parse_json_bytes(Path(args.launch).read_bytes()); stage=parse_json_bytes(Path(launch["stage_manifest_path"]).read_bytes()); result=RUN(launch,FixedProcessTransport(stage["backend_adapters"]),args.output_root); _emit(result); return 0
        if args.command=="verify-run": result=verify_run(Path(args.run_root)); _emit(result); return 0 if result["ok"] else 2
        if args.command=="generate-fixtures": ok=check_generated(root/"generated"); _emit({"schema":"m20.generate-fixtures-check.v1","ok":ok}); return 0 if ok else 2
        if args.command=="verify-reference-vectors": from .tests.vector_runner import run_vectors; result=run_vectors(); _emit(result); return 0 if result["failed"]==0 and result["total"]==74 else 2
        if args.command=="run-attacks": from .tests.attack_oracles import run_all_attacks; result=run_all_attacks(root); _emit(result); return 0 if result["survived"]==0 else 2
        if args.command=="mutation-sweep":
            if args.jobs < 1 or args.jobs > 64: raise PipelineError("cli_arguments_invalid",2)
            from .tests.mutation_sweep import write_sweep
            result=write_sweep(root,Path(args.output),args.jobs); _emit(result["summary"]); return 0 if result["summary"]["score_affecting_survived"]==0 and result["summary"]["undetermined_survived"]==0 else 2
        if args.command=="freeze-manifest":
            output=Path(args.output)
            if output.exists() or output.is_symlink(): raise PipelineError("output_exists",2)
            result=freeze_manifest(root,design); output.write_bytes(canonical_bytes(result)); _emit(result); return 0
        manifest=parse_json_bytes(Path(args.manifest).read_bytes()); result=verify_manifest(root,design,manifest); _emit(result); return 0 if result["ok"] else 3
    except (PipelineError,Stage0Error) as error: _emit({"schema":"m20.cli-error.v1","code":error.code}); return error.exit_code
    except (OSError,ValueError,KeyError) as error: _emit({"schema":"m20.cli-error.v1","code":"invalid_input"}); return 2
