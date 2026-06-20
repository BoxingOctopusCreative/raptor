use std::path::PathBuf;

use raptor_core::acquire::{build_package_url, ensure_deb, verify_control_checksums, AcquireContext};
use raptor_core::control::ControlFile;
use raptor_core::repository::PackageIndexEntry;

#[test]
fn ensure_deb_uses_local_pool_file() {
    let demo_deb = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/repo/pool/h/hello-raptor_0.1.0_all.deb");
    if !demo_deb.exists() {
        return;
    }

    let control = ControlFile::parse(
        "Package: hello-raptor\nVersion: 0.1.0\nArchitecture: all\nFilename: pool/h/hello-raptor_0.1.0_all.deb\n",
    )
    .unwrap();
    let entry = PackageIndexEntry {
        control,
        file_path: demo_deb.clone(),
        source_uri: Some("https://example.com/ubuntu".into()),
        packages_index_path: None,
        signed_by: None,
        suite: None,
        component: None,
    };

    let archives = std::env::temp_dir().join(format!("raptor-archives-{}", std::process::id()));
    let acquire_ctx = AcquireContext { archives_dir: archives };
    let resolved = ensure_deb(&entry, &acquire_ctx).unwrap();
    assert_eq!(resolved, demo_deb);
}

#[test]
fn verify_control_checksums_matches_index_metadata() {
    let file = std::env::temp_dir().join(format!("raptor-acquire-{}", std::process::id()));
    std::fs::write(&file, b"test payload").unwrap();
    let control = ControlFile {
        package: "demo".into(),
        version: "1".into(),
        size: "12".into(),
        sha256: "813ca5285c28ccee5cab8b10ebda9c908fd6d78ed9dc94cc65ea6cb67a7f13ae".into(),
        ..Default::default()
    };
    verify_control_checksums(&file, &control).unwrap();
    let _ = std::fs::remove_file(file);
}

#[test]
fn package_url_joins_uri_and_filename() {
    assert_eq!(
        build_package_url("http://archive.ubuntu.com/ubuntu", "pool/main/a/apt_1_amd64.deb"),
        "http://archive.ubuntu.com/ubuntu/pool/main/a/apt_1_amd64.deb"
    );
}
