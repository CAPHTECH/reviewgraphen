# Diagnostic evidence normalization record

One copied execution artifact was normalized solely to satisfy `git diff
--check`:

- source: snapshot-34 B1 `adapter.stderr` in the temporary run root;
- original bytes: 610;
- original SHA-256:
  `71d1e00b562b3bd7141df759ca8d25c34cc8f5b2ded917ab546ad722ad219698`;
- repository copy bytes: 609;
- repository-copy SHA-256:
  `2bbb1939f33e2b0ddf450f38e23f1631836bd66f68f27f685af7812f5c833f2c`;
- transformation: deletion of exactly one trailing blank line; and
- reason: avoid a new-blank-line-at-EOF violation from `git diff --check`.

No diagnostic content, JSON event, or error text changed. Future byte changes
made while importing execution artifacts require the same original hash,
normalized hash, exact transformation, and reason record.

The post-timeout-amendment snapshot-06 B1 `adapter.stderr` was normalized by
the same rule:

- original bytes: 584;
- original SHA-256:
  `fc777b9f17de0c87d8dab8a8c0f2592a57a1ff3b15ab3f7cfdc0c1c7426cfa42`;
- repository copy bytes: 583;
- repository-copy SHA-256:
  `4276e42e8e64caa039cbafa2bd5664736ddc8980c2e745edbb43b73efd536bba`;
- transformation: deletion of exactly one trailing blank line; and
- reason: avoid a new-blank-line-at-EOF violation from `git diff --check`.

The `Model unloaded.` error event and all other diagnostic bytes are
otherwise unchanged.

Both parallel-2 snapshot-06 `adapter.stderr` files were normalized by deleting
exactly one trailing blank line for `git diff --check`:

- B1: 696 bytes and SHA-256
  `377a8e5e917b2df6bff701d88f5a24c5d146cc68cbe77e4758e4654a26387ffa`
  before; 695 bytes and SHA-256
  `34b512ee28df1d057cd6e1a69dc3b38b0aadebe249d0a980e87d94cee9e2cf40`
  after.
- full: 696 bytes and SHA-256
  `0f47e013504fbad10095479b778a54415437f1a7f76fd4cec711dad588713d80`
  before; 695 bytes and SHA-256
  `b809059fa175046c83ef489298807a8da027f5dd1db895406bf601611d0f4a2f`
  after.

No model-crash event or other diagnostic content changed.
