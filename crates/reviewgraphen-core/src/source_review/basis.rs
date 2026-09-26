//! Constructor boundary: facts remain typed and registry-bound before wire use.

use super::registry::{RegistryError, TypeScriptRegistryBinding, validate_typescript_binding};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceFileOutcome {
    Parsed,
    ParseFailed,
    NonTargetExtension,
    ProfileExcluded,
    UnreadBound,
    UnsupportedEntry,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum SourceSyntaxRole {
    Callable,
    Call,
    Binding,
    Surface,
    Scope,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceReviewFileV1 {
    pub path: String,
    pub language: String,
    pub outcome: SourceFileOutcome,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceReviewSyntaxV1 {
    pub file_path: String,
    pub role: SourceSyntaxRole,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceReviewBasisV1 {
    pub binding: TypeScriptRegistryBinding,
    pub files: Vec<SourceReviewFileV1>,
    pub syntax: Vec<SourceReviewSyntaxV1>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SourceReviewBasisError {
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error("source-review file records must be unique and use the registered language")]
    InvalidFile,
    #[error("callable and call records require a parsed source file")]
    UnparsedCallableOrCall,
    #[error("syntax record refers to no admitted file")]
    UnknownFile,
}

impl SourceReviewBasisV1 {
    pub fn new(
        binding: TypeScriptRegistryBinding,
        files: Vec<SourceReviewFileV1>,
        syntax: Vec<SourceReviewSyntaxV1>,
    ) -> Result<Self, SourceReviewBasisError> {
        validate_typescript_binding(&binding)?;
        let mut by_path = BTreeMap::new();
        for file in &files {
            if file.language != binding.tuple.language || by_path.insert(&file.path, file).is_some()
            {
                return Err(SourceReviewBasisError::InvalidFile);
            }
        }
        for record in &syntax {
            let file = by_path
                .get(&record.file_path)
                .ok_or(SourceReviewBasisError::UnknownFile)?;
            // X1: parsed-only is common validation only for callable and call.
            if matches!(
                record.role,
                SourceSyntaxRole::Callable | SourceSyntaxRole::Call
            ) && file.outcome != SourceFileOutcome::Parsed
            {
                return Err(SourceReviewBasisError::UnparsedCallableOrCall);
            }
        }
        Ok(Self {
            binding,
            files,
            syntax,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_review::registry::typescript_registry_binding;
    #[test]
    fn x1_keeps_arm_specific_binding_and_scope_records_possible() {
        let file = SourceReviewFileV1 {
            path: "settings.gradle.kts".into(),
            language: "typescript".into(),
            outcome: SourceFileOutcome::NonTargetExtension,
        };
        assert!(
            SourceReviewBasisV1::new(
                typescript_registry_binding(),
                vec![file],
                vec![SourceReviewSyntaxV1 {
                    file_path: "settings.gradle.kts".into(),
                    role: SourceSyntaxRole::Binding
                }]
            )
            .is_ok()
        );
    }
}
