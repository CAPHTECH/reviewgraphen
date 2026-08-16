# Reasoning control probe result

The fresh replacement-2 probe reached the provider after server recovery.
`model_reasoning_effort=none` was mechanically observed in all four retained
Responses requests:

- every `response.created.response.reasoning.effort` was `none`;
- reasoning delta event count was zero in every request;
- provider `output_tokens_details.reasoning_tokens` was zero in every request;
- requests 2, 3, and 4 emitted output-text deltas and completed normally;
- request 1 completed with a function call and no output-text delta.

This confirms that the profile/Codex path suppresses reasoning for this model.
No reasoning-token cap was needed for this probe. The overall adapter trial is
still `protocol_invalid`: Codex produced an item type (`todo_list`) rejected by
the adapter, and the wrapper observed four shaped requests rather than the
single-request contract. That failure is separate from reasoning suppression;
no candidate score is assigned.

The provider streams and their event inspections are retained below. The
pilot's schema/protocol gate is therefore not passed by this one probe, and
production remains blocked until a separately preregistered schema-conformance
pilot succeeds. This result does not justify silently normalizing or relaxing
the candidate protocol.
