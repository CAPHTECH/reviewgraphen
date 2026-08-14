#!/usr/bin/env bash
set -euo pipefail

if [[ ${1-} == --all ]]; then
  jobs=${2:-3}
  if [[ ! "$jobs" =~ ^[1-3]$ ]]; then
    echo "batch concurrency must be between 1 and 3" >&2
    exit 64
  fi

  mapfile -t trial_dirs < <(
    find /tmp/m7-real-prepared -mindepth 3 -maxdepth 3 -type d -name replicate-1 -print | sort
  )
  active=0
  failures=0
  for batch_trial_dir in "${trial_dirs[@]}"; do
    relative_dir=${batch_trial_dir#/tmp/m7-real-prepared/}
    batch_result_dir=/tmp/m7-real-runs/$relative_dir
    if [[ -s "$batch_result_dir/candidate.raw.json" && -f "$batch_result_dir/exit-status" ]]; then
      read -r prior_status < "$batch_result_dir/exit-status" || true
      if [[ ${prior_status-} == 0 ]]; then
        continue
      fi
    fi
    bash "$0" "$batch_trial_dir" "$batch_result_dir" &
    ((active += 1))
    if (( active >= jobs )); then
      if ! wait -n; then
        ((failures += 1))
      fi
      ((active -= 1))
    fi
  done
  while (( active > 0 )); do
    if ! wait -n; then
      ((failures += 1))
    fi
    ((active -= 1))
  done
  if (( failures > 0 )); then
    echo "$failures trial process(es) exited nonzero" >&2
    exit 1
  fi
  exit 0
fi

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <prepared-trial-dir> <result-dir>" >&2
  exit 64
fi

trial_dir=$(realpath -e -- "$1")
result_dir_arg=$2

if [[ ! "$trial_dir" =~ ^/tmp/m7-real-prepared/snapshot-[0-9][0-9]/(b1|g3_proxy)/replicate-1$ ]]; then
  echo "refusing trial directory outside the fixed prepared corpus: $trial_dir" >&2
  exit 64
fi
if [[ "$result_dir_arg" != /tmp/m7-real-runs/* ]]; then
  echo "refusing result directory outside /tmp/m7-real-runs: $result_dir_arg" >&2
  exit 64
fi
if [[ ! -f "$trial_dir/manifest.json" || ! -d "$trial_dir/agent_input" ]]; then
  echo "trial is missing manifest.json or agent_input/: $trial_dir" >&2
  exit 66
fi
if [[ -n $(find "$trial_dir/manifest.json" "$trial_dir/agent_input" -type l -print -quit) ]]; then
  echo "refusing symlinked trial input: $trial_dir" >&2
  exit 66
fi

mkdir -p -- "$result_dir_arg"
result_dir=$(realpath -e -- "$result_dir_arg")
if [[ "$result_dir" != /tmp/m7-real-runs/* ]]; then
  echo "resolved result directory escaped /tmp/m7-real-runs: $result_dir" >&2
  exit 64
fi

session_dir=$(mktemp -d /tmp/m7-real-session.XXXXXX)
codex_home=$(mktemp -d /tmp/m7-real-codex-home.XXXXXX)
cleanup() {
  rm -rf -- "$session_dir" "$codex_home"
}
trap cleanup EXIT INT TERM

cp -a -- "$trial_dir/manifest.json" "$session_dir/manifest.json"
cp -a -- "$trial_dir/agent_input" "$session_dir/agent_input"
: > "$session_dir/candidate.json"
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
  --die-with-parent \
  --unshare-pid \
  --unshare-ipc \
  --unshare-uts \
  --proc /proc \
  --dev /dev \
  --ro-bind /usr /usr \
  --ro-bind /bin /bin \
  --ro-bind /lib /lib \
  --ro-bind /lib64 /lib64 \
  --ro-bind /etc /etc \
  --dir /run \
  --dir /run/systemd \
  --dir /run/systemd/resolve \
  --ro-bind /run/systemd/resolve/stub-resolv.conf /run/systemd/resolve/stub-resolv.conf \
  --dir /home \
  --dir /home/codex \
  --bind "$codex_home" /home/codex/.codex \
  --ro-bind "$codex_bin" /codex \
  --ro-bind "$code_host_bin" /codex-code-mode-host \
  --dir /workspace \
  --ro-bind "$session_dir/manifest.json" /workspace/manifest.json \
  --ro-bind "$session_dir/agent_input" /workspace/agent_input \
  --bind "$session_dir/candidate.json" /workspace/candidate.json \
  --tmpfs /tmp \
  --chdir /workspace \
  --setenv HOME /home/codex \
  --setenv CODEX_HOME /home/codex/.codex \
  /codex exec \
    --dangerously-bypass-approvals-and-sandbox \
    --dangerously-bypass-hook-trust \
    --ignore-user-config \
    --ignore-rules \
    --ephemeral \
    --skip-git-repo-check \
    -m gpt-5.6-sol \
    -c model_reasoning_effort=\"high\" \
    -o /workspace/candidate.json \
    'Read only manifest.json and agent_input/. Perform the requested blind review. Return only one compact JSON object matching agent_input/candidate-output.schema.json, with no Markdown fence or other text. Copy trial_id exactly from manifest.json. Do not access any path outside /workspace.' \
    >"$result_dir/events.jsonl" \
    2>"$result_dir/stderr.log"
status=$?
set -e

cp -a -- "$session_dir/candidate.json" "$result_dir/candidate.raw.json"
printf '%s\n' "$status" > "$result_dir/exit-status"
exit "$status"
