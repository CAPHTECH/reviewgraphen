# M7 HEAD v1

This additive benchmark measures mechanically verified bug yield on the frozen
FSL HEAD. It does not reuse known-fix units as the production corpus and does
not claim recall.

The required order is:

1. freeze the test/fix-generator contract;
2. calibrate it on all twenty `m7-real-v1` positive parents;
3. stop if fewer than nine generated tests satisfy the canonical-fix oracle;
4. freeze complete HEAD production-file ownership;
5. run B1, G3-proxy, and full ReviewGraphen over identical source bytes;
6. mechanically verify every linked candidate;
7. classify executable `.fsl` specification support; and
8. report verified yield, precision, overlap, and model/mechanical disagreement.

All model records and proposed patches are non-authority research artifacts.
The FSL source repository is read-only; verification uses disposable clones and
never publishes upstream state.
