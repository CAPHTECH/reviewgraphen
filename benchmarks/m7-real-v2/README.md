# M7 discriminating real-regression benchmark v2

This additive benchmark is preregistered by ADR 0031. It replaces neither the
`m7-real-v1` corpus nor any prior result. Its purpose is to measure:

1. whether ReviewGraphen scaffolding improves known-regression detection; and
2. whether findings omitted by the scaffold are noise or valid missed defects.

The required order is calibration, exact paired-McNemar power analysis,
untouched-holdout selection, corpus construction, three-arm execution,
defect-level blind adjudication, and reporting. Each completed stage is written
under this directory before the next stage begins.

`preregistration.json` is frozen before candidate enumeration or any new model
trial. Calibration units and all prior M7 real units are forbidden from the
final corpus.
