#!/usr/bin/env bash
set -uo pipefail
if [[ $# -ne 3 ]]; then
  echo "usage: run_trial.sh <reviewgraphen|control> <trial-id> <result-dir>" >&2
  exit 64
fi
arm=$1
trial=$2
result=$3
[[ "$arm" == reviewgraphen || "$arm" == control ]] || exit 64
[[ ! -e "$result" ]] || { echo "result directory must be fresh" >&2; exit 64; }

root=/home/rizumita/workspace/reviewgraphen
exp=$root/benchmarks/m11-review-agentic-local-v1
bwrap=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
scratch=/tmp/m11-scratch-$trial
config=/tmp/m11-config-$trial
mkdir -p "$result"
identity_file=$exp/PINNED_BACKEND_IDENTITY
health_file=$exp/PINNED_BACKEND_HEALTH
if [[ "$arm" == control && -f "$exp/PINNED_BACKEND_IDENTITY-CONTROL-DIAGNOSTIC" ]]; then
  identity_file=$exp/PINNED_BACKEND_IDENTITY-CONTROL-DIAGNOSTIC
  health_file=$exp/PINNED_BACKEND_HEALTH-CONTROL-DIAGNOSTIC
fi
python3 "$exp/scripts/check_backend_identity.py" \
  "$(cat "$identity_file")" \
  "$(cat "$health_file")" \
  "$result/backend-identity.json" || exit 66
rm -rf "$scratch" "$config"
mkdir -p "$config"
chmod 700 "$config"
if [[ "$arm" == reviewgraphen ]]; then
  bash "$exp/scripts/make_scratch.sh" "$scratch" --with-reviewgraphen
else
  bash "$exp/scripts/make_scratch.sh" "$scratch"
fi
find "$scratch/rust" -type f -exec sha256sum {} + | sort -k2 > "$result/pre-source-manifest.txt"
sed "s#  $scratch/#  #" "$result/pre-source-manifest.txt" > "$result/pre-source-relative.txt"
if ! cmp -s "$exp/SOURCE_MANIFEST.sha256" "$result/pre-source-relative.txt"; then
  echo "source identity gate failed" >&2
  exit 65
fi

started=$(date +%s)
set +e
"$bwrap" \
  --ro-bind / / --dev-bind /dev /dev --proc /proc \
  --tmpfs /home/rizumita/workspace --tmpfs /tmp/claude-1000 \
  --bind "$scratch" "$scratch" --bind "$config" "$config" \
  --ro-bind "$exp" "$exp" \
  --setenv CLAUDE_CONFIG_DIR "$config" \
  --setenv ANTHROPIC_BASE_URL http://192.168.68.71:11999 \
  --setenv ANTHROPIC_API_KEY ollama \
  --setenv CLAUDE_CODE_MAX_OUTPUT_TOKENS 32000 \
  --setenv PATH "$scratch/.reviewgraphen/bin:/home/rizumita/.local/share/mise/installs/claude/latest:/usr/local/bin:/usr/bin:/bin" \
  --setenv HOME /home/rizumita --chdir "$scratch" \
  timeout 5400 claude --print --model qwen3.8:27b-mlx \
    --output-format stream-json --verbose --permission-mode bypassPermissions \
    --allowedTools "Read,Write,Bash,Grep,Glob" \
    < "$exp/task/PROMPT-$arm.md" > "$result/stream.jsonl" 2> "$result/claude.stderr"
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
if [[ -f "$result/review.json" ]] && python3 "$exp/scripts/validate_report.py" "$result/review.json" > "$result/report-validation.json"; then report_valid=true; fi
first_tool=true
if [[ "$arm" == reviewgraphen ]]; then
  python3 "$exp/scripts/check_first_tool.py" "$result" || first_tool=false
fi
python3 "$exp/scripts/output_profile.py" "$result"
if python3 "$exp/scripts/detect_truncated_tail.py" "$result" >/dev/null; then
  tail_status=0
else
  tail_status=$?
fi
if [[ $status -eq 0 && "$source_unchanged" == true && "$report_valid" == true && "$first_tool" == true && $tail_status -eq 0 ]]; then
  outcome=completed_valid
elif [[ "$first_tool" == false ]]; then outcome=intervention_not_used
elif [[ $status -eq 124 ]]; then outcome=timeout
elif [[ $tail_status -ne 0 ]]; then outcome=truncated_tail
else outcome=incomplete_or_invalid
fi
printf '%s\n' "$outcome" > "$result/outcome"
rm -rf "$config"
echo "trial=$trial arm=$arm outcome=$outcome elapsed=$((finished-started))s"
[[ "$outcome" == completed_valid ]]
