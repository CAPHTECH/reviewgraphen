# Public-function Node quickstart

This is the separate production-v4 request family. It synthesizes the frozen D
relation rule and `node.public_function_contract@1`; it does not change the
legacy v1–v3 production entry points. The Node arm admits only accepted exact
`pub` free functions with an accepted module `contains` witness.

From an admitted repository root, build the CLI and invoke:

```sh
target/debug/reviewgraphen review --request examples/public-function-node-quickstart/request.v4.json --artifacts public-function-artifacts
```

The fresh output directory contains `audit.run.v4.json`,
`human-report.manifest.v3.json`, `human-report.md`, and an artifact manifest.
The human report remains non-authority. D's candidate-enumeration limitation is
reported only for D; Node coverage has its own profile-included public-free-
function denominator. This example is a versioned request fixture, not a
claim that the repository has a complete Node universe or a safety sign-off.
