# m15 Qwen intelligent ReviewGraphen-use probe

This benchmark tests whether Qwen can use a small, stateful, incremental
ReviewGraphen projection interface while performing a bounded code review.  It
does not front-load a model-authored obligation list.  The reviewer receives a
small target overview, chooses up to five source cards, and must converge on a
grounded structured report.

The benchmark adapter serves frozen projections derived from the exact m11 FSL
snapshot.  It is not represented as the production ReviewGraphen CLI.

