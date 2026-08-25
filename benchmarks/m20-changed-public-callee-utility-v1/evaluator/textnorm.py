"""Code-point-exact substantive text normalization."""
import unicodedata
from .model_boundary import TypedError, ValidationError

ENUMS = ("changed", "context", "support", "routine_scope_omission", "task_blocking_source_unavailable", "task_blocking_reference_unresolved", "task_blocking_projection_integrity", "bounded_context", "source", "reference", "projection", "claim", "abstention", "issue_present", "issue_absent", "inconclusive")


def _normal(value: str) -> str:
    return unicodedata.normalize("NFKC", value).casefold()


def build_lexemes(packet: dict | None = None, output: dict | None = None, extra=()) -> tuple[str, ...]:
    raw = list(extra) + list(ENUMS)
    if packet:
        raw.extend([packet.get("task_id", "")])
        inventory = packet.get("source_inventory", {})
        raw.append(inventory.get("source_inventory_id", ""))
        for source in inventory.get("admitted_sources", []):
            raw.extend([source.get("source_id", ""), source.get("payload_id", ""), source.get("path", ""), source.get("sha256", "")])
        for loss in inventory.get("declared_losses", []):
            raw.extend([loss.get("loss_id", ""), loss.get("undecidable_question_id") or "", loss.get("recovery_reference", "")])
        for payload in packet.get("payloads", []):
            raw.extend([payload.get("payload_id", ""), payload.get("sha256", "")])
    values = {_normal(x) for x in raw if isinstance(x, str) and _normal(x)}
    return tuple(sorted(values, key=lambda x: (-len(x), tuple(map(ord, x)))))


def normalize(text: str, packet: dict | None = None, output: dict | None = None, field_limit: int = 1024, extra_lexemes=()) -> dict:
    if not isinstance(text, str) or "\x00" in text or any(0xD800 <= ord(c) <= 0xDFFF for c in text):
        raise TypedError([ValidationError("invalid_text_codepoint", "", "text")])
    utf8_length = len(text.encode("utf-8", "strict"))
    s = _normal(text)
    lexemes = build_lexemes(packet, output, extra_lexemes)
    out = []
    i = 0
    while i < len(s):
        matched = next((lexeme for lexeme in lexemes if s.startswith(lexeme, i)), None)
        if matched is not None:
            out.append(" ")
            i += len(matched)
            continue
        cp = s[i]
        category = unicodedata.category(cp)
        out.append(" " if category.startswith(("P", "Z")) or cp in "\t\n\r\f\v" else cp)
        i += 1
    normalized = " ".join(part for part in "".join(out).split(" ") if part)
    tokens = []
    i = 0
    while i < len(normalized):
        if "a" <= normalized[i] <= "z":
            j = i + 1
            while j < len(normalized) and ("a" <= normalized[j] <= "z" or "0" <= normalized[j] <= "9"):
                j += 1
            tokens.append(normalized[i:j])
            i = j
        else:
            i += 1
    distinct = sorted(set(tokens))
    return {"utf8_length": utf8_length, "normalized_text": normalized, "tokens": tokens,
            "distinct_tokens": distinct, "substantive": 24 <= utf8_length <= field_limit and len(distinct) >= 3}
