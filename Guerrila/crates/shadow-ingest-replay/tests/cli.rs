use assert_cmd::Command;

/// G4-B1-T3: The replay binary builds and responds to --version.
///
/// Verifies:
///   1. The binary compiles and links against shadow-ingest.
///   2. --version prints a version string and exits 0.
///   3. Running with no args exits non-zero (--input is required).
#[test]
fn g4_b1_shadow_ingest_replay_cli_smoke() {
    // --version should succeed and print the version.
    let version_out = Command::cargo_bin("shadow-ingest-replay")
        .expect("binary not found")
        .arg("--version")
        .output()
        .expect("failed to run");

    assert!(
        version_out.status.success(),
        "--version should exit 0, got: {:?}",
        version_out.status,
    );
    let stdout = String::from_utf8_lossy(&version_out.stdout);
    assert!(
        stdout.contains("shadow-ingest-replay"),
        "--version stdout should contain binary name, got: {stdout}",
    );

    // No args → exit 1 (--input required).
    let no_args = Command::cargo_bin("shadow-ingest-replay")
        .expect("binary not found")
        .output()
        .expect("failed to run");

    assert!(!no_args.status.success(), "no args should exit non-zero",);
}
