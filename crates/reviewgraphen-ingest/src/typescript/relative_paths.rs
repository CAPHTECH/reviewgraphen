//! Relative specifiers are resolved only against a supplied Git-tree table.

use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelativeEntryOutcome {
    Parsed,
    Excluded,
    Unread,
    ParseFailed,
    Other,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelativeEntry {
    pub path: String,
    pub outcome: RelativeEntryOutcome,
}
impl RelativeEntry {
    #[must_use]
    pub fn parsed(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            outcome: RelativeEntryOutcome::Parsed,
        }
    }
    #[must_use]
    pub fn unread(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            outcome: RelativeEntryOutcome::Unread,
        }
    }
    #[must_use]
    pub fn excluded(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            outcome: RelativeEntryOutcome::Excluded,
        }
    }
    #[must_use]
    pub fn parse_failed(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            outcome: RelativeEntryOutcome::ParseFailed,
        }
    }
    #[must_use]
    pub fn other(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            outcome: RelativeEntryOutcome::Other,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelativeResolution {
    pub candidates: Vec<String>,
    pub target: Option<String>,
    pub reasons: Vec<String>,
    pub primary_reason: Option<String>,
}

pub fn resolve_relative(
    caller_path: &str,
    specifier: &str,
    entries: Vec<RelativeEntry>,
) -> RelativeResolution {
    resolve_relative_in_tree(caller_path, specifier, entries, true)
}

pub fn resolve_relative_in_tree(
    caller_path: &str,
    specifier: &str,
    entries: Vec<RelativeEntry>,
    tree_complete: bool,
) -> RelativeResolution {
    let Some(base) = normalize(caller_path, specifier) else {
        return unresolved(Vec::new(), vec!["relative_specifier_unsupported"]);
    };
    let Some(candidates) = candidates(&base, specifier) else {
        return unresolved(Vec::new(), vec!["relative_specifier_unsupported"]);
    };
    let table = entries
        .into_iter()
        .map(|entry| (entry.path, entry.outcome))
        .collect::<BTreeMap<_, _>>();
    let mut reasons = Vec::new();
    let existing = candidates
        .iter()
        .filter_map(|path| table.get(path).map(|outcome| (path, outcome)))
        .collect::<Vec<_>>();
    let rejection_witness = extensionless(specifier)
        && [format!("{base}.js"), format!("{base}/index.js")]
            .iter()
            .any(|path| table.contains_key(path));
    if existing.len() > 1 || rejection_witness {
        reasons.push("relative_target_ambiguous");
    }
    if existing
        .iter()
        .any(|(_, outcome)| **outcome == RelativeEntryOutcome::Excluded)
    {
        reasons.push("relative_target_excluded");
    }
    if existing.iter().any(|(_, outcome)| {
        matches!(
            outcome,
            RelativeEntryOutcome::Unread | RelativeEntryOutcome::ParseFailed
        )
    }) {
        reasons.push("relative_target_unread");
    }
    if existing
        .iter()
        .any(|(_, outcome)| **outcome == RelativeEntryOutcome::ParseFailed)
    {
        reasons.push("parse_failure");
    }
    if !tree_complete {
        reasons.push("relative_target_unread");
    }
    if existing.is_empty() && tree_complete {
        reasons.push("relative_target_missing");
    }
    reasons.sort_by_key(|reason| precedence(reason));
    reasons.dedup();
    let target = (reasons.is_empty() && existing.len() == 1).then(|| existing[0].0.clone());
    RelativeResolution {
        candidates,
        target,
        primary_reason: reasons.first().map(|reason| (*reason).to_owned()),
        reasons: reasons.into_iter().map(str::to_owned).collect(),
    }
}
fn unresolved(candidates: Vec<String>, reasons: Vec<&str>) -> RelativeResolution {
    RelativeResolution {
        candidates,
        target: None,
        primary_reason: reasons.first().map(|reason| (*reason).to_owned()),
        reasons: reasons.into_iter().map(str::to_owned).collect(),
    }
}
fn extensionless(specifier: &str) -> bool {
    specifier
        .rsplit('/')
        .next()
        .is_some_and(|name| !name.contains('.'))
}
fn candidates(base: &str, specifier: &str) -> Option<Vec<String>> {
    if extensionless(specifier) {
        Some(vec![
            base.into(),
            format!("{base}.ts"),
            format!("{base}.tsx"),
            format!("{base}/index.ts"),
            format!("{base}/index.tsx"),
        ])
    } else if specifier.ends_with(".ts") || specifier.ends_with(".tsx") {
        Some(vec![base.into()])
    } else if specifier.ends_with(".js") {
        let stem = base.strip_suffix(".js")?;
        Some(vec![
            base.into(),
            format!("{stem}.ts"),
            format!("{stem}.tsx"),
        ])
    } else {
        None
    }
}
fn normalize(caller_path: &str, specifier: &str) -> Option<String> {
    if !(specifier.starts_with("./") || specifier.starts_with("../"))
        || specifier.contains(['\\', '\0', '?', '#', '%'])
    {
        return None;
    }
    let mut parts = caller_path.split('/').collect::<Vec<_>>();
    parts.pop();
    for part in specifier.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            value => parts.push(value),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}
fn precedence(reason: &str) -> usize {
    [
        "parse_failure",
        "unsupported_syntax",
        "unsupported_caller",
        "dynamic_dispatch",
        "relative_specifier_unsupported",
        "relative_target_unread",
        "relative_target_ambiguous",
        "relative_target_excluded",
        "relative_target_missing",
    ]
    .iter()
    .position(|candidate| *candidate == reason)
    .unwrap_or(usize::MAX)
}
