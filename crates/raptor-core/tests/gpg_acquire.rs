use raptor_core::acquire::{ensure_deb, AcquireContext};
use raptor_core::control::ControlFile;
use raptor_core::repository::PackageIndexEntry;
use raptor_core::trust::{write_trust_record, sha256_file, TrustRecord};

#[test]
fn remote_acquire_requires_trust_record() {
    let dir = std::env::temp_dir().join(format!("raptor-gpg-acquire-{}", std::process::id()));
    let packages = dir.join("Packages");
    let keyring = dir.join("keyring.gpg");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&packages, b"Package: demo\nVersion: 1\nArchitecture: all\nFilename: pool/d.deb\n").unwrap();
    std::fs::write(&keyring, b"").unwrap();

    let entry = PackageIndexEntry {
        control: ControlFile::parse(
            "Package: demo\nVersion: 1\nArchitecture: all\nFilename: pool/d.deb\n",
        )
        .unwrap(),
        file_path: dir.join("missing.deb"),
        source_uri: Some("https://example.com/ubuntu".into()),
        packages_index_path: Some(packages.clone()),
        signed_by: Some(keyring.to_string_lossy().into_owned()),
        suite: Some("jammy".into()),
        component: Some("main".into()),
        repo_priority: 500,
    };

    std::env::remove_var("RAPTOR_ALLOW_INSECURE");
    let ctx = AcquireContext {
        archives_dir: dir.join("archives"),
    };
    assert!(ensure_deb(&entry, &ctx).is_err());

    let record = TrustRecord {
        source_uri: "https://example.com/ubuntu".into(),
        suite: "jammy".into(),
        component: "main".into(),
        arch: "amd64".into(),
        keyring: keyring.to_string_lossy().into_owned(),
        packages_sha256: sha256_file(&packages).unwrap(),
        verified_at_secs: 1,
    };
    write_trust_record(&packages, &record).unwrap();

    // Trust is valid but download will fail (no server) — error should not be trust-related.
    let err = ensure_deb(&entry, &ctx).unwrap_err().to_string();
    assert!(!err.contains("trust record"), "unexpected: {err}");
    let _ = std::fs::remove_dir_all(dir);
}
