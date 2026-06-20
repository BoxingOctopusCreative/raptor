use std::path::Path;

use raptor_core::error::Error;
use raptor_core::release::{ReleaseChecksum, ReleaseIndex};
use raptor_core::remote::fetch_remote_indexes;
use raptor_core::sources::SourcesList;
use raptor_core::verify::verify_payload_checksums;

#[test]
fn rejects_unsigned_remote_without_override() {
    let sources = SourcesList::parse("deb https://example.com/ubuntu jammy main\n").unwrap();
    std::env::remove_var("RAPTOR_ALLOW_INSECURE");
    let err = fetch_remote_indexes(&sources, Path::new("/tmp/cache"), "amd64").unwrap_err();
    assert!(matches!(err, Error::InsecureRepository(_)));
}

#[test]
fn verifies_packages_against_release_checksums() {
    let file = std::env::temp_dir().join(format!("raptor-verify-test-{}", std::process::id()));
    std::fs::write(&file, b"test payload").unwrap();

    let checksum = ReleaseChecksum {
        size: 12,
        md5: None,
        sha256: Some(
            "813ca5285c28ccee5cab8b10ebda9c908fd6d78ed9dc94cc65ea6cb67a7f13ae".into(),
        ),
    };

    verify_payload_checksums(&file, &checksum).unwrap();

    let bad = ReleaseChecksum {
        size: 12,
        md5: None,
        sha256: Some("00".repeat(64)),
    };
    assert!(verify_payload_checksums(&file, &bad).is_err());
    let _ = std::fs::remove_file(file);
}

#[test]
fn release_index_lookup_matches_apt_paths() {
    let content = r#"SHA256:
 813ca5285c28ccee5cab8b10ebda9c908fd6d78ed9dc94cc65ea6cb67a7f13ae 12 main/binary-amd64/Packages.gz
"#;
    let index = ReleaseIndex::parse(content).unwrap();
    assert!(index.checksum("main/binary-amd64/Packages.gz").is_some());
}
