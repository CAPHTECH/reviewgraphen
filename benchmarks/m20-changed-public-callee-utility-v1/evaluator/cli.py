"""Closed public CLI; paired runs and stage reductions have no public input."""
import argparse
import os
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

from .artifacts import verify_run
from .canonical import canonical_bytes, parse_json_bytes, sha256_bytes
from .freeze import check_generated, freeze_manifest, verify_manifest
from .model_boundary import ModelResult
from .pipeline import PipelineError
from .stage0_driver import Stage0Error
from .stage_driver import JUDGE_ADAPTER, JUDGE_PATH, REVIEWER_ADAPTER, REVIEWER_PATH, seal_controls, stage1, stage2a, verify_stage

COMMANDS = ("stage0", "seal-controls", "stage1", "stage2a", "verify-stage", "verify-run", "generate-fixtures", "verify-reference-vectors", "run-attacks", "mutation-sweep", "freeze-manifest", "verify-frozen")


class FixedProcessTransport:
    """The only production model transport; selectors are compile-time constants."""
    REVIEWER = Path(REVIEWER_PATH)
    JUDGE = Path(JUDGE_PATH)

    def __init__(self):
        self._pins = {"reviewer": self._identity(self.REVIEWER), "judge": self._identity(self.JUDGE)}
        self._workspace = tempfile.TemporaryDirectory(prefix="m20-fixed-transport-")

    @staticmethod
    def _identity(path: Path) -> str:
        try:
            status = path.lstat()
        except OSError as error:
            raise PipelineError("backend_adapter_unavailable", 2) from error
        if path.is_symlink() or not stat.S_ISREG(status.st_mode) or status.st_mode & 0o111 == 0:
            raise PipelineError("backend_adapter_unavailable", 2)
        return sha256_bytes(path.read_bytes())

    def descriptor(self):
        return {"reviewer": REVIEWER_ADAPTER, "judge": JUDGE_ADAPTER}

    def _invoke(self, name, path, data, timeout):
        if self._identity(path) != self._pins[name]:
            raise PipelineError("backend_identity_changed", 3)
        try:
            result = subprocess.run([str(path)], input=data, stdin=None, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, cwd=self._workspace.name, env={"PATH":"", "LC_ALL":"C", "LANG":"C"}, shell=False, timeout=timeout, check=False)
            return ModelResult(result.stdout, process_exit=result.returncode)
        except subprocess.TimeoutExpired as error:
            return ModelResult(error.stdout or b"", timeout=True)

    def review(self, request, slot, timeout):
        if timeout != 900 or isinstance(slot, bool) or slot not in (0, 1):
            raise PipelineError("reviewer_budget_contract_invalid", 4)
        return self._invoke("reviewer", self.REVIEWER, request, 900)

    def judge(self, request, instruction, timeout):
        if timeout != 90:
            raise PipelineError("judge_budget_contract_invalid", 4)
        return self._invoke("judge", self.JUDGE, instruction + b"\n" + request, 90)


def _emit(value):
    sys.stdout.buffer.write(canonical_bytes(value) + b"\n")


class _TypedParser(argparse.ArgumentParser):
    def error(self, message):
        raise PipelineError("cli_arguments_invalid", 2)


def _parser():
    parser = _TypedParser(prog="python3 -m evaluator")
    sub = parser.add_subparsers(dest="command", required=True)
    command = sub.add_parser("stage0"); command.add_argument("output_root"); command.add_argument("--jobs", type=int)
    command = sub.add_parser("seal-controls"); command.add_argument("stage0_selection"); command.add_argument("labeler_1"); command.add_argument("labeler_2"); command.add_argument("new_control_root")
    command = sub.add_parser("stage1"); command.add_argument("stage0_selection"); command.add_argument("new_output_root"); command.add_argument("--controls", required=True)
    command = sub.add_parser("stage2a"); command.add_argument("stage1_root"); command.add_argument("new_output_root")
    sub.add_parser("verify-stage").add_argument("stage_root")
    sub.add_parser("verify-run").add_argument("run_root")
    command = sub.add_parser("generate-fixtures"); command.add_argument("--check", action="store_true", required=True)
    sub.add_parser("verify-reference-vectors")
    sub.add_parser("run-attacks")
    command = sub.add_parser("mutation-sweep"); command.add_argument("output"); command.add_argument("--jobs", type=int, default=1)
    sub.add_parser("freeze-manifest").add_argument("output")
    sub.add_parser("verify-frozen").add_argument("manifest")
    return parser


def main(argv=None):
    try:
        args = _parser().parse_args(argv); root = Path(__file__).resolve().parent; design = root.parent / "EVALUATOR_SPEC.md"
        if args.command == "stage0":
            from .stage0_driver import production_stage0
            result = production_stage0(args.output_root, jobs=args.jobs); _emit({"schema":result["schema"], "output_root":result["output_root"], "selection":result["selection"].value}); return 0
        if args.command == "seal-controls": result = seal_controls(args.stage0_selection, args.labeler_1, args.labeler_2, args.new_control_root); _emit(result); return 0
        if args.command == "stage1": result = stage1(args.stage0_selection, args.new_output_root, args.controls, FixedProcessTransport()); _emit(result); return 0
        if args.command == "stage2a": result = stage2a(args.stage1_root, args.new_output_root, FixedProcessTransport()); _emit(result); return 0
        if args.command == "verify-stage": result = verify_stage(args.stage_root); _emit(result); return 0 if result["ok"] else 2
        if args.command == "verify-run": result = verify_run(Path(args.run_root)); _emit(result); return 0 if result["ok"] else 2
        if args.command == "generate-fixtures": ok = check_generated(root / "generated"); _emit({"schema":"m20.generate-fixtures-check.v1", "ok":ok}); return 0 if ok else 2
        if args.command == "verify-reference-vectors":
            from .tests.vector_runner import run_vectors
            result = run_vectors(); _emit(result); return 0 if result["failed"] == 0 and result["total"] == 74 else 2
        if args.command == "run-attacks":
            from .tests.attack_oracles import run_all_attacks
            result = run_all_attacks(root); _emit(result); return 0 if result["survived"] == 0 else 2
        if args.command == "mutation-sweep":
            if args.jobs < 1 or args.jobs > 64: raise PipelineError("cli_arguments_invalid", 2)
            from .tests.mutation_sweep import write_sweep
            result = write_sweep(root, Path(args.output), args.jobs); _emit(result["summary"]); return 0 if result["summary"]["score_affecting_survived"] == 0 and result["summary"]["undetermined_survived"] == 0 else 2
        if args.command == "freeze-manifest":
            output = Path(args.output)
            if output.exists() or output.is_symlink(): raise PipelineError("output_exists", 2)
            result = freeze_manifest(root, design); output.write_bytes(canonical_bytes(result)); _emit(result); return 0
        manifest = parse_json_bytes(Path(args.manifest).read_bytes()); result = verify_manifest(root, design, manifest); _emit(result); return 0 if result["ok"] else 3
    except (PipelineError, Stage0Error) as error:
        _emit({"schema":"m20.cli-error.v1", "code":error.code})
        if isinstance(error, Stage0Error) and error.diagnostic is not None: sys.stderr.buffer.write(canonical_bytes(error.diagnostic) + b"\n")
        return error.exit_code
    except (OSError, ValueError, KeyError) as error:
        _emit({"schema":"m20.cli-error.v1", "code":"invalid_input"}); return 2
