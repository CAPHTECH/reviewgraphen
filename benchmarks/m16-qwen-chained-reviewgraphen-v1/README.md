# m16 Qwen chained ReviewGraphen-use probe

m16 follows m15 by moving sequential expansion from a prompt preference into
tool-controlled external state.  Every successful Projection returns an opaque
token required for exactly the next expansion.  Calls batched before observing
the preceding result therefore cannot succeed.

The benchmark reuses m15's frozen overview/source cards and report contract.

