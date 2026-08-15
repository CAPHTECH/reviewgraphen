# Generic reviewer output v2 verification

Measured 2026-08-14 UTC with Codex CLI 0.147.0, GPT-5.6 Sol, high reasoning.

The provider-facing v1 schema could not mechanically express its root-level claim/abstention exclusivity in the provider's Structured Outputs subset. In the prior live run, 3 of 5 responses contained both an abstention and a claim and were correctly rejected as malformed.

V2 keeps the root as an object and places a closed `anyOf` under `result`: either a non-empty `structured` claim list or an `abstained` object. Both branches close unknown properties. The runtime requires canonical v2 JSON and caller-fixed schema/execution identity, then deterministically lowers it into the existing strict v1 parser so property, target, source, and evidence-request closure are unchanged.

The repeated live run executed all five obligations with malformed 0, abstained 5, structured 0, and proposed claims 0. Live and replay canonical run bytes were identical at SHA-256 `504a7ceb8b7418df021610b67ff283414022eb1b9afb979f0463234df4fd28bd`.

This fixed the observed transport failure only. All five obligations were analyzer capability gaps, so the run still did not test or detect a code defect. It remained `non_authority`, `incomplete`, and `trusted_pass=false`.
