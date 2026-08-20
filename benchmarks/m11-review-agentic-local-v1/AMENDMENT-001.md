# Amendment 001 — existing-path read-only bind

Status: frozen before the first model request.

The first scheduled launch exited before the agent process or model request
because bwrap could not create the proposed `/opt/m11` bind target under the
read-only root.  The empty stream, zero elapsed seconds, and stderr are retained
under `runs/infrastructure-bwrap-bind-target-missing`.

The experiment directory is now read-only-bound at its already-existing
absolute path, and `reviewgraphen-context` reads the same frozen projection at
that path.  Prompt, projection bytes, source bytes, backend, order, retry rule,
and all semantic conditions are unchanged.  This is an infrastructure repair,
not a semantic retry: no model request existed to retry.

