# m7-head-local-v1 unit selection

Status: frozen before any generation or judge call. The exact result of this
algorithm is pinned in `units.json`. This document explains and justifies the
algorithm; `units.json` is the authoritative data.

## Target and revision

`/home/rizumita/github/fsl` is read-only for this entire experiment: no
issue, branch, commit, patch, or pull request is ever created against it,
and no sandboxed process is ever given write access to the real checkout
(source content is extracted by reading file bytes and copied into each
packet; the live repository is never bind-mounted into a reviewer or judge
sandbox). The revision is frozen at HEAD `e589014d1655b1224f5b83a7f2de99532a0dcdba`
(`fix(domain): apply declared evolves when a saga step emits its event
(#779) (#789)`) as observed at preregistration time. If the real `fsl`
repository moves before execution, execution still targets this exact pinned
commit (`git show <commit>:<path>`), not whatever HEAD has become by then.

## Why crates alone don't work as units

`fsl`'s Rust workspace has 11 crates. Their `src/` sizes range from 12,662
bytes (`fsl-solver`) to 1,164,401 bytes (`fslc`), a roughly 92x spread. A
94,358-byte admitted input (source + ReviewGraphen scaffold) was the largest
input that reached valid final content anywhere in this benchmark family so
far (`m7-local-factorial-v3`, snapshot-06 full); several multi-hundred-
kilobyte and larger inputs have failed to reach final content for reasons
this program has not been able to attribute cleanly to size alone (the
server administrator's "200k input tokens is a danger zone" claim was
explicitly withdrawn in `benchmarks/m7-local-factorial-v3/LM_STUDIO_TRANSITION.md`).
Given the already-documented low completion rate at this scale, one crate
per unit would put roughly half the crates far outside any size range this
program has evidence can complete, while badly under-using the two or three
smallest crates.

## Algorithm

1. Enumerate every `*.rs` file under `rust/*/src/`, excluding `rust/spikes/`
   (an explicitly experimental crate group, not part of the maintained
   production surface) and excluding any `tests/` subdirectory. This yields
   118 files.
2. Exclude any single file whose byte size alone exceeds the 120,000-byte
   packet cap (§ below), because splitting a file mid-body would break
   review coherence and no packet could admit it whole. Four files are
   excluded this way and listed verbatim in `units.json` `oversized_excluded`,
   each with its exact byte count and SHA-256: `fsl-core/src/dialect.rs`,
   `fsl-core/src/domain_lowering.rs`, `fsl-runtime/src/lib.rs`, and
   `fslc/src/main.rs` (630,350 bytes — by far the largest single file in the
   workspace, evidently the `fslc` CLI's argument-dispatch entry point).
   This mirrors the existing "G3-proxy" exclusion precedent in
   `m7-local-factorial-v2/preregistration.json` §`g3_exclusion`: an explicit,
   disclosed scope exclusion rather than a silent truncation. 114 files
   remain eligible.
3. Sort the 114 eligible files lexicographically by their repository-relative
   path. Because crate directory names themselves sort together, this
   produces one global, deterministic file order that mostly (not always —
   see packet 6 in `units.json`, which spans `fsl-lsp` → `fsl-runtime` →
   `fsl-solver-z3`) tracks crate boundaries.
4. Walk the sorted list, greedily accumulating files into the current
   **packet** while its running byte total stays at or under 120,000 bytes;
   when the next file would exceed the cap, close the packet and start a new
   one with that file. This is a full partition of the 114 eligible files —
   every eligible file is assigned to exactly one packet, matching ADR
   0035's "primary ownership... exactly one deterministic size-bounded
   packet" principle — even though only a subset of the resulting packets is
   selected as review units for this pilot (§ below). This run of the
   algorithm produces 31 packets, from 59,681 to 119,891 bytes each.
5. **120,000-byte cap justification**: chosen before generation, grounded
   only in prior (non-m7-head-local-v1) observations — the largest admitted
   input in this benchmark family known to have reached valid final content
   is 94,358 bytes (`m7-local-factorial-v3` snapshot-06 full). 120,000 bytes
   is a round number modestly above that, leaving headroom for this
   experiment's own ReviewGraphen scaffold overhead on the `qwen_full` arm
   without approaching sizes this program has only ever seen fail to
   complete (867,861+ bytes) or the 262,144-token context ceiling.
6. **Selecting 9 of 31 packets**: rather than reviewing all 31 (far more
   than the operator's 8-10 target, and more than this benchmark's low
   observed completion rate can plausibly sustain within a reasonable
   number of attempts) or the first 9 (which would cluster entirely inside
   `fsl-core`/`fsl-lsp`, the alphabetically-first crates, and never reach
   `fsl-verifier` or `fslc`), packets are chosen by fixed-stride systematic
   sampling across the full ordered packet list:
   `packet_index(i) = floor(i * 31 / 9)` for `i = 0..8`, giving indices
   `[0, 3, 6, 10, 13, 17, 20, 24, 27]`. This is a standard, parameter-free
   sampling rule — it takes no information about packet content, only the
   packet count and the target unit count — and it spreads the 9 selected
   units across nearly the full alphabetical span of the workspace: `fsl-core`
   (units 0-1), `fsl-lsp`/`fsl-runtime`/`fsl-solver-z3` (unit 2),
   `fsl-syntax` (unit 3), `fsl-tools` (units 4-6), `fsl-verifier` (unit 7),
   and `fslc` (unit 8).
7. Each selected packet becomes one review unit, identified as
   `head-local-00` through `head-local-08` in packet-index order. Both arms
   (`qwen_b1`, `qwen_full`) review the exact same file set for a given unit;
   see `preregistration.json`.

## Known limitations of this selection

- 9 of 31 packets (about 29% of eligible files by packet count, less by byte
  weight since packets are similarly sized) are reviewed; the other 22 are
  out of scope for this pilot and are not claimed to be representative of
  them individually. Systematic sampling across the full ordered list is
  intended to avoid systematic bias toward any one crate or alphabetical
  region, but it is not a random sample and carries no formal coverage
  guarantee.
- 4 files (including the single largest file in the workspace) are excluded
  from unit construction entirely by the oversize rule, not reviewed by
  either arm.
- `tests/` directories and inline `#[cfg(test)]` modules within otherwise
  eligible files are not excluded from admitted file bytes (the size counts
  above are whole-file), but files physically located under a `tests/`
  directory are excluded at step 1.
- A packet occasionally spans a crate boundary (unit 2). This is a
  consequence of pure byte-size packing and is disclosed, not treated as an
  error.
