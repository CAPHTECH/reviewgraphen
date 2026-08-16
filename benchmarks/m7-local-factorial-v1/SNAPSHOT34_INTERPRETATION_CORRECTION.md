# Snapshot-34 interpretation correction

This additive note does not modify the frozen v1 observation. The v1
snapshot-34 B1 attempt ran for 4,458.289 seconds and produced no materialized
final response. An earlier follow-up attributed that observation to an MLX
runner hang after server instability was identified.

Later v2 raw capture of an authorized rerun of the same snapshot and B1 input
showed sustained reasoning-only generation: 12,898
`response.reasoning_summary_text.delta` events and no output-text event through
sequence 12,900 before the diagnostic was intentionally stopped. This is
direct evidence for the rerun and a mechanism consistent with the v1 empty
final. The v1 raw stream was not retained, so the v1 cause cannot be proven
byte-for-byte after the fact. Its interpretation is corrected from
`server_hang` to `reasoning_runaway_consistent_but_historical_raw_unavailable`.
