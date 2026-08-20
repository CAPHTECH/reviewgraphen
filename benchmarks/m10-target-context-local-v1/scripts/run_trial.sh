#!/usr/bin/env bash
# One isolated projection/control agentic implementation trial.
# usage: run_trial.sh <projection|control> <trial-id> <result-dir>
set -uo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <projection|control> <trial-id> <result-dir>" >&2
  exit 64
fi
arm=$1
trial=$2
result=$3
if [[ "$arm" != "projection" && "$arm" != "control" ]]; then
  echo "arm must be projection or control" >&2
  exit 64
fi
if [[ -e "$result" ]]; then
  echo "result directory must be fresh: $result" >&2
  exit 64
fi

root=/home/rizumita/workspace/reviewgraphen
exp="$root/benchmarks/m10-target-context-local-v1"
bwrap=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
scratch=/tmp/m10-scratch-$trial
target=/tmp/m10-target-$trial
config=/tmp/m10-config-$trial
timeout_seconds=5400

mkdir -p "$result"
if ! python3 "$exp/scripts/check_backend_identity.py" \
     "$(cat "$exp/PINNED_BACKEND_IDENTITY")" \
     "$(cat "$exp/PINNED_BACKEND_HEALTH")" \
     "$result/backend-identity.json"; then
  echo "backend identity gate failed; refusing to start $trial" >&2
  exit 66
fi

rm -rf "$scratch" "$target" "$config"
mkdir -p "$target" "$config"
chmod 700 "$config"
if [[ "$arm" == "projection" ]]; then
  bash "$exp/scripts/make_scratch.sh" "$scratch" --with-projection >/dev/null
else
  bash "$exp/scripts/make_scratch.sh" "$scratch" >/dev/null
fi
(cd "$scratch" && find . -type f -not -path './.git/*' -exec sha256sum {} + | sort -k2) \
  > "$result/pre-loop-manifest.txt"

started=$(date +%s)
set +e
setsid --wait bash "$exp/scripts/pgid_exec.sh" "$result/trial.pgid" "$bwrap" \
  --ro-bind / / \
  --dev-bind /dev /dev \
  --proc /proc \
  --tmpfs /home/rizumita/workspace \
  --tmpfs /tmp/claude-1000 \
  --bind "$scratch" "$scratch" \
  --bind "$target" "$target" \
  --bind "$config" "$config" \
  --bind /home/rizumita/.cargo /home/rizumita/.cargo \
  --setenv CARGO_TARGET_DIR "$target" \
  --setenv REVIEWGRAPHEN_TRUSTED_CARGO /home/rizumita/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo \
  --setenv CLAUDE_CONFIG_DIR "$config" \
  --setenv ANTHROPIC_BASE_URL http://192.168.68.71:11999 \
  --setenv ANTHROPIC_API_KEY ollama \
  --setenv CLAUDE_CODE_MAX_OUTPUT_TOKENS 32000 \
  --setenv PATH "$scratch/.reviewgraphen/bin:/home/rizumita/.local/share/mise/installs/claude/latest:/home/rizumita/.cargo/bin:/usr/local/bin:/usr/bin:/bin" \
  --setenv HOME /home/rizumita \
  --chdir "$scratch" \
  timeout "$timeout_seconds" claude \
    --print \
    --model qwen3.8:27b-mlx \
    --output-format stream-json \
    --verbose \
    --permission-mode bypassPermissions \
    --allowedTools "Read,Edit,Write,Bash,Grep,Glob" \
    < "$exp/task/PROMPT-${arm}.md" \
    > "$result/stream.jsonl" 2> "$result/claude.stderr"
status=$?
set -e
finished=$(date +%s)

printf '%s\n' "$status" > "$result/claude-status"
printf '%s\n' "$((finished - started))" > "$result/elapsed-seconds"
if (( status == 124 )); then
  printf 'loop_incomplete_timeout\n' > "$result/loop-outcome"
elif (( status != 0 )); then
  printf 'loop_incomplete_error\n' > "$result/loop-outcome"
else
  printf 'completed\n' > "$result/loop-outcome"
fi

(cd "$scratch" && find . -type f -not -path './.git/*' -exec sha256sum {} + | sort -k2) \
  > "$result/post-loop-manifest.txt"
cp -r "$scratch" "$result/worktree"
rm -rf "$config"
echo "trial=$trial arm=$arm status=$status outcome=$(cat "$result/loop-outcome") elapsed=$((finished - started))s"
