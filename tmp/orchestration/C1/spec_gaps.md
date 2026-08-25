# C1 spec gap — v1/v2 ingestion boundary

ADR 0038 §2.1, §3.2, §8.5, and §13 require the legacy v1 extraction/report
state to remain byte-stable and require the v2 contract to use separate
schemas/types.  They do not specify the versioned top-level ingestion
projection or public API that carries both of the following v2-only facts
without adding them to `ProgramSpace.extraction.limitations` or
`reviewgraphen.extraction_report.v1`:

1. closed `reviewgraphen.ingestion_obstruction.v2` located call occurrences;
2. the distinct global `direct_calls` limitation whose macro latent occurrence
   count is `unknown`.

The exact closed occurrence record requires call kind/reason/span/source IDs,
whereas the required global limitation must not fabricate a located call
occurrence.  The ADR does not state whether they share a discriminated v2
ingestion report, are separate versioned projections, or are introduced via a
new extractor-major input contract.  Choosing a new top-level schema ID,
union, `IngestResult` field, or extractor-major boundary would be a design
decision outside C1's implementation authority.

No B1/B3/S3 changes were made after recording this gap, because their correct
implementation depends on the selected v2 projection and its closed semantic
validator.
