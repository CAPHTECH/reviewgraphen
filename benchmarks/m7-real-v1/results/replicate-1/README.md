# Result artifact guide

This directory is the compact, non-authority result projection for the sole
`m7-real-v1` replicate. It preserves raw model JSON, normalized candidates,
collections, private scores, manifests, validation logs, trial and
adjudication execution records, exact blinded adjudication batches, decisions,
and all recorded shell commands.

The 75 MiB full Codex trial transcripts are omitted from Git after their hashes
and command records were captured. The adjudication execution record does the
same for its transcripts. The 109 MiB per-item copies of full production files
are reproducible from the corpus and are omitted; the exact 60-line-margin
source excerpts actually mounted for blind adjudication are retained
losslessly as Base64-encoded `.txt.b64` files under
`adjudication/public/batches/`.

Research conclusions and limitations are in [`REPORT.md`](REPORT.md). The
canonical measured summary is [`summary.json`](summary.json). Private
role/arm reconciliation remains separated under `private/` and
`adjudication/private/`.
