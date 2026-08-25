# G5 specification gaps

EVALUATOR_SPEC.md §12 requires the frozen runtime record to be included in the
`evaluator_bundle_sha256` preimage. This is implemented unchanged, but it makes
the bundle hash depend on the executing Python/runtime platform and conflicts
with byte-identical third-party reproduction across machines. Escalated for
design resolution; no behavioral change was made here.
