//! Binds the `syn` and `proc-macro2` versions this crate actually built
//! against into compile-time constants, read from the workspace
//! `Cargo.lock` -- the authoritative record of what Cargo really resolved
//! -- rather than a string copied by hand from `Cargo.toml`, which could
//! silently drift out of sync with the real pin. `proc-macro2` is bound
//! for the same reason as `syn`: `syn`'s own `Span`s (and this crate's
//! derived `Location`s and symbol IDs) are backed directly by
//! `proc_macro2::Span`, so a different `proc-macro2` resolution is a
//! different span/location-computation implementation, not merely an
//! unrelated transitive dependency.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));
    let lock_path = find_cargo_lock(&manifest_dir).unwrap_or_else(|| {
        panic!(
            "could not locate a Cargo.lock at or above {}",
            manifest_dir.display()
        )
    });
    println!("cargo:rerun-if-changed={}", lock_path.display());

    let lock_text = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", lock_path.display()));
    let lock: toml::Table = lock_text
        .parse()
        .unwrap_or_else(|error| panic!("failed to parse {} as TOML: {error}", lock_path.display()));

    let crate_name = env::var("CARGO_PKG_NAME").expect("CARGO_PKG_NAME is set by cargo");
    for (dependency_name, env_var) in [
        ("syn", "REVIEWGRAPHEN_INGEST_SYN_VERSION"),
        ("proc-macro2", "REVIEWGRAPHEN_INGEST_PROC_MACRO2_VERSION"),
    ] {
        let version = resolve_locked_dependency_version(&lock, &crate_name, dependency_name)
            .unwrap_or_else(|| {
                panic!(
                    "{} has no resolvable `{dependency_name}` dependency for `{crate_name}`",
                    lock_path.display()
                )
            });
        println!("cargo:rustc-env={env_var}={version}");
    }
}

/// Walks upward from `start` looking for a `Cargo.lock`. The workspace root
/// (not this crate's own directory) holds it, and that relative depth is an
/// implementation detail of the current workspace layout, not something to
/// hardcode.
fn find_cargo_lock(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(candidate) = dir {
        let lock_path = candidate.join("Cargo.lock");
        if lock_path.is_file() {
            return Some(lock_path);
        }
        dir = candidate.parent();
    }
    None
}

/// Resolves the exact version Cargo locked for `dependency_name` as a
/// dependency of `package_name`, straight from a parsed `Cargo.lock`. A
/// lock file's `dependencies` entries are either the bare package name
/// (unambiguous) or `"name version"` (when more than one same-named package
/// is locked, as is the case for `syn` in this workspace); either shape
/// resolves to the one real, locked version, never a guess.
fn resolve_locked_dependency_version(
    lock: &toml::Table,
    package_name: &str,
    dependency_name: &str,
) -> Option<String> {
    let packages = lock.get("package")?.as_array()?;
    let own_package = packages
        .iter()
        .find(|package| package.get("name").and_then(toml::Value::as_str) == Some(package_name))?;
    let dependency_ref = own_package
        .get("dependencies")
        .and_then(toml::Value::as_array)?
        .iter()
        .filter_map(toml::Value::as_str)
        .find(|dependency| {
            *dependency == dependency_name || dependency.starts_with(&format!("{dependency_name} "))
        })?;

    if let Some(version) = dependency_ref.strip_prefix(&format!("{dependency_name} ")) {
        return Some(version.to_owned());
    }
    packages
        .iter()
        .find(|package| package.get("name").and_then(toml::Value::as_str) == Some(dependency_name))
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned)
}
