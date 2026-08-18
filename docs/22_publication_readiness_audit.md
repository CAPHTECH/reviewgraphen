# 22. Publication Readiness Audit

> Status: audit, 2026-08-18. Investigation and documentation only — no
> file was deleted, no history was rewritten, no directory was moved.
> This document does not decide whether or when `CAPHTECH/reviewgraphen`
> goes public; it is the material for that decision.
> Scope note: `crates/` was not touched or audited here — a separate
> agent is doing rule-versioning work there in its own worktree at the
> time of writing. This audit covers everything else: `benchmarks/`,
> `docs/`, `upstream-issues/`, top-level files, and the full git
> history.

This document complements `docs/20_open_source_and_commercial_boundary.md`,
which already sets the intended public/commercial repository boundary
and lists "benchmark/dataset license" as an unresolved item in its
Decision Gate (§12, item 4). This audit supplies the concrete, current-state
facts for that gate item and for the two other investigations the
operator asked for. It does not repeat §20's framework; where useful it
reuses that document's own artifact classification (`public` /
`public-after-redaction` / `customer-owned` / `commercial-confidential`
/ `security-sensitive` / `third-party-restricted`).

---

## 1. Investigation 1 — things that must not become public

### 1.1 `benchmarks/*/private/` — what's actually in them, and why "private"

Five top-level `private/` directories exist (plus nested ones under
`results/`): `m7-head-v1`, `m7-pilot-v1`, `m7-pilot-v2`, `m7-real-v1`,
`m7-real-v2`.

**"Private" here is a benchmark-methodology term, not a confidentiality
classification.** Each benchmark's own README says so directly:
`m7-pilot-v1/README.md:6` — *"`private/` contains the oracle and must
never be included in [the reviewer's input]"*; `m7-pilot-v2/README.md:3`
— *"the checked-in private directory... [is] excluded from reviewer
inputs."* This is **blinding**: the oracle (which lines are the seeded
defect, which commit is the fix, which test proves it) must never reach
the model being evaluated, or the benchmark measures nothing. It is not
about keeping the data secret from humans, and the experiments these
directories belong to are already complete — the blinding purpose these
directories served no longer requires anything to stay hidden.

| Directory | Contents | Size | Publication concern |
| --- | --- | --- | --- |
| `m7-pilot-v1/private/` | Synthetic-corpus oracle: file paths, symbols, line ranges, mechanism tags, severities. No embedded source. | 36K, 8 files | None found. |
| `m7-pilot-v2/private/` | Synthetic-corpus oracle, pair mapping, commitments. No embedded source. | 56K, 13 files | None found. |
| `m7-real-v1/private/` | Real-defect oracle: `fix_commit`/`parent_commit` git SHAs (fsl commit hashes), presence-test commands and **hashes** of test stdout/stderr (not the content), inventory/tree hashes. No embedded fsl source. | 812K, 182 files | See §1.3 (benchmark-contamination risk, distinct from license). |
| `m7-real-v2/private/` | Same shape as m7-real-v1: candidate frames, presence indices, hashes. No embedded source found. | 2.8M, 243 files | Same as above. |
| `m7-head-v1/private/` | Calibration generation records (`record.json` + `provider-output/raw-response.json` per calibration case) — model-generated proposals, some containing small diff-format excerpts of real fsl source as patch context (see §1.2). | 932K, 110 files | Third-party source excerpts — see §1.2. |

**Conclusion for §1.1:** nothing in `private/` is confidential in the
security sense. The only two live concerns these directories raise are
(a) small embedded fsl excerpts in `m7-head-v1/private/calibration/`
(§1.2) and (b) a benchmark-integrity question, not a secrecy one, for
`m7-real-v1`/`m7-real-v2`'s oracle data (§1.3).

### 1.2 Third-party source code — fsl content embedded in this repo

**Two locations, embedded by design** (the benchmark packet construction
deliberately includes source bytes to build review packets):

1. **`benchmarks/m7-real-v1/public/snapshot-*/snapshot/rust/...`** — 80
   `.rs` files across 41 snapshot directories, **~19 MB total**, **full,
   unmodified file copies** extracted via `git show` from specific fsl
   commits, fsl's own `// SPDX-License-Identifier: Apache-2.0` /
   `// Copyright 2026 Ryoichi Izumita` headers intact on all 80 (verified
   by direct count, not sampling).
2. **`benchmarks/m7-head-v1/private/calibration/generation/calibration-*/`**
   — 20 records, 472K total. Each contains a model-generated "fix
   proposal" in unified-diff format; the diff's context lines quote a
   handful of real lines of fsl source (`rust/fslc/src/main.rs` and
   similar) as part of the patch. Smaller in both scale and character
   than §1's full-file copies — excerpts within a diff, not file
   copies — but still verbatim third-party bytes.

No fsl content was found embedded anywhere else in the repository
(`private/` directories other than `m7-head-v1` do not embed source;
confirmed by direct grep, not assumed from the pattern above).

**License facts, checked directly, not assumed:**
- fsl (`/home/rizumita/github/fsl`) is licensed **Apache License 2.0**
  (`LICENSE` file present) and carries a **`NOTICE` file**
  (`FSL — AI-Native Formal Specification Language (fslc)`, `Copyright
  2026 Ryoichi Izumita`).
- Apache-2.0 **permits redistribution**, including embedding excerpts
  and full files in another repository, **provided**: a copy of the
  license is included, copyright/attribution notices in the files
  themselves are preserved (confirmed: all 80 files still carry their
  original header — nothing was stripped), and the `NOTICE` file's
  content is carried forward to the redistribution (Apache-2.0 §4(d)).
- **reviewgraphen currently has no `LICENSE` file at all** (checked
  directly: `ls LICENSE*` at repo root returns nothing), and **no
  `NOTICE` file, and no attribution statement anywhere** (README.md,
  `m7-real-v1/README.md`, and `docs/` were all checked; none mentions
  that fsl source is embedded or under what license).
- **The copyright holder of fsl and the primary author of reviewgraphen
  are the same person.** Every reviewgraphen commit and the fsl `NOTICE`
  file both attribute to `Ryoichi Izumita <r.izumita@caph.jp>` (verified
  via `git log`, not assumed from names alone). This substantially
  reduces the *legal* risk (the copyright holder can authorize their own
  redistribution) but does not, on its own, satisfy Apache-2.0's stated
  terms (NOTICE inclusion) or address the community-facing question in
  §3 — fsl has 24 org members and at least one other code contributor
  besides the operator (see §3).

**What this means for publication:** the embedded fsl content is not a
legal blocker in the sense of "cannot be published" — Apache-2.0
explicitly permits this kind of redistribution, and the same person
holds copyright on both sides. It is, however, an unmet compliance
item already anticipated in `docs/20_open_source_and_commercial_boundary.md`
§5 and §12(4) ("license fileを置くだけでなく、NOTICE、dependency
inventory、source traceをrelease processへ含めます" / "benchmark/dataset
license確認"). **Concretely missing right now:** a `LICENSE` file for
reviewgraphen itself, a `NOTICE` file (or equivalent attribution
section) crediting fsl/Apache-2.0 for the embedded content, and a
one-line note in `m7-real-v1/README.md` stating the embedding and its
license. **Difficulty: low** — these are additive files, no rewrite of
existing commits needed, no deletion required.

### 1.3 A separate, non-license risk: benchmark contamination

Not something the operator asked about directly, but adjacent enough to
§1.1/§1.2 to flag here rather than omit: `m7-real-v1`/`m7-real-v2`'s
oracle data records **which fsl commits are the defect and which are
the fix** for 20+ real historical fsl bugs. Once this repository is
public and crawled, that answer key becomes part of the training-data
surface for any model trained afterward. A future re-run of this exact
benchmark, using a model trained on a post-publication crawl, could no
longer measure "detects a previously unseen defect" for these specific
fsl commits — the labels would already be in the model's training data.
This doesn't block publishing the oracle (it's the researchers' own
derived data, not something fsl restricts), but it is worth recording
as a known limitation on any *future* reuse of these specific 20+ units,
distinct from the current publication decision. `docs/20_open_source_and_commercial_boundary.md`
§9 gestures at "benchmark contamination disclosure" as a general
concern; this is the concrete instance of it.

### 1.4 Credentials, secrets, tokens, internal hosts/IPs, local paths, PII

Full results from an independent, exhaustive scan of the working tree
**and** the complete git history (201 commits, all refs — not just
`HEAD`):

**Credentials/secrets: none found, anywhere, ever.** Searched the full
`git log -p --all` (947k lines) for AWS-key, GitHub-token,
OpenAI/Anthropic-key, PEM-block, and bearer-token patterns — zero
matches. No `.env`, `*credentials*`, `.pem`, or `.key` file was ever
committed at any point in history (checked via full-history path
search, not just the current tree). **Git history has zero deleted
files, total** — nothing has ever been removed from this repository's
history, so there is no "committed-then-scrubbed" secret to hunt for
separately. Every `OLLAMA_PRIV_API_KEY` occurrence is the literal known
dummy string `ollama`; every `.credentials.json` mention is a path
reference in a script, never the file's actual content.

**Internal hosts/IPs — real, low severity, working-tree only:**

| Value | What it is | Scope |
| --- | --- | --- |
| `192.168.68.71` | Private LAN IP (RFC1918) of the LM Studio benchmark server | 22 files in the current working tree (34 occurrences across history, but present at every point — not something that was ever removed) |
| `mac-studio.local:11434` | mDNS hostname, same physical machine | several files, `m7-local-factorial-*` docs |
| `127.0.0.1` | Loopback (local request-shaper proxy) | 33 occurrences — benign |

Not internet-routable, no remote-access risk. Reveals the operator's
home-network layout, nothing more. **Redaction difficulty: low** — a
normal find-and-replace commit; no history rewrite needed since these
values have always been present (nothing to selectively scrub from old
commits without touching every commit that ever mentioned them, and
since it's low-severity, that trade-off likely isn't worth it — see
recommendation in §6).

**Local environment paths — real, extensive, low severity:** `/home/rizumita/...`
appears in 43 working-tree files; `/Users/rizumita/Workspace/reviewgraphen`
in onboarding docs. Both are the operator's own path, already tied to
their public git identity. `/home/codex`, `/home/reviewer` are **not**
real host paths — they are sandbox-internal paths inside the `bwrap`
isolation the process-reviewer adapter constructs; benign.
`/Users/evil` is a deliberately-named adversarial test fixture, not a
real leaked path.

**Personal information beyond normal authorship: none.** Every one of
201 commits has exactly one author, `Ryoichi Izumita <r.izumita@caph.jp>`
— already the repository's known, public identity, not a finding. No
other person's name or email appears anywhere in tracked content;
apparent email-like matches are all RFC 2606/6761 reserved fake domains
(`*@example.invalid`) used as test fixtures.

**Bottom line for §1.4: nothing found requires a git history rewrite.**
Every real finding (one internal IP, one internal hostname, extensive
local paths) is redactable with an ordinary commit against the current
tree. Severity is low across the board.

---

## 2. Investigation 2 — does the capability-gap record read clearly?

The diagnosis documents themselves are well-calibrated: precise
file/line citations, explicit "confirmed" vs. "expected but not
directly observed" distinctions (`CAPABILITY_GAP_DIAGNOSIS.md` states
plainly that some inferences weren't directly re-checked), and credit
given where due — `docs/measurement-validity-obligation-synthesis-capability-gap.md`
states the synthesis code itself "works correctly and is tested
extensively," and calls ADR 0011's capability-trace design "intentional,
mature design." **No overclaiming and no unnecessary self-flagellation
was found in the diagnosis documents themselves.** The three addenda
(`m7-real-v1/results/full-reviewgraphen-replicate-2/REPORT.md`,
`m7-pilot-v2/README.md`, `m7-local-factorial-v2/README.md`) are all
dated, explicitly labeled "appended, not a rewrite," placed where a
reader will find them without hunting, and each quotes the exact
original phrase it corrects — reading the original and the addendum
together is straightforward in all three cases.

**Two real problems found, both outside the diagnosis documents
themselves:**

1. **Top-level `README.md` overclaims by omission.** It presents
   obligation synthesis as a normal, working pipeline stage with no
   mention of the capability-gap finding and no pointer to the
   diagnosis document. Its own "文書の性格" section additionally states
   *"本ドキュメント群は実装前の設計基準です...ReviewGraphen自体の実装が存在することを意味しません"*
   ("this document set is a pre-implementation design standard...it
   does not mean an implementation of ReviewGraphen exists") — which is
   now false; a substantial implementation exists. This reads like an
   unrevised early-milestone snapshot, not something written with the
   capability-gap finding in mind. `HANDOFF.md`'s phase-status section
   is similarly stale and disconnected from the finding.

2. **No document anywhere gives an accurate "as of today" summary of
   what the system can and cannot do.** Checked every diagnosis
   document, every addendum, and both top-level entry points
   (`README.md`, `HANDOFF.md`) — none serves this purpose. A reader has
   to assemble the current picture from at least six scattered files.
   **This is the single most important gap for publication, exactly as
   the operator suspected before this audit ran.**

**New complication, not yet reflected in any file:** the operator has
stated that the obligation-synthesis defect was **fixed today**, by a
separate agent working in `crates/` in its own worktree (out of this
audit's scope). Every diagnosis document is written in present tense as
of 2026-08-17 — *"Status: diagnosis, no fix implemented"*
(measurement-validity doc), *"Is ReviewGraphen currently able to
generate substantive review obligations for fsl? Essentially no"*
(`CAPABILITY_GAP_DIAGNOSIS.md`). **Per this repository's own append-only
discipline, none of these should be rewritten** — but as they stand
today, every one of them will read as **currently true when it no
longer is**, with nothing telling a reader that this changed, or when.
The fix does not retroactively correct these documents' framing; it
makes the *absence* of a current-status summary more costly than it was
yesterday.

---

## 3. Investigation 3 — third-party impact of `upstream-issues/`

The 5 issues (#817, #818, #820, #821, #822) are already public on
`ymm-oss/fsl` — publishing `reviewgraphen` does not newly expose their
content. What changes is that `reviewgraphen` would additionally host a
**dated, methodical record of having searched fsl systematically for
defects and filed issues about them**, distinct from the issues
existing on their own. Considerations, not a decision:

- **Reduces concern:** the operator holds admin access to `ymm-oss/fsl`
  and is fsl's dominant contributor (671 of ~702 tracked commits in a
  quick contributor check) and its `NOTICE`-file copyright holder. This
  reads far more like a maintainer dogfooding their own review tool on
  their own project than an outsider unilaterally auditing someone
  else's code without relationship — which is generally well-received
  in open source, not adversarial.
- **Doesn't fully resolve it:** `ymm-oss` has roughly two dozen org
  members and at least one other code contributor besides the operator
  (checked directly via the GitHub API, not assumed) — fsl is not a
  solo project even though the operator dominates it. Those other
  stakeholders did not individually consent to being the subject of a
  published case study, even a well-conducted one. `upstream-issues/`'s
  current framing (a general-purpose, repeatable procedure — clone,
  verify, draft, post, record) doesn't make the operator's own
  maintainer relationship to fsl visible to an outside reader; someone
  encountering it cold could reasonably read it as "here is how this
  tool searches arbitrary public repositories for defects and files
  issues," without the context that narrows who it was actually run
  against and why that made the exercise low-risk.
- **Framing risk differs by publication purpose (see §4):** as a
  *research record*, full methodological transparency (verification
  before filing, explicit non-claim of "verified" where only
  code-reading was done, disclosed AI authorship) is exactly what such
  a record should show, and reads as rigor. As a *product* demo,
  "reviewgraphen found 5 real bugs in a real codebase" is a strong
  claim, but a skeptical reader could note the demonstration repo is
  one the operator already deeply understands and administers — a
  different, weaker claim than showing the same process working against
  a codebase with no prior relationship. That gap matters more for a
  product pitch than for a research record.
- **A precedent question, not a fact:** publishing this as a *reusable,
  documented methodology* (not just as one completed instance) could be
  read as inviting the same procedure against other repositories with
  less care applied than was actually used here — automated,
  AI-assisted issue-filing against public repositories has drawn
  negative community reaction in other contexts when done without this
  level of verification. Nothing here suggests future runs would be
  less careful, but the document as written doesn't state that the
  verification discipline (isolated clone, mechanical reproduction
  where possible, explicit uncertainty disclosure, per-issue human
  approval) is a **requirement**, not just what happened to occur this
  time.

No recommendation is made here on whether to publish, keep private, or
restructure `upstream-issues/` — per the operator's instruction, that
decision is the operator's and the user's to make together.

---

## 4. Research-record vs. product framing — where the judgment differs

| Question | As a research record | As a usable product |
| --- | --- | --- |
| Capability-gap history (§2) | An asset — this level of self-caught, dated, evidenced measurement-validity correction is unusual and credible. Publish as-is once an as-of-today summary exists. | A liability if not paired with a current-status summary — a prospective user skimming for "does this work" could stop at an old diagnosis doc and conclude it doesn't, when it now does. The as-of-today summary is not optional here. |
| `upstream-issues/` (§3) | Fits naturally — a methodology paper documents what it actually did, including auditing real code. | Double-edged — strong evidence of capability, weakened by the demonstration repo being one the operator controls; consider whether a second, arm's-length demonstration matters before using this as a headline capability claim. |
| Embedded fsl source (§1.2) | Lower stakes — a research corpus commonly redistributes source excerpts under their license with attribution; the missing NOTICE/LICENSE is a paperwork gap, not a narrative one. | Same technical fix needed, but a product's redistribution obligations get more scrutiny from users evaluating it for their own compliance posture — worth being unambiguous, not just technically compliant. |
| `private/` benchmark data (§1.1, §1.3) | Standard research practice to publish oracles once blinding is no longer needed — arguably strengthens reproducibility claims. | Less relevant to a product pitch either way; the contamination note (§1.3) matters more here since a product might want to keep re-running this benchmark against newer models. |
| Internal IP/hostname/paths (§1.4) | Cosmetic either way. | Cosmetic either way, redact for polish if desired. |

---

## 5. Summary table

| Item | Blocks publication? | Fix | Difficulty |
| --- | --- | --- | --- |
| Secrets/credentials in history | No — none exist | — | — |
| Internal IP/hostname/local paths | No | Redact in working tree | Low |
| Embedded fsl source, no LICENSE/NOTICE | Not legally, but a stated compliance gap already flagged in `docs/20` | Add `LICENSE`, `NOTICE`, attribution note in `m7-real-v1/README.md` | Low |
| Benchmark contamination risk (m7-real-v1/v2 oracles) | No — informational | Document as a known limitation on future reuse | Low (documentation only) |
| No as-of-today capability summary | Should block, in the operator's own framing ("最も必要なもの") | Write one canonical status document; add dated addenda to each stale diagnosis doc pointing to it | Medium — requires accurately describing today's fix, which this audit did not verify itself (out of scope, `crates/` untouched) |
| `README.md` / `HANDOFF.md` staleness | Not a hard blocker, but misleading if left as-is | Update or clearly supersede the stale sections | Low–Medium |
| `upstream-issues/` community-norms question | Judgment call, not fact | See §3 — no single fix, a framing/scoping decision | N/A |

**Nothing found requires git history rewriting.** The hardest items
here are writing-and-judgment work (an accurate status summary,
deciding how to frame `upstream-issues/`), not technical remediation.

---

## 6. Recommendation (mine — separated from the facts above)

1. **Do not publish yet.** Not because of any hard blocker (there is
   none) — because the single most consequential gap (§2, no
   current-state summary) is also the one hardest to get right quickly,
   and publishing before it exists risks the exact
   overclaim/underclaim-by-omission problem this audit found in
   `README.md`.
2. **Write the as-of-today summary first**, once today's `crates/` fix
   is verified (by whoever did that work, not assumed from this audit).
   It should state plainly what changed, cite the diagnosis documents
   it supersedes in framing (not content — they stay as historical
   record per the append-only discipline), and be the one document
   README.md points to.
3. **Add the LICENSE/NOTICE files** — low effort, already anticipated
   in `docs/20`, no reason to defer.
4. **Decide `upstream-issues/`'s fate deliberately, not by default.**
   My own reading is that it strengthens a research-record framing and
   is a mixed signal for a product framing, and that the community-norms
   question in §3 is real but not disqualifying given the operator's
   actual relationship to fsl — but this is a judgment call for the
   operator and user, not a fact I can settle.
5. **The low-severity local-network redactions (§1.4) are optional
   polish**, not a gate — do them whenever convenient, they don't
   block anything on their own.

This recommendation is mine; the facts in §1–§3 stand independent of
whether it's accepted.
