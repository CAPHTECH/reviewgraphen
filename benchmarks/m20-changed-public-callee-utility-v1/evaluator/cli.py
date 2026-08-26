"""Closed public CLI; paired runs and stage reductions have no public input."""
import argparse
import base64
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
from .pipeline import PipelineError, _response_hmac_key, _seal_backend_response
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
        try:
            registration = parse_json_bytes((Path(__file__).resolve().parent.parent / "preregistration.json").read_bytes())
            gate = registration.get("backend_gate", {})
            self._backend_pins = {"listing":gate.get("pinned_listing_sha256"), "health":gate.get("pinned_health_sha256")}
            executable_pins = {"reviewer":gate.get("reviewer_transport_sha256"), "judge":gate.get("judge_transport_sha256")}
            if any(not isinstance(value, str) or len(value) != 64 for value in self._backend_pins.values()):
                raise PipelineError("backend_pin_contract_invalid", 3)
            if any(not isinstance(value, str) or len(value) != 71 or not value.startswith("sha256:") for value in executable_pins.values()):
                raise PipelineError("backend_executable_pin_contract_invalid", 3)
            if self._pins != executable_pins:
                raise PipelineError("backend_executable_identity_mismatch", 3)
            if registration.get("arms", {}).get("common", {}).get("requested_backend_max_output_tokens") != 12_000:
                raise PipelineError("reviewer_budget_contract_invalid", 3)
            if sha256_bytes(_response_hmac_key()) != gate.get("response_hmac_key_sha256"):
                raise PipelineError("backend_response_hmac_key_preregistration_mismatch", 3)
            self.preflight()
        except Exception:
            self._workspace.cleanup()
            raise

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

    def preflight(self):
        if self._identity(self.REVIEWER) != self._pins["reviewer"]:
            raise PipelineError("backend_identity_changed", 3)
        try:
            result = subprocess.run([str(self.REVIEWER), "--gate"], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, cwd=self._workspace.name, env={"PATH":"", "LC_ALL":"C", "LANG":"C"}, shell=False, timeout=90, check=False)
            value = parse_json_bytes(result.stdout)
        except (OSError, ValueError, subprocess.TimeoutExpired) as error:
            raise PipelineError("backend_identity_health_unavailable", 3) from error
        if result.returncode != 0 or not isinstance(value, dict) or set(value) != {"schema", "listing", "health"} or value["schema"] != "m20.backend_identity_health.v1" or not isinstance(value["listing"], list) or not isinstance(value["health"], dict):
            raise PipelineError("backend_identity_health_invalid", 3)
        observed = {"listing":sha256_bytes(canonical_bytes(value["listing"]))[7:], "health":sha256_bytes(canonical_bytes(value["health"]))[7:]}
        if observed != self._backend_pins:
            raise PipelineError("backend_identity_health_mismatch", 3)
        return observed

    def _invoke(self, name, path, arguments, data, timeout, contract):
        if self._identity(path) != self._pins[name]:
            raise PipelineError("backend_identity_changed", 3)
        try:
            result = subprocess.run([str(path), *arguments], input=canonical_bytes(data), stdin=None, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, cwd=self._workspace.name, env={"PATH":"", "LC_ALL":"C", "LANG":"C"}, shell=False, timeout=timeout, check=False)
            if result.returncode != 0:
                response = self._failure_response(data, contract, "process_error")
                return ModelResult(b"", process_exit=result.returncode, backend_request=data, backend_response=response, backend_response_hmac=_seal_backend_response(canonical_bytes(response)))
            response_raw = result.stdout
            response_hmac = _seal_backend_response(response_raw)
            response = parse_json_bytes(response_raw)
            expected = {"schema", "request_seal_sha256", "effective_max_output_tokens", "effective_timeout_seconds", "finish_reason", "usage", "raw_response_base64"}
            usage = response.get("usage") if isinstance(response, dict) else None
            usage_valid = isinstance(usage, dict) and set(usage) == {"input_tokens", "output_tokens", "cache_tokens"} and all(value is None or isinstance(value, int) and not isinstance(value, bool) and value >= 0 for value in usage.values())
            if not isinstance(response, dict) or set(response) != expected or response["schema"] != "m20.fixed-backend-response.v1" or response["request_seal_sha256"] != data["request_seal_sha256"] or response["effective_max_output_tokens"] != contract["max_output_tokens"] or response["effective_timeout_seconds"] != contract["timeout_seconds"] or response["finish_reason"] not in {"stop", "length", "tool_calls", "error", "unknown"} or not usage_valid or not isinstance(response["raw_response_base64"], str):
                raise PipelineError("backend_effective_contract_mismatch", 4)
            try:
                raw = base64.b64decode(response["raw_response_base64"], validate=True)
            except (ValueError, TypeError) as error:
                raise PipelineError("backend_effective_contract_mismatch", 4) from error
            observed_usage = tuple((name, usage[name]) for name in ("input_tokens", "output_tokens", "cache_tokens") if usage[name] is not None)
            return ModelResult(raw, process_exit=0, provider_truncation=response["finish_reason"] == "length", usage=observed_usage, backend_request=data, backend_response=response, backend_response_hmac=response_hmac)
        except (ValueError, UnicodeError) as error:
            raise PipelineError("backend_effective_contract_mismatch", 4) from error
        except subprocess.TimeoutExpired as error:
            raw = error.stdout or b""
            response = self._failure_response(data, contract, "transport_timeout", raw)
            return ModelResult(raw, timeout=True, backend_request=data, backend_response=response, backend_response_hmac=_seal_backend_response(canonical_bytes(response)))

    @staticmethod
    def _failure_response(request, contract, finish_reason, raw=b""):
        return {"schema":"m20.fixed-backend-response.v1", "request_seal_sha256":request["request_seal_sha256"], "effective_max_output_tokens":contract["max_output_tokens"], "effective_timeout_seconds":contract["timeout_seconds"], "finish_reason":finish_reason, "usage":{"input_tokens":None,"output_tokens":None,"cache_tokens":None}, "raw_response_base64":base64.b64encode(raw).decode("ascii")}

    @staticmethod
    def _sealed(kind, payload, instruction, max_output_tokens, timeout_seconds):
        body = {"schema":"m20.fixed-backend-request.v1", "kind":kind, "payload":parse_json_bytes(payload), "instruction":instruction, "max_output_tokens":max_output_tokens, "timeout_seconds":timeout_seconds}
        body["request_seal_sha256"] = sha256_bytes(canonical_bytes(body))
        return body

    def review(self, request, slot, timeout):
        if timeout != 900 or isinstance(slot, bool) or slot not in (0, 1):
            raise PipelineError("reviewer_budget_contract_invalid", 4)
        self.preflight()
        contract={"max_output_tokens":12_000,"timeout_seconds":900}
        sealed=self._sealed("reviewer",request,None,contract["max_output_tokens"],contract["timeout_seconds"])
        return self._invoke("reviewer", self.REVIEWER, ["--max-output-tokens", "12000", "--timeout-seconds", "900"], sealed, 900, contract)

    def judge(self, request, instruction, timeout):
        if timeout != 90:
            raise PipelineError("judge_budget_contract_invalid", 4)
        contract={"max_output_tokens":12_000,"timeout_seconds":90}
        sealed=self._sealed("judge",request,instruction.decode("utf-8","strict"),contract["max_output_tokens"],contract["timeout_seconds"])
        return self._invoke("judge", self.JUDGE, ["--timeout-seconds", "90"], sealed, 90, contract)


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
    command = sub.add_parser("verify-stage"); command.add_argument("stage_root"); command.add_argument("--reexecute-all", action="store_true")
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
            result = production_stage0(args.output_root, jobs=args.jobs); _emit({"schema":result["schema"], "output_root":result["output_root"], "selection":result["selection"].value, "stage0_artifact_manifest_sha256":result["stage0_artifact_manifest_sha256"]}); return 0
        if args.command == "seal-controls": result = seal_controls(args.stage0_selection, args.labeler_1, args.labeler_2, args.new_control_root); _emit(result); return 0
        if args.command == "stage1": result = stage1(args.stage0_selection, args.new_output_root, args.controls, FixedProcessTransport()); _emit(result); return 0
        if args.command == "stage2a": result = stage2a(args.stage1_root, args.new_output_root, FixedProcessTransport()); _emit(result); return 0
        if args.command == "verify-stage": result = verify_stage(args.stage_root,args.reexecute_all); _emit(result); return 0 if result["ok"] else 2
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
    except (OSError, KeyError, ValueError) as error:
        sys.stderr.buffer.write(canonical_bytes({"schema":"m20.cli-unhandled-error-diagnostic.v1", "exception_type":type(error).__name__, "message":str(error)}) + b"\n")
        code = "invalid_input_io" if isinstance(error, OSError) else "invalid_input_missing_field" if isinstance(error, KeyError) else "invalid_input_value"; _emit({"schema":"m20.cli-error.v1", "code":code}); return 2
