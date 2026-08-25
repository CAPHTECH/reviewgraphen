# G3 specification gaps

1. EVALUATOR_SPEC.md §12 requires writing four freeze hashes and a manifest
   path to `preregistration.json`, while the delegation instruction declares
   that pre-existing m20 files (including that file) are read-only. This task
   therefore leaves the preregistration file untouched and implements only the
   evaluator bundle and its freeze manifest.
2. Several closed JSON schema field lists (notably command request envelopes)
   are specified semantically but not enumerated as complete JSON Schemas.
   The implementation uses its closed typed decoders as the normative command
   boundary and ships matching closed JSON-Schema descriptors for interchange.
