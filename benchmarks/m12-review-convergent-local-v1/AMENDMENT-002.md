# Amendment 002 — Codex structured-output schema compatibility

The first Codex judge attempt reached the provider but was rejected before
generation with `invalid_json_schema`: the current Codex structured-output
subset does not permit `uniqueItems` on `duplicate_of`.  No judgment content
was generated.

For the replacement attempt, `judge/output-schema.codex-compatible.json` is
semantically identical to the frozen m7 output schema, with the unsupported
provider-level `uniqueItems: true` keyword omitted and explicit `type: string`
added beside the existing string `const`/`enum` constraints as required by the
current provider subset.  The semantic rule is unchanged:
duplicate IDs, self references, missing judgments, and extra judgments are
checked deterministically after the call.  This amendment changes transport
compatibility only; it does not change any disposition, quality field, finding,
source, model, effort, or judge instruction.  Both rejected attempts failed
provider schema validation before any judgment content was generated.
