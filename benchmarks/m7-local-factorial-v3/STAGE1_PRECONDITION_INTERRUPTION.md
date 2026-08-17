# Stage 1 precondition interruption

The first attempted v3 stage1 call was interrupted before completion. Inspection
of its raw SSE showed reasoning events, revealing that the fixed wrapper had
not exported `M7_V3_REASONING_EFFORT=none`. It is not a v3 result and is not
counted toward the 4/4 gate. The partial raw response remains under the v3 run
root for auditability. The wrapper was corrected before the registered stage1
run.
