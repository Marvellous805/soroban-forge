use crate::cli::VerifyArgs;
use crate::commands::build::{self, CONTRACT_PACKAGES, WASM_TARGET};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A provenance manifest produced by `scripts/provenance.sh`
/// (schema `soroban-forge/provenance@1`).
#[derive(Debug, Deserialize)]
struct Manifest {
    schema: String,
    git_commit: Option<String>,
    generated_at: Option<String>,
    target: Option<String>,
    artifacts: Vec<ManifestArtifact>,
}

#[derive(Debug, Deserialize)]
struct ManifestArtifact {
    name: String,
    sha256: String,
    bytes: Option<u64>,
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
enum VerifyStatus {
    Match,
    Mismatch,
}

pub fn run(args: VerifyArgs) -> Result<()> {
    match (args.wasm.as_deref(), args.expected.as_deref()) {
        (Some(wasm), expected) => verify_wasm(wasm, expected, args.package.as_deref()),
        (None, Some(expected)) => verify_expected(expected, args.package.as_deref()),
        (None, None) => verify_manifest(&args.manifest),
    }
}

/// `--wasm <PATH>` (optionally with `--expected`): rebuild the contract that
/// matches the artifact's file name and compare hashes. The supplied artifact
/// is never overwritten; it is only read and hashed.
fn verify_wasm(wasm_path: &str, expected: Option<&str>, package: Option<&str>) -> Result<()> {
    let input_path = PathBuf::from(wasm_path);
    if !input_path.exists() {
        bail!("WASM artifact not found at {}", input_path.display());
    }
    let package = match package {
        Some(package) => package.to_owned(),
        None => infer_package_from_wasm_name(&input_path)?,
    };

    let rebuilt_path = rebuild_wasm(&package)?;
    let input_hash = sha256_hex_file(&input_path)?;
    let rebuilt_hash = sha256_hex_file(&rebuilt_path)?;

    let expected_hash = expected.map(expect_valid_sha256).transpose()?;
    let status = compare(&input_hash, &rebuilt_hash, expected_hash.as_deref());
    print_row(
        &package,
        "Input SHA256",
        &input_hash,
        "Rebuilt SHA256",
        &rebuilt_hash,
        status,
    );
    finish(status, &package)
}

/// `--expected <SHA256>` alone: rebuild the selected contract (or every
/// contract package when none is selected) and compare each rebuilt hash
/// against the supplied hash. No original WASM file is required.
fn verify_expected(expected: &str, package: Option<&str>) -> Result<()> {
    let expected_hash = expect_valid_sha256(expected)?;
    let packages: Vec<&str> = match package {
        Some(package) => {
            if !CONTRACT_PACKAGES.contains(&package) {
                bail!(
                    "`{package}` is not a known Soroban Forge contract package; expected one of: {}",
                    CONTRACT_PACKAGES.join(", ")
                );
            }
            vec![package]
        }
        None => CONTRACT_PACKAGES.to_vec(),
    };

    build::ensure_wasm_target_installed()?;
    for package in &packages {
        rebuild_package(package)?;
    }

    let mut mismatches = 0;
    print_header("Expected SHA256", "Actual SHA256");
    for package in &packages {
        let rebuilt_path = wasm_artifact_path(package)?;
        let rebuilt_hash = sha256_hex_file(&rebuilt_path)?;
        let status = compare(&expected_hash, &rebuilt_hash, None);
        print_row(
            package,
            "Expected SHA256",
            &expected_hash,
            "Actual SHA256",
            &rebuilt_hash,
            status,
        );
        if status == VerifyStatus::Mismatch {
            mismatches += 1;
        }
    }

    if mismatches > 0 {
        bail!("verification failed: {mismatches} artifact(s) did not match expected SHA-256");
    }
    Ok(())
}

/// `--manifest <PATH>`: validate every listed artifact in the provenance
/// manifest against a deterministic rebuild of the contract packages.
fn verify_manifest(manifest_path: &str) -> Result<()> {
    let manifest = parse_manifest(Path::new(manifest_path))?;

    build::ensure_wasm_target_installed()?;
    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("--release")
        .arg("--target")
        .arg(WASM_TARGET);
    for package in CONTRACT_PACKAGES {
        cmd.arg("--package").arg(package);
    }
    let status = cmd
        .status()
        .context("failed to run cargo build for provenance verification")?;
    if !status.success() {
        bail!("cargo build failed with status {status}");
    }

    println!("Verifying provenance manifest against source rebuild");
    if let Some(commit) = manifest.git_commit.as_deref() {
        println!("  git_commit:   {commit}");
    }
    if let Some(generated_at) = manifest.generated_at.as_deref() {
        println!("  generated_at: {generated_at}");
    }
    if let Some(target) = manifest.target.as_deref() {
        println!("  target:       {target}");
    }

    let mut mismatches = 0;
    print_header("Expected SHA256", "Actual SHA256");
    for artifact in &manifest.artifacts {
        let rebuilt_path = build::wasm_release_dir().join(&artifact.name);
        let actual_hash = match sha256_hex_file(&rebuilt_path) {
            Ok(hash) => hash,
            Err(error) => {
                println!(
                    "{:<32} {:<66} {:<66} MISSING ({error})",
                    artifact.name, artifact.sha256, "-"
                );
                mismatches += 1;
                continue;
            }
        };
        // The manifest records the artifact size it was generated from; a
        // divergent size is its own drift signal even when by slim chance the
        // hash still matches.
        let size_drift = match artifact.bytes {
            Some(recorded) => match fs::metadata(&rebuilt_path).map(|meta| meta.len()) {
                Ok(actual) if actual != recorded => {
                    println!(
                        "  note: {:<32} size drifted: manifest {recorded} bytes vs rebuilt {actual} bytes",
                        artifact.name,
                    );
                    true
                }
                _ => false,
            },
            None => false,
        };
        let status = compare(&artifact.sha256, &actual_hash, None);
        print_row(
            &artifact.name,
            "Expected SHA256",
            &artifact.sha256,
            "Actual SHA256",
            &actual_hash,
            status,
        );
        if status == VerifyStatus::Mismatch || size_drift {
            mismatches += 1;
        }
    }

    if mismatches > 0 {
        bail!("provenance verification failed: {mismatches} artifact(s) drifted");
    }
    Ok(())
}

/// Load and validate a provenance manifest, normalizing every listed SHA-256
/// to lowercase so comparisons are stable.
fn parse_manifest(path: &Path) -> Result<Manifest> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read provenance manifest at {}", path.display()))?;
    let manifest: Manifest = serde_json::from_str(&raw)
        .with_context(|| format!("malformed provenance manifest at {}", path.display()))?;
    if manifest.schema != "soroban-forge/provenance@1" {
        bail!(
            "unsupported provenance manifest schema `{}` (expected `soroban-forge/provenance@1`)",
            manifest.schema
        );
    }
    for artifact in &manifest.artifacts {
        expect_valid_sha256(&artifact.sha256)?;
    }
    Ok(manifest)
}

/// Pure hash comparison: when an explicit `expected` hash is present the
/// result matches only if both the input artifact hash and the rebuilt hash
/// equal it; otherwise the input artifact hash must equal the rebuilt hash.
fn compare(input_hash: &str, rebuilt_hash: &str, expected: Option<&str>) -> VerifyStatus {
    let matches = match expected {
        Some(expected) => {
            input_hash.eq_ignore_ascii_case(expected) && rebuilt_hash.eq_ignore_ascii_case(expected)
        }
        None => input_hash.eq_ignore_ascii_case(rebuilt_hash),
    };
    if matches {
        VerifyStatus::Match
    } else {
        VerifyStatus::Mismatch
    }
}

/// SHA-256 of `bytes`, hex-encoded lowercase — the same representation used
/// by `scripts/provenance.sh` (`sha256sum`).
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sha256_hex_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(sha256_hex(&bytes))
}

/// Validate a SHA-256 argument and normalize it to lowercase hex.
fn expect_valid_sha256(input: &str) -> Result<String> {
    let input = input.trim();
    if input.len() != 64 || !input.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!(
            "invalid SHA-256 `{input}`: expected exactly 64 hex characters (as printed by sha256sum)"
        );
    }
    Ok(input.to_ascii_lowercase())
}

/// Map a WASM artifact file name back to its contract package using the
/// repository's naming convention (`soroban_forge_escrow.wasm` →
/// `soroban-forge-escrow`).
fn infer_package_from_wasm_name(path: &Path) -> Result<String> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .context("WASM artifact path has no file name")?;
    let package = stem.replace('_', "-");
    if !CONTRACT_PACKAGES.contains(&package.as_str()) {
        bail!(
            "cannot determine a Soroban Forge contract package from `{stem}`; pass --package explicitly"
        );
    }
    Ok(package)
}

/// Rebuild `package` for `wasm32v1-none` in release mode and return the
/// rebuilt artifact's path.
fn rebuild_wasm(package: &str) -> Result<PathBuf> {
    build::ensure_wasm_target_installed()?;
    rebuild_package(package)?;
    wasm_artifact_path(package)
}

fn rebuild_package(package: &str) -> Result<()> {
    let status = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg(WASM_TARGET)
        .arg("--package")
        .arg(package)
        .status()
        .context("failed to run cargo build")?;
    if !status.success() {
        bail!("cargo build failed with status {status}");
    }
    Ok(())
}

fn wasm_artifact_path(package: &str) -> Result<PathBuf> {
    if !CONTRACT_PACKAGES.contains(&package) {
        bail!(
            "`{package}` is not a known Soroban Forge contract package; expected one of: {}",
            CONTRACT_PACKAGES.join(", ")
        );
    }
    let stem = package.replace('-', "_");
    let path = build::wasm_release_dir().join(format!("{stem}.wasm"));
    if !path.exists() {
        bail!(
            "rebuilt WASM artifact not found at {}; did the build succeed?",
            path.display()
        );
    }
    Ok(path)
}

fn print_header(expected_label: &str, actual_label: &str) {
    println!(
        "{:<32} {:<66} {:<66} Status",
        "Contract", expected_label, actual_label
    );
    println!("{:-<32} {:-<66} {:-<66} {:-<6}", "", "", "", "");
}

fn print_row(
    contract: &str,
    _expected_label: &str,
    expected_hash: &str,
    _actual_label: &str,
    actual_hash: &str,
    status: VerifyStatus,
) {
    println!(
        "{:<32} {:<66} {:<66} {}",
        contract,
        expected_hash,
        actual_hash,
        match status {
            VerifyStatus::Match => "MATCH",
            VerifyStatus::Mismatch => "MISMATCH",
        }
    );
}

fn finish(status: VerifyStatus, package: &str) -> Result<()> {
    if status == VerifyStatus::Mismatch {
        bail!("verification failed: hashes do not match for {package}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "soroban-forge-verify-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hash_comparison_reports_match_and_mismatch() {
        let hash = sha256_hex(b"payload");
        // Identical hashes match.
        assert_eq!(compare(&hash, &hash, None), VerifyStatus::Match);
        // Explicating the same hash as expected and comparing equal rebuilt hashes matches.
        assert_eq!(compare(&hash, &hash, Some(&hash)), VerifyStatus::Match);
        // Divergent hashes mismatch, with or without an expected value.
        let other = sha256_hex(b"different");
        assert_eq!(compare(&hash, &other, None), VerifyStatus::Mismatch);
        assert_eq!(compare(&hash, &other, Some(&hash)), VerifyStatus::Mismatch);
    }

    #[test]
    fn invalid_sha256_is_rejected() {
        assert!(expect_valid_sha256("not-a-hash").is_err());
        assert!(expect_valid_sha256("abc").is_err());
        // 63 hex characters is too short.
        assert!(expect_valid_sha256(&"a".repeat(63)).is_err());
        assert!(expect_valid_sha256(&"g".repeat(64)).is_err());
        // 64 hex characters normalize to lowercase.
        let normalized = expect_valid_sha256(&"A".repeat(64)).unwrap();
        assert_eq!(normalized, "a".repeat(64));
    }

    #[test]
    fn wasm_file_name_maps_to_package() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("soroban_forge_escrow.wasm");
        fs::write(&path, vec![0; 4]).unwrap();
        assert_eq!(
            infer_package_from_wasm_name(&path).unwrap(),
            "soroban-forge-escrow"
        );

        // Unknown names fail actionably rather than guessing.
        let unknown = dir.join("random_contract.wasm");
        fs::write(&unknown, vec![0; 4]).unwrap();
        let err = infer_package_from_wasm_name(&unknown).unwrap_err();
        assert!(err.to_string().contains("--package"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parses_valid_manifest() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("provenance-manifest.json");
        fs::write(
            &path,
            r#"{
                "schema": "soroban-forge/provenance@1",
                "git_commit": "abc123",
                "target": "wasm32v1-none",
                "artifacts": [
                    { "name": "soroban_forge_escrow.wasm", "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", "bytes": 123 }
                ]
            }"#,
        )
        .unwrap();
        let manifest = parse_manifest(&path).unwrap();
        assert_eq!(manifest.schema, "soroban-forge/provenance@1");
        assert_eq!(manifest.artifacts.len(), 1);
        assert_eq!(manifest.artifacts[0].name, "soroban_forge_escrow.wasm");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn malformed_manifest_is_rejected() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("provenance-manifest.json");
        fs::write(&path, "this is not json").unwrap();
        let err = parse_manifest(&path).unwrap_err();
        assert!(err.to_string().contains("malformed provenance manifest"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_manifest_is_actionable() {
        let path = temp_dir().join("does-not-exist.json");
        let err = parse_manifest(&path).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to read provenance manifest"));
    }

    #[test]
    fn manifest_with_invalid_sha_is_rejected() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("provenance-manifest.json");
        fs::write(
            &path,
            r#"{
                "schema": "soroban-forge/provenance@1",
                "artifacts": [
                    { "name": "soroban_forge_escrow.wasm", "sha256": "nope", "bytes": 123 }
                ]
            }"#,
        )
        .unwrap();
        let err = parse_manifest(&path).unwrap_err();
        assert!(err.to_string().contains("invalid SHA-256"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_wasm_artifact_is_actionable() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let missing = dir.join("soroban_forge_escrow.wasm");
        let err = verify_wasm(&missing.display().to_string(), None, None).unwrap_err();
        assert!(err.to_string().contains("WASM artifact not found"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unknown_package_for_expected_is_rejected_before_building() {
        let err = verify_expected(&"a".repeat(64), Some("not-a-contract")).unwrap_err();
        assert!(err
            .to_string()
            .contains("not a known Soroban Forge contract package"));
    }
}
