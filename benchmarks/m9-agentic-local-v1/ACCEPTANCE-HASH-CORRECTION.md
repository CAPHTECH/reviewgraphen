# Acceptance-test hash correction

Date: 2026-08-20. This is an append-only measurement-record correction; no
trial, verifier output, or result is changed.

The m9 preregistrations name
`47fd74b43cfc1090fd6d662a8b0835709563f378728b598012fa772151fded92` as
the task-2 acceptance-test hash. That is actually m8 task 1's acceptance-test
hash. The task-2 file used by m9's verifier is
`benchmarks/m8-impl-local-v1/task2/m8_extern_block_shadow.rs`, whose SHA-256
is `dd79b76edd5b0bfea46611cde32bdd1ff5ef9bc1d6a94c3a98f0ffd4adf754ef`.

This is a preregistration metadata defect, not evidence that another test ran:
`scripts/verify_trial.py` names and copies the task-2 path, and every retained
verification log records its five extern-block acceptance tests. The original
preregistrations remain unedited. m10 pins the actual task-2 hash.
