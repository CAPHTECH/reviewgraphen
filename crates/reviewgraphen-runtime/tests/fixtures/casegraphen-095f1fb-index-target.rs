use super::{
    projection::{hash, query_memory},
    query::lexical_terms,
    MemoryIndex, MemoryIndexItem, MemoryIndexValidation, MemoryPolicy, MemoryQuery,
    MemoryValidationFinding, MEMORY_INDEX_SCHEMA,
};
use crate::native_model::CaseSpace;

pub fn rebuild_memory_index(
    case_space: &CaseSpace,
    query: &MemoryQuery,
    policy: &MemoryPolicy,
) -> Result<MemoryIndex, Vec<MemoryValidationFinding>> {
    let projection = query_memory(case_space, query, policy)?;
    let mut index = MemoryIndex {
        schema: MEMORY_INDEX_SCHEMA.to_owned(),
        base_revision_id: projection.base_revision_id,
        policy_id: policy.policy_id.clone(),
        query_hash: projection.query_hash,
        items: projection
            .items
            .iter()
            .map(|item| MemoryIndexItem {
                claim_id: item.claim_id.clone(),
                memory_kind: item.memory_kind,
                subject_refs: item.subject_refs.clone(),
                lexical_terms: lexical_terms(item),
                source_refs: item.source_refs.clone(),
            })
            .collect(),
        index_content_hash: String::new(),
        derived: true,
        authoritative: false,
    };
    index
        .items
        .sort_by(|left, right| left.claim_id.cmp(&right.claim_id));
    index.index_content_hash = index_content_hash(&index);
    Ok(index)
}

pub fn validate_memory_index(
    case_space: &CaseSpace,
    query: &MemoryQuery,
    policy: &MemoryPolicy,
    actual: &MemoryIndex,
) -> MemoryIndexValidation {
    match rebuild_memory_index(case_space, query, policy) {
        Ok(expected) => {
            let recomputed_actual_hash = index_content_hash(actual);
            let mut findings = Vec::new();
            if !actual.derived {
                findings.push(MemoryValidationFinding {
                    code: "memory_index_not_derived".to_owned(),
                    location: "$.derived".to_owned(),
                    detail: "a memory index must declare itself derived".to_owned(),
                });
            }
            if actual.authoritative {
                findings.push(MemoryValidationFinding {
                    code: "memory_index_claims_authority".to_owned(),
                    location: "$.authoritative".to_owned(),
                    detail: "a derived memory index must never claim authority".to_owned(),
                });
            }
            if actual.index_content_hash != recomputed_actual_hash {
                findings.push(MemoryValidationFinding {
                    code: "memory_index_content_hash_mismatch".to_owned(),
                    location: "$.index_content_hash".to_owned(),
                    detail: "the declared content hash does not match the supplied index body"
                        .to_owned(),
                });
            }
            if actual != &expected {
                findings.push(MemoryValidationFinding {
                    code: "memory_index_not_rebuild_equivalent".to_owned(),
                    location: "$.items".to_owned(),
                    detail: "the supplied derived index differs from a replay-based rebuild"
                        .to_owned(),
                });
            }
            let valid = findings.is_empty();
            MemoryIndexValidation {
                valid,
                expected_content_hash: expected.index_content_hash,
                actual_content_hash: actual.index_content_hash.clone(),
                findings,
            }
        }
        Err(findings) => MemoryIndexValidation {
            valid: false,
            expected_content_hash: String::new(),
            actual_content_hash: actual.index_content_hash.clone(),
            findings,
        },
    }
}

fn index_content_hash(index: &MemoryIndex) -> String {
    let mut content = index.clone();
    content.index_content_hash.clear();
    hash(&content)
}
