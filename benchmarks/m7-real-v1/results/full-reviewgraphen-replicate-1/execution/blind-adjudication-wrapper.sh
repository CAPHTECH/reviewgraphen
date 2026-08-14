#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <batch-dir> <result-dir>" >&2
  exit 64
fi

batch_dir=$(realpath -e -- "$1")
result_arg=$2
if [[ ! "$batch_dir" =~ ^/tmp/m7-real-full-adjudication-input-v3/batch-[0-9][0-9]$ ]]; then
  echo "refusing batch directory outside fixed blind input" >&2
  exit 64
fi
if [[ "$result_arg" != /tmp/m7-real-full-adjudication-runs/* || -e "$result_arg" ]]; then
  echo "refusing invalid or existing result directory" >&2
  exit 64
fi
if [[ -n $(find "$batch_dir" -type l -print -quit) ]]; then
  echo "refusing symlinked input" >&2
  exit 66
fi

mkdir -p -- "$result_arg"
result_dir=$(realpath -e -- "$result_arg")
session_dir=$(mktemp -d /tmp/m7-real-full-adjudication-session.XXXXXX)
codex_home=$(mktemp -d /tmp/m7-real-full-adjudication-home.XXXXXX)
cleanup() { rm -rf -- "$session_dir" "$codex_home"; }
trap cleanup EXIT INT TERM
: > "$session_dir/decisions.json"
for auth_file in auth.json installation_id models_cache.json; do
  if [[ -f "/home/rizumita/.codex/$auth_file" ]]; then
    cp -a -- "/home/rizumita/.codex/$auth_file" "$codex_home/$auth_file"
  fi
done

prompt="$session_dir/prompt.txt"
{
  printf '%s\n' 'Independently adjudicate every item from the bounded, role-blind material below.'
  printf '%s\n' 'Return only the JSON object required by decision-output.schema.json.'
  while IFS= read -r relative; do
    printf '\n===== %s =====\n' "$relative"
    sed -n '1,2000p' "$batch_dir/$relative"
  done < <(cd "$batch_dir" && find . -type f ! -name decision-output.schema.json -printf '%P\n' | LC_ALL=C sort)
} > "$prompt"

bwrap_bin=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
codex_bin=/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex
set +e
"$bwrap_bin" \
  --die-with-parent --unshare-pid --unshare-ipc --unshare-uts \
  --proc /proc --dev /dev \
  --ro-bind /usr /usr --ro-bind /bin /bin --ro-bind /lib /lib --ro-bind /lib64 /lib64 --ro-bind /etc /etc \
  --dir /run --dir /run/systemd --dir /run/systemd/resolve \
  --ro-bind /run/systemd/resolve/stub-resolv.conf /run/systemd/resolve/stub-resolv.conf \
  --dir /home --dir /home/codex --bind "$codex_home" /home/codex/.codex \
  --ro-bind "$codex_bin" /codex \
  --dir /workspace --ro-bind "$batch_dir" /workspace/input \
  --bind "$session_dir/decisions.json" /workspace/decisions.json \
  --tmpfs /tmp --chdir /workspace \
  --setenv HOME /home/codex --setenv CODEX_HOME /home/codex/.codex \
  /codex exec \
    --dangerously-bypass-approvals-and-sandbox \
    --dangerously-bypass-hook-trust --ignore-user-config --ignore-rules \
    --ephemeral --skip-git-repo-check \
    -m gpt-5.6-sol -c model_reasoning_effort=\"high\" \
    --output-schema /workspace/input/decision-output.schema.json \
    -o /workspace/decisions.json - \
    < "$prompt" >"$result_dir/events.jsonl" 2>"$result_dir/stderr.log"
status=$?
set -e
cp -a -- "$session_dir/decisions.json" "$result_dir/decisions.raw.json"
printf '%s\n' "$status" > "$result_dir/exit-status"
exit "$status"
