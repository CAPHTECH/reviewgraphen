#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
CORPUS="$ROOT/benchmarks/m7-pilot-v2"
TMP_ROOT="$(mktemp -d /tmp/reviewgraphen-m7-v2-verify.XXXXXX)"
trap 'rm -rf "$TMP_ROOT"' EXIT

mkdir -p "$TMP_ROOT/compile"
for revision in "$CORPUS"/public/*/{base,head}; do
  unit="$(basename "$(dirname "$revision")")"
  kind="$(basename "$revision")"
  staged="$TMP_ROOT/compile/$unit-$kind"
  cp -R "$revision" "$staged"
  cargo check --quiet --manifest-path "$staged/Cargo.toml"
  cargo fmt --check --manifest-path "$staged/Cargo.toml"
done

cargo run --quiet -p reviewgraphen-benchmark -- prepare-pilot \
  "$CORPUS/public" "$TMP_ROOT/first" "$CORPUS/execution-config.json" 1 >"$TMP_ROOT/first.log"
cargo run --quiet -p reviewgraphen-benchmark -- prepare-pilot \
  "$CORPUS/public" "$TMP_ROOT/second" "$CORPUS/execution-config.json" 1 >"$TMP_ROOT/second.log"
diff -r "$TMP_ROOT/first" "$TMP_ROOT/second"

python3 - "$CORPUS" "$TMP_ROOT/first" <<'PYVERIFY'
from pathlib import Path
import hashlib, json, sys
corpus=Path(sys.argv[1]); prepared=Path(sys.argv[2])
commitments=json.loads((corpus/'commitments.json').read_text())
assert commitments['schema']=='reviewgraphen.benchmark.corpus_commitments.v1'
assert len(commitments['units'])==12
units={entry['unit_id'].split(':',1)[1]:entry for entry in commitments['units']}
assert len(units)==12
pairs=json.loads((corpus/'private/pairs.json').read_text())
assert pairs['schema']=='reviewgraphen.benchmark.corpus_pairs.v1'
assert len(pairs['pairs'])==6
positive={p['positive_unit'].split(':',1)[1] for p in pairs['pairs']}
controls={p['control_unit'].split(':',1)[1] for p in pairs['pairs']}
assert len(positive)==len(controls)==6 and not positive & controls and positive|controls==set(units)
closed={'check_write_gap','cross_file_contract','cross_file_effect','repeatable_entry','repeated_effect','state_transition_gap','unstable_scope_key','unstable_token'}
assert all(set(p['mechanism_ids']) <= closed and p['mechanism_ids'] for p in pairs['pairs'])
for path in (corpus/'public').rglob('*'):
    assert not path.is_symlink(), path
for unit, commitment in units.items():
    b1=json.loads((prepared/unit/'b1/replicate-1/manifest.json').read_text())
    g3=json.loads((prepared/unit/'g3_proxy/replicate-1/manifest.json').read_text())
    assert b1['expected_packet_ids']==[]
    assert g3['expected_packet_ids'] and len(g3['expected_packet_ids'])==len(set(g3['expected_packet_ids']))
    assert b1['input_tree_hash']==g3['input_tree_hash']==commitment['input_tree_hash']
    assert b1['source_bundle_hash']==g3['source_bundle_hash']==commitment['source_bundle_hash']
    assert b1['source_inventory']==g3['source_inventory']==commitment['source_inventory']
    assert b1['paired_configuration_hash']==g3['paired_configuration_hash']
    oracle=json.loads((corpus/'private'/f'{unit}.json').read_text())
    assert oracle['unit_id']==f'unit:{unit}'
    assert oracle['input_tree_hash']==b1['input_tree_hash']
    assert oracle['source_bundle_hash']==b1['source_bundle_hash']
    assert bool(oracle['roots'])==(unit in positive)
    assert len(oracle['roots'])==(1 if unit in positive else 0)
    inventory={x['path']:x for x in b1['source_inventory']}
    for root in oracle['roots']:
        assert set(root['mechanism_tags']) <= closed and root['mechanism_tags']
        assert root['tree_hash']==b1['input_tree_hash']
        entry=inventory[root['path']]
        assert root['file_sha256']==entry['content_hash']
        source=(corpus/'public'/unit/'head'/root['path']).read_bytes()
        assert 'sha256:'+hashlib.sha256(source).hexdigest()==entry['content_hash']
        lines=source.splitlines(keepends=True)
        assert 1 <= root['start_line'] <= root['end_line'] <= len(lines)==entry['line_count']
        span=b''.join(lines[root['start_line']-1:root['end_line']])
        assert 'sha256:'+hashlib.sha256(span).hexdigest()==root['span_sha256']
assert len(list(prepared.glob('*/b1/replicate-1/manifest.json')))==12
assert len(list(prepared.glob('*/g3_proxy/replicate-1/manifest.json')))==12
assert len(json.loads((prepared/'inventory.json').read_text())['trials'])==24
PYVERIFY

for unit in "$CORPUS"/public/*; do
  name="$(basename "$unit")"
  manifest="$TMP_ROOT/first/$name/b1/replicate-1/manifest.json"
  oracle="$CORPUS/private/$name.json"
  trial_id="$(jq -r .trial_id "$manifest")"
  cat >"$TMP_ROOT/candidate-$name.json" <<JSON
{"schema":"reviewgraphen.benchmark.candidate_output.v1","trial_id":"$trial_id","outcome":"structured","findings":[],"obligation_results":[]}
JSON
  cargo run --quiet -p reviewgraphen-benchmark -- score "$manifest" "$TMP_ROOT/candidate-$name.json" "$oracle" >/dev/null
done

echo "m7-pilot-v2 verified: 12 units, 24 revisions, 24 deterministic trials"
