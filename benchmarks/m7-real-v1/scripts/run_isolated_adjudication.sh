#!/usr/bin/env bash
set -euo pipefail

if [[ ${1-} == --all ]]; then
  jobs=${2:-3}
  if [[ ! "$jobs" =~ ^[1-3]$ ]]; then
    echo "batch concurrency must be between 1 and 3" >&2
    exit 64
  fi
  mapfile -t batch_dirs < <(
    find /tmp/m7-real-adjudication-input -mindepth 1 -maxdepth 1 -type d -name 'batch-*' -print | sort
  )
  active=0
  failures=0
  for batch_dir in "${batch_dirs[@]}"; do
    result_dir=/tmp/m7-real-adjudication-runs/$(basename "$batch_dir")
    if [[ -s "$result_dir/decisions.raw.json" && -f "$result_dir/exit-status" ]]; then
      read -r prior_status < "$result_dir/exit-status" || true
      if [[ ${prior_status-} == 0 ]]; then
        continue
      fi
    fi
    bash "$0" "$batch_dir" "$result_dir" &
    ((active += 1))
    if (( active >= jobs )); then
      if ! wait -n; then ((failures += 1)); fi
      ((active -= 1))
    fi
  done
  while (( active > 0 )); do
    if ! wait -n; then ((failures += 1)); fi
    ((active -= 1))
  done
  if (( failures > 0 )); then
    echo "$failures adjudication process(es) exited nonzero" >&2
    exit 1
  fi
  exit 0
fi

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <batch-dir> <result-dir>" >&2
  exit 64
fi

batch_dir=$(realpath -e -- "$1")
result_arg=$2
if [[ ! "$batch_dir" =~ ^/tmp/m7-real-adjudication-input/batch-[0-9][0-9]$ ]]; then
  echo "refusing batch directory outside fixed adjudication input" >&2
  exit 64
fi
if [[ "$result_arg" != /tmp/m7-real-adjudication-runs/* ]]; then
  echo "refusing result directory outside fixed adjudication output" >&2
  exit 64
fi
if [[ -n $(find "$batch_dir" -type l -print -quit) ]]; then
  echo "refusing symlinked adjudication input" >&2
  exit 66
fi

mkdir -p -- "$result_arg"
result_dir=$(realpath -e -- "$result_arg")
session_dir=$(mktemp -d /tmp/m7-real-adjudication-session.XXXXXX)
codex_home=$(mktemp -d /tmp/m7-real-adjudication-home.XXXXXX)
cleanup() { rm -rf -- "$session_dir" "$codex_home"; }
trap cleanup EXIT INT TERM
: > "$session_dir/decisions.json"
for auth_file in auth.json installation_id models_cache.json; do
  if [[ -f "/home/rizumita/.codex/$auth_file" ]]; then
    cp -a -- "/home/rizumita/.codex/$auth_file" "$codex_home/$auth_file"
  fi
done

bwrap_bin=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
codex_bin=/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex
code_host_bin=/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex-code-mode-host
set +e
"$bwrap_bin" \
  --die-with-parent --unshare-pid --unshare-ipc --unshare-uts \
  --proc /proc --dev /dev \
  --ro-bind /usr /usr --ro-bind /bin /bin --ro-bind /lib /lib --ro-bind /lib64 /lib64 --ro-bind /etc /etc \
  --dir /run --dir /run/systemd --dir /run/systemd/resolve \
  --ro-bind /run/systemd/resolve/stub-resolv.conf /run/systemd/resolve/stub-resolv.conf \
  --dir /home --dir /home/codex --bind "$codex_home" /home/codex/.codex \
  --ro-bind "$codex_bin" /codex --ro-bind "$code_host_bin" /codex-code-mode-host \
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
    -o /workspace/decisions.json \
    'Read only input/. Follow input/instructions.txt and independently adjudicate every item. Return only the required compact JSON object. Do not access any path outside /workspace.' \
    >"$result_dir/events.jsonl" 2>"$result_dir/stderr.log"
status=$?
set -e
cp -a -- "$session_dir/decisions.json" "$result_dir/decisions.raw.json"
printf '%s\n' "$status" > "$result_dir/exit-status"
exit "$status"
