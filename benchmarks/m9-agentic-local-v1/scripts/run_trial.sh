#!/usr/bin/env bash
# One agentic trial: Claude Code, pointed at the local qwen server, with
# tools, inside a bubblewrap sandbox.
#
# Isolation (stricter than m8, because this agent really does write files):
#   --ro-bind / /                     the whole host filesystem is read-only
#   --tmpfs /home/rizumita/workspace  the real repository is not merely
#                                     unwritable, it is INVISIBLE. Otherwise
#                                     the agent could read the acceptance
#                                     test and every earlier result from the
#                                     checkout next door, and the blind
#                                     judgement would be worthless.
#   --bind <scratch>                  the only writable project tree
#   --bind <target>                   this trial's PRIVATE CARGO_TARGET_DIR.
#                                     m8 proved a shared one serves another
#                                     tree's library and produces wrong
#                                     measurements; an agentic loop runs
#                                     cargo far more often than m8 did.
#   --bind ~/.cargo                   registry cache, writable
#   fresh CLAUDE_CONFIG_DIR           no credentials exist inside, so there
#                                     is no session that could fall back to a
#                                     billed endpoint
#
# usage: run_trial.sh <arm> <trial-id> <result-dir>
set -uo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <skill|noskill> <trial-id> <result-dir>" >&2
  exit 64
fi
arm=$1
trial=$2
result=$3
if [[ "$arm" != "skill" && "$arm" != "noskill" ]]; then
  echo "arm must be skill or noskill" >&2
  exit 64
fi
if [[ -e "$result" ]]; then
  echo "result directory must be fresh: $result" >&2
  exit 64
fi

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
exp="$root/benchmarks/m9-agentic-local-v1"
bwrap=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
scratch=/tmp/m9-scratch-$trial
target=/tmp/m9-target-$trial
config=/tmp/m9-config-$trial
# Wall-clock cap. A trial that hits it is `loop_incomplete_timeout`, which is
# NOT a code failure -- see preregistration.json.
timeout_seconds=5400

mkdir -p "$result"
rm -rf "$scratch" "$target" "$config"
mkdir -p "$target" "$config"
chmod 700 "$config"

if [[ "$arm" == "skill" ]]; then
  bash "$exp/scripts/make_scratch.sh" "$scratch" --with-skill >/dev/null
else
  bash "$exp/scripts/make_scratch.sh" "$scratch" >/dev/null
fi
# Exact pre-loop state, so the verifier can name every file the agent touched.
(cd "$scratch" && find . -type f -not -path './.git/*' -exec sha256sum {} + \
  | sort -k2) > "$result/pre-loop-manifest.txt"

started=$(date +%s)
set +e
"$bwrap" \
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
  --setenv PATH /home/rizumita/.local/share/mise/installs/claude/latest:/home/rizumita/.cargo/bin:/usr/local/bin:/usr/bin:/bin \
  --setenv HOME /home/rizumita \
  --chdir "$scratch" \
  timeout "$timeout_seconds" claude \
    --print \
    --model qwen3.8:27b-mlx \
    --output-format stream-json \
    --verbose \
    --permission-mode bypassPermissions \
    --allowedTools "Read,Edit,Write,Bash,Grep,Glob,Skill" \
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

(cd "$scratch" && find . -type f -not -path './.git/*' -exec sha256sum {} + \
  | sort -k2) > "$result/post-loop-manifest.txt"
cp -r "$scratch" "$result/worktree"
rm -rf "$config"
echo "trial=$trial arm=$arm status=$status outcome=$(cat "$result/loop-outcome") elapsed=$((finished - started))s"
