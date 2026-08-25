# C2 D source-registration handoff

## Chosen API

```rust
impl ReviewAggregate {
    pub fn with_read_only_d_snapshot_sources(
        self,
        run_id: StableId,
        registrations: Vec<ArtifactRegistered>,
        sources: SnapshotSourcesRecorded,
    ) -> Result<ReviewAggregate>;
}
```

This is a consuming builder for the aggregate returned by
`read_only_from_d_two_layer_bundle`. It admits only a pristine D two-layer
aggregate, requires the supplied registration IDs to exactly equal the IDs in
the source record, and reuses the core `record_snapshot_sources` closure
validation. The resulting aggregate contains no event log, plan, claim,
decision, verification, or execution state; therefore it cannot mint or append
an authority-bearing event.

## Alternative rejected

A D-specific `EventLog` constructor would add a second event permission state
and duplicate event admission/replay boundaries in the stack-sensitive event
carrier. The aggregate builder keeps registration metadata outside authority
events while retaining the existing exact closure validation, so it has the
smaller authority and memory surface.
