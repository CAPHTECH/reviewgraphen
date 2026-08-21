#!/usr/bin/env bash
set -uo pipefail
if [[ $# -ne 2 ]]; then
  echo "usage: run_trial.sh <trial-id> <fresh-result-dir>" >&2
  exit 64
fi
trial=$1
result=$2
[[ ! -e "$result" ]] || { echo "result directory must be fresh" >&2; exit 64; }

root=/home/rizumita/workspace/reviewgraphen
exp=$root/benchmarks/m12-review-convergent-local-v1
base=$root/benchmarks/m11-review-agentic-local-v1
bwrap=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
scratch=/tmp/m12-scratch-$trial
config=/tmp/m12-config-$trial
mkdir -p "$result"
python3 "$base/scripts/check_backend_identity.py" \
  "$(cat "$exp/PINNED_BACKEND_IDENTITY")" \
  "$(cat "$exp/PINNED_BACKEND_HEALTH")" \
  "$result/backend-identity.json" || exit 66
rm -rf "$scratch" "$config"
mkdir -p "$config"
chmod 700 "$config"
bash "$base/scripts/make_scratch.sh" "$scratch" --with-reviewgraphen
find "$scratch/rust" -type f -exec sha256sum {} + | sort -k2 > "$result/pre-source-manifest.txt"
sed "s#  $scratch/#  #" "$result/pre-source-manifest.txt" > "$result/pre-source-relative.txt"
if ! cmp -s "$base/SOURCE_MANIFEST.sha256" "$result/pre-source-relative.txt"; then
  echo "source identity gate failed" >&2
  exit 65
fi

started=$(date +%s)
set +e
"$bwrap" \
  --ro-bind / / --dev-bind /dev /dev --proc /proc \
  --tmpfs /home/rizumita/workspace --tmpfs /tmp/claude-1000 \
  --bind "$scratch" "$scratch" --bind "$config" "$config" \
  --ro-bind "$exp" "$exp" --ro-bind "$base" "$base" \
  --setenv CLAUDE_CONFIG_DIR "$config" \
  --setenv ANTHROPIC_BASE_URL http://192.168.68.71:11999 \
  --setenv ANTHROPIC_API_KEY ollama \
  --setenv CLAUDE_CODE_MAX_OUTPUT_TOKENS 12000 \
  --setenv PATH "$scratch/.reviewgraphen/bin:/home/rizumita/.local/share/mise/installs/claude/latest:/usr/local/bin:/usr/bin:/bin" \
  --setenv HOME /home/rizumita --chdir "$scratch" \
  timeout 1800 claude --print --model Qwen3.8-27B-MLX-4bit --effort low \
    --output-format stream-json --verbose --permission-mode bypassPermissions \
    --allowedTools "Read,Write,Bash,Grep,Glob" \
    < "$exp/task/PROMPT.md" > "$result/stream.jsonl" 2> "$result/claude.stderr"
status=$?
set -e
finished=$(date +%s)
printf '%s\n' "$status" > "$result/agent-status"
printf '%s\n' "$((finished-started))" > "$result/elapsed-seconds"
find "$scratch/rust" -type f -exec sha256sum {} + | sort -k2 > "$result/post-source-manifest.txt"
if [[ -f "$scratch/review.json" ]]; then cp "$scratch/review.json" "$result/review.json"; fi
source_unchanged=false
cmp -s "$result/pre-source-manifest.txt" "$result/post-source-manifest.txt" && source_unchanged=true
report_valid=false
if [[ -f "$result/review.json" ]] && python3 "$base/scripts/validate_report.py" "$result/review.json" > "$result/report-validation.json"; then report_valid=true; fi
protocol_ok=false
if python3 "$exp/scripts/check_protocol.py" "$result"; then protocol_ok=true; fi
python3 "$base/scripts/output_profile.py" "$result" > /dev/null
if python3 "$base/scripts/detect_truncated_tail.py" "$result" > /dev/null; then tail_status=0; else tail_status=$?; fi
if [[ $status -eq 0 && "$source_unchanged" == true && "$report_valid" == true && "$protocol_ok" == true && $tail_status -eq 0 ]]; then
  outcome=completed_valid_convergent
elif [[ $status -eq 124 && "$report_valid" == true ]]; then outcome=timeout_with_valid_checkpoint
elif [[ "$report_valid" == true && "$protocol_ok" == false ]]; then outcome=valid_report_protocol_violation
elif [[ $status -eq 124 ]]; then outcome=timeout_without_valid_report
elif [[ $tail_status -ne 0 ]]; then outcome=truncated_tail
else outcome=incomplete_or_invalid
fi
printf '%s\n' "$outcome" > "$result/outcome"
rm -rf "$config"
echo "trial=$trial outcome=$outcome elapsed=$((finished-started))s"
[[ "$outcome" == completed_valid_convergent ]]
