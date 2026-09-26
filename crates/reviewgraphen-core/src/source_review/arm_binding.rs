//! Exact checked-value lookup of a claimed registry binding to a supported
//! source-review arm (TypeScript r2).
//!
//! This is pure value comparison against immutable registry identities. It
//! reads no repository and mints no source authority. Historical TypeScript
//! r1, retired Kotlin arms and any Rust tuple are never fallbacks.

use thiserror::Error;

use crate::{ContentHash, canonical_json};

use super::registry::{
    TYPESCRIPT_ARM_HASH, TYPESCRIPT_ARM_ID, TYPESCRIPT_REGISTRY_HASH, TYPESCRIPT_REGISTRY_ID,
    typescript_registry_binding,
};

const TYPESCRIPT_TUPLE_HASH: &str =
    "sha256:30b5b7ed971d9bad50164563cef086488781f36870e0c12863aa023bcf186a55";

/// The seven claimed tuple fields, compared exactly (no case/prefix folding).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArmTupleClaim {
    pub profile_id: String,
    pub profile_version: String,
    pub language: String,
    pub producer_id: String,
    pub extractor_set_hash: String,
    pub rule_set_hash: String,
    pub projection_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArmBindingClaim {
    pub registry_id: String,
    pub registry_hash: String,
    pub tuple: ArmTupleClaim,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckedArm {
    TypeScriptR2,
}

/// The exact five-field registry binding of a supported arm.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedArmBinding {
    pub arm: CheckedArm,
    pub registry_id: String,
    pub registry_hash: String,
    pub arm_id: String,
    pub arm_hash: String,
    pub tuple_hash: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum ArmBindingError {
    #[error("registry revision is not a supported v5 arm")]
    UnknownRegistryRevision,
    #[error("claimed tuple does not equal the selected arm's exact tuple")]
    RegistryBindingMismatch,
    #[error("embedded registry identity failed verification")]
    EmbeddedRegistryInvalid,
}

/// Looks up the exact arm for a claim, in lookup order: registry ID and hash
/// select one revision, then all seven tuple fields and the tuple hash must be
/// exactly that revision's values.
pub fn check_arm_binding(claim: &ArmBindingClaim) -> Result<CheckedArmBinding, ArmBindingError> {
    let (arm, expected, arm_id, arm_hash, tuple_hash) =
        match (claim.registry_id.as_str(), claim.registry_hash.as_str()) {
            (TYPESCRIPT_REGISTRY_ID, TYPESCRIPT_REGISTRY_HASH) => {
                let tuple = typescript_registry_binding().tuple;
                (
                    CheckedArm::TypeScriptR2,
                    ArmTupleClaim {
                        profile_id: tuple.profile_id,
                        profile_version: tuple.profile_version,
                        language: tuple.language,
                        producer_id: tuple.producer_id,
                        extractor_set_hash: tuple.extractor_set_hash,
                        rule_set_hash: tuple.rule_set_hash,
                        projection_id: tuple.projection_id,
                    },
                    TYPESCRIPT_ARM_ID,
                    TYPESCRIPT_ARM_HASH,
                    TYPESCRIPT_TUPLE_HASH,
                )
            }
            _ => return Err(ArmBindingError::UnknownRegistryRevision),
        };
    if claim.tuple != expected || tuple_hash_of(&claim.tuple)? != tuple_hash {
        return Err(ArmBindingError::RegistryBindingMismatch);
    }
    Ok(CheckedArmBinding {
        arm,
        registry_id: claim.registry_id.clone(),
        registry_hash: claim.registry_hash.clone(),
        arm_id: arm_id.to_owned(),
        arm_hash: arm_hash.to_owned(),
        tuple_hash: tuple_hash.to_owned(),
    })
}

fn tuple_hash_of(tuple: &ArmTupleClaim) -> Result<String, ArmBindingError> {
    let value = serde_json::json!({
        "profile_id": tuple.profile_id,
        "profile_version": tuple.profile_version,
        "language": tuple.language,
        "producer_id": tuple.producer_id,
        "extractor_set_hash": tuple.extractor_set_hash,
        "rule_set_hash": tuple.rule_set_hash,
        "projection_id": tuple.projection_id,
    });
    let bytes = canonical_json(&value).map_err(|_| ArmBindingError::EmbeddedRegistryInvalid)?;
    Ok(ContentHash::sha256(&bytes).to_string())
}
