#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <prepared-root> <fresh-or-resumable-runs-root> <benchmark-bin>" >&2
  exit 64
fi

prepared_root=$(realpath -e -- "$1")
runs_arg=$2
benchmark_bin=$(realpath -e -- "$3")
inventory="$prepared_root/calibration-inventory.private.json"

if [[ ! -f "$inventory" ]]; then
  echo "missing calibration inventory: $inventory" >&2
  exit 66
fi
if [[ $(jq -r '.schema + ":" + (.unit_count | tostring)' "$inventory") != \
  reviewgraphen.benchmark.m7_head_calibration_inventory.v1:20 ]]; then
  echo "unexpected calibration inventory" >&2
  exit 65
fi
if [[ ! -x "$benchmark_bin" ]]; then
  echo "benchmark binary is not executable: $benchmark_bin" >&2
  exit 66
fi

mkdir -p -- "$runs_arg"
runs_root=$(realpath -e -- "$runs_arg")
case "$runs_root" in
  /tmp/m7-head-calibration-runs|*/target/m7-head-calibration-runs) ;;
  *)
    echo "refusing runs root outside the fixed calibration locations: $runs_root" >&2
    exit 64
    ;;
esac

bwrap_bin=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
codex_bin=/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex
for executable in "$bwrap_bin" "$codex_bin"; do
  if [[ ! -x "$executable" ]]; then
    echo "missing executable: $executable" >&2
    exit 66
  fi
done

credential_home=$(mktemp -d /tmp/m7-head-codex-home.XXXXXX)
cleanup() {
  rm -rf -- "$credential_home"
}
trap cleanup EXIT INT TERM
for auth_file in auth.json installation_id models_cache.json; do
  if [[ -f "/home/rizumita/.codex/$auth_file" ]]; then
    cp -a -- "/home/rizumita/.codex/$auth_file" "$credential_home/$auth_file"
  fi
done
if [[ ! -f "$credential_home/auth.json" ]]; then
  echo "no Codex credential was admitted" >&2
  exit 66
fi

mapfile -t calibration_ids < <(
  jq -r '.units[].calibration_id' "$inventory"
)
for calibration_id in "${calibration_ids[@]}"; do
  if [[ ! "$calibration_id" =~ ^calibration-[0-9][0-9]$ ]]; then
    echo "invalid calibration id: $calibration_id" >&2
    exit 65
  fi
  input_root="$prepared_root/$calibration_id/input"
  result_root="$runs_root/$calibration_id"
  output_root="$result_root/provider-output"
  record="$result_root/record.json"
  if [[ -s "$record" ]]; then
    echo "$calibration_id already recorded"
    continue
  fi
  if [[ -e "$result_root" ]]; then
    echo "refusing implicit retry after incomplete attempt: $result_root" >&2
    exit 73
  fi
  mkdir -p -- "$result_root"
  echo "$calibration_id starting"
  set +e
  "$benchmark_bin" run-process-reviewer-constrained \
    codex \
    "$input_root" \
    output.schema.json \
    "$output_root" \
    "$record" \
    "$bwrap_bin" \
    "$credential_home" \
    "$codex_bin" \
    gpt-5.6-sol \
    high \
    >"$result_root/adapter.stdout" \
    2>"$result_root/adapter.stderr"
  status=$?
  set -e
  printf '%s\n' "$status" >"$result_root/exit-status"
  if (( status != 0 )); then
    echo "$calibration_id adapter failed with status $status" >&2
    exit "$status"
  fi
  echo "$calibration_id recorded"
done
