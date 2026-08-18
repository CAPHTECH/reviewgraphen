#!/usr/bin/env bash
# Task-2 replication driver, per AMENDMENT-003.md.
#
# Ten trials, strictly alternating treatment/control so an upstream stop
# leaves the arms balanced and any server drift over the ~3-hour window hits
# both arms alike. One request in flight, never concurrent, never retried.
#
# Packet hashes are verified immediately before every request. Any upstream
# failure, or a model-identity mismatch, stops the entire series: the next
# trial is not started and no trial is replaced.
set -uo pipefail

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
exp="$root/benchmarks/m8-impl-local-v1"
runs=/tmp/m8-impl-local-v1-runs
log=/tmp/m8-replication.log

declare -A PACKET=(
  [methodology]=/tmp/m8-t2-packets-v2/packet-methodology.txt
  [baseline]=/tmp/m8-t2-packets-v2/packet-baseline.txt
)
declare -A EXPECT=(
  [methodology]=dd06c8ba09f50db4cbcc75de23e997a38974f66a532da15ca932a57ec6cb2816
  [baseline]=2e4c2fa6ab93c9e22553087076074dfb656ca0db5888a0eb729705d8e51ab7d7
)

export CARGO_TARGET_DIR=/tmp/claude-1000/-home-rizumita-workspace-reviewgraphen/0d42d984-55de-424b-ad9a-e8b8f298b722/scratchpad/cargo-target
export REVIEWGRAPHEN_TRUSTED_CARGO=/home/rizumita/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo

note() { printf '%s %s\n' "$(date -Is)" "$*" | tee -a "$log"; }

completed=0
declare -A DONE=([methodology]=0 [baseline]=0)
note "SERIES START"
for index in 1 2 3 4 5; do
  for arm in methodology baseline; do
    trial="rep-${arm}-${index}"
    packet=${PACKET[$arm]}
    observed=$(sha256sum "$packet" | awk '{print $1}')
    if [[ "$observed" != "${EXPECT[$arm]}" ]]; then
      note "STOP packet hash drift for $arm: $observed"
      exit 65
    fi
    note "START $trial (packet sha256 verified)"
    rm -rf "$runs/$trial"
    python3 "$exp/scripts/run_generation.py" "$packet" "$runs/$trial" \
      > "/tmp/${trial}.log" 2>&1
    status=$?
    if (( status != 0 )); then
      class=$(python3 -c "import json;print(json.load(open('$runs/$trial/generation-metrics.json')).get('failure_class'))" 2>/dev/null)
      note "STOP $trial generation status=$status failure_class=$class"
      note "SERIES STOPPED: completed=$completed treatment=${DONE[methodology]} control=${DONE[baseline]}"
      exit "$status"
    fi
    note "OK $trial generated"
    completed=$((completed+1))
    DONE[$arm]=$(( ${DONE[$arm]} + 1 ))

    scratch="/tmp/m8-rep-scratch-$trial"
    rm -rf "$scratch"
    python3 "$exp/scripts/apply_and_verify_task2.py" "$runs/$trial" "$scratch" \
      > "$runs/$trial/verify.log" 2>&1
    verdict=$(python3 -c "import json;print(json.load(open('$runs/$trial/verification.json'))['verdict'])" 2>/dev/null)
    note "VERIFY $trial verdict=$verdict"

    # Diagnostic probes (AMENDMENT-003 section 4). Never a gate.
    if [[ -d "$scratch" ]]; then
      for probe in m8_foreign_macro_probe m8_foreign_safefn_probe; do
        cp "$exp/task2/$probe.rs" "$scratch/crates/reviewgraphen-ingest/tests/"
        if cargo test --manifest-path "$scratch/Cargo.toml" -p reviewgraphen-ingest \
             --test "$probe" > "$runs/$trial/$probe.log" 2>&1; then
          printf 'pass\n' > "$runs/$trial/$probe.result"
        else
          printf 'fail\n' > "$runs/$trial/$probe.result"
        fi
        note "PROBE $trial $probe=$(cat "$runs/$trial/$probe.result")"
      done
      rm -rf "$scratch"
    fi
  done
done
note "SERIES COMPLETE: 10 trials"
