# H4 evaluator specification gaps

Status: blocking. Per H4, implementation stopped before changing evaluator behavior.

## G1: tokenizer authority is unspecified

`EVALUATOR_SPEC.md` sections 3.1 and 3.2 require a 16,384-token preflight, but do not identify the tokenizer name, model revision, vocabulary/config bytes or hash, executable/service-manifest hash, or a deterministic counting procedure. `README.md` and `PROTOCOL.md` require those identities to be sealed but likewise provide no concrete values. Choosing a tokenizer would create measurement authority not present in the specification.

Required design decision:

- exact tokenizer implementation and version/revision;
- content-addressed vocabulary/config assets and their normative location;
- exact byte-to-token counting algorithm, including special-token and chat-template handling;
- whether the counted input is canonical packet bytes alone or the backend's complete wrapped request;
- how a standard-library-only evaluator obtains or implements this tokenizer.

## G2: tokenizer/preflight audit representation is unspecified

Section 11.1 fixes every production artifact path, but lists no tokenizer or preflight record. The five closed schemas also expose no normative tokenizer identity/count record. `PROTOCOL.md` requires the identity, procedure, exact request bytes, component byte accounting, count, and ceiling to be sealed.

Required design decision:

- the exact artifact path and closed record schema;
- which existing manifest/request/execution records gain fields, if any;
- the canonical identity/hash preimages for tokenizer and preflight records;
- how `verify-run` independently recomputes the token count.

## G3: over-ceiling terminal semantics are unspecified

The specification says that if either complete packet exceeds the token ceiling the unit is model-ineligible before either reviewer call. It does not define the public `RUN` terminal result, typed error code, CLI exit code, or required audit artifacts for this outcome. Treating it as exit 2, a sealed non-model result, or an arm zero would produce different study enumeration and evidence.

Required design decision:

- exact `RUN` result/error and CLI exit;
- whether both packet/preflight records are sealed before refusal;
- how the model-ineligible unit is represented without becoming an arm result;
- the exact/+1 fixture construction under the frozen tokenizer.

## Resolution needed

Amend the normative evaluator specification/data/schema contract with G1--G3. Once resolved, H4 can implement the ceiling and then address X01--X08, artifact replay closure, and the remaining hostile-input totality findings without inventing measurement semantics.
