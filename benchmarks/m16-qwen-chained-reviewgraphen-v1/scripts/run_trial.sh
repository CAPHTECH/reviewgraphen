#!/usr/bin/env bash
set -uo pipefail
if [[ $# -ne 4 ]]; then
  echo "usage: run_trial.sh <trial-id> <model> <low|high> <fresh-result-dir>" >&2
  exit 64
fi
trial=$1
model=$2
effort=$3
result=$4
[[ "$model" == Qwen3.8-27B-MLX-4bit || "$model" == Qwen3.8-27B-MLX-8bit ]] || exit 64
[[ "$effort" == low || "$effort" == high ]] || exit 64
[[ ! -e "$result" ]] || { echo "result directory must be fresh" >&2; exit 64; }
root=/home/rizumita/workspace/reviewgraphen
exp=$root/benchmarks/m16-qwen-chained-reviewgraphen-v1
cards_exp=$root/benchmarks/m15-qwen-intelligent-reviewgraphen-v1
base=$root/benchmarks/m11-review-agentic-local-v1
bwrap=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
scratch=/tmp/m16-scratch-$trial
config=/tmp/m16-config-$trial
mkdir -p "$result"
python3 "$base/scripts/check_backend_identity.py" "$(cat "$exp/PINNED_BACKEND_IDENTITY")" "$(cat "$exp/PINNED_BACKEND_HEALTH")" "$result/backend-identity.json" || exit 66
rm -rf "$scratch" "$config"
mkdir -p "$scratch/.reviewgraphen/bin" "$scratch/.reviewgraphen/cards" "$config"
chmod 700 "$config"
cp "$exp/scripts/reviewgraphen-context" "$scratch/.reviewgraphen/bin/"
cp "$cards_exp/cards/"*.json "$scratch/.reviewgraphen/cards/"
chmod 755 "$scratch/.reviewgraphen/bin/reviewgraphen-context"
started=$(date +%s)
set +e
"$bwrap" --ro-bind / / --dev-bind /dev /dev --proc /proc \
  --tmpfs /home/rizumita/workspace --tmpfs /tmp \
  --bind "$scratch" "$scratch" --bind "$config" "$config" \
  --ro-bind "$exp" "$exp" --ro-bind "$cards_exp" "$cards_exp" --ro-bind "$base" "$base" \
  --setenv CLAUDE_CONFIG_DIR "$config" --setenv ANTHROPIC_BASE_URL http://192.168.68.71:11999 \
  --setenv ANTHROPIC_API_KEY ollama --setenv CLAUDE_CODE_MAX_OUTPUT_TOKENS 12000 \
  --setenv PATH "$scratch/.reviewgraphen/bin:/home/rizumita/.local/share/mise/installs/claude/latest:/usr/local/bin:/usr/bin:/bin" \
  --setenv HOME /home/rizumita --chdir "$scratch" \
  timeout 900 claude --print --model "$model" --effort "$effort" \
    --output-format stream-json --verbose --permission-mode bypassPermissions \
    --tools "Bash,Write" --allowedTools "Bash,Write" \
    < "$exp/task/PROMPT.md" > "$result/stream.jsonl" 2> "$result/claude.stderr"
status=$?
set -e
finished=$(date +%s)
printf '%s\n' "$status" > "$result/agent-status"
printf '%s\n' "$((finished-started))" > "$result/elapsed-seconds"
printf '%s\n' "$model" > "$result/requested-model"
printf '%s\n' "$effort" > "$result/requested-effort"
[[ -f "$scratch/review.json" ]] && cp "$scratch/review.json" "$result/review.json"
analysis_ok=false
if python3 "$exp/scripts/analyze_trial.py" "$result" "$cards_exp/cards/inventory.json"; then analysis_ok=true; fi
python3 "$base/scripts/output_profile.py" "$result" > /dev/null
if [[ $status -eq 0 && "$analysis_ok" == true ]]; then outcome=completed_chained_use
elif [[ $status -eq 124 ]]; then outcome=timeout
else outcome=incomplete_or_protocol_violation
fi
printf '%s\n' "$outcome" > "$result/outcome"
sha256sum "$result/stream.jsonl" > "$result/stream.jsonl.sha256"
rm -rf "$config"
echo "trial=$trial model=$model effort=$effort outcome=$outcome elapsed=$((finished-started))s"
[[ "$outcome" == completed_chained_use ]]

