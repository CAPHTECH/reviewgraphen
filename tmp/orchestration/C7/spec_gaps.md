# C7c specification gap: arm-neutral packet task binding

ADR 0038 §8.2 requires every `arm-neutral.source-grounded-packet@2` to carry
an opaque `review-task:sha256:...` ID derived from the fixed m20 preimage
`{comparison_contract, task_contract, unit_id}`.  It also requires the
corresponding hidden task binding to retain the m20 comparison/unit IDs and
arm-specific D closure, while prohibiting those values from the visible packet.

The current `reviewgraphen.generic_review_request.v2` and
`GenericReviewRunV2` contain neither an m20 comparison/unit ID nor a
pre-derived `review-task` ID or hidden binding.  Deriving a task ID from an
execution ID, obligation, context, source inventory, repository identity, or
Git closure would violate §8.2's required preimage and/or its explicit
prohibition on source/obligation identity entering that preimage.  Inventing a
CLI-side wrapper or adding an unreviewed request field would likewise make C9
the packet-contract author, the alternative C9 correctly rejected.

The frozen m20 evaluator specification independently says only its pipeline
constructs this packet and accepts no caller-supplied packet, so it does not
provide a runtime construction input for the quickstart route.

Required resolution: define one of (a) a versioned v2 request input carrying a
validated opaque task binding supplied by an owning evaluator, or (b) a
separate non-m20 quickstart packet contract with a distinct schema/name and
explicit task-ID derivation.  Until then C7 cannot honestly expose the
requested arm-neutral packet builder or deterministic observer output bound to
such a packet.
