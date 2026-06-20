//! Mirror sync tests using a local `file://` mock upstream (no network, ~KB of disk).

use std::path::PathBuf;

use raptor_core::mirror::{mock::MockUpstream, sync_mirror, MirrorConfig};

fn demo_deb_available() -> bool {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/hello-raptor_0.1.0_all.deb")
        .exists()
}

#[test]
fn sync_mirror_indexes_from_mock_upstream() {
    if !demo_deb_available() {
        eprintln!("skipping: run examples/demo.sh to create demo deb fixture");
        return;
    }

    let upstream_dir = std::env::temp_dir().join(format!("raptor-mock-upstream-{}", std::process::id()));
    let mirror_dir = std::env::temp_dir().join(format!("raptor-mock-mirror-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&upstream_dir);
    let _ = std::fs::remove_dir_all(&mirror_dir);

    let mock = MockUpstream::build(&upstream_dir, 1).unwrap();
    let config = MirrorConfig {
        sync_pool: false,
        ..mock.mirror_config(0)
    };

    let report = sync_mirror(&mirror_dir, &config).unwrap();
    assert_eq!(report.indexes.len(), 1);
    assert!(report.indexes[0].exists());
    assert!(mirror_dir.join("dists/stable/Release").exists());
    assert!(report.pool.is_empty());

    let _ = std::fs::remove_dir_all(&upstream_dir);
    let _ = std::fs::remove_dir_all(&mirror_dir);
}

#[test]
fn sync_mirror_pool_respects_package_limit() {
    if !demo_deb_available() {
        eprintln!("skipping: run examples/demo.sh to create demo deb fixture");
        return;
    }

    let upstream_dir =
        std::env::temp_dir().join(format!("raptor-mock-upstream-pool-{}", std::process::id()));
    let mirror_dir =
        std::env::temp_dir().join(format!("raptor-mock-mirror-pool-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&upstream_dir);
    let _ = std::fs::remove_dir_all(&mirror_dir);

    let mock = MockUpstream::build(&upstream_dir, 3).unwrap();
    let config = mock.mirror_config(2);

    let report = sync_mirror(&mirror_dir, &config).unwrap();
    assert_eq!(report.indexes.len(), 1);
    assert_eq!(report.pool.len(), 2, "pool_package_limit should cap downloads");

    for path in &report.pool {
        assert!(path.exists());
        assert!(path.metadata().unwrap().len() > 0);
    }

    let _ = std::fs::remove_dir_all(&upstream_dir);
    let _ = std::fs::remove_dir_all(&mirror_dir);
}

#[test]
fn mock_upstream_uses_file_uri() {
    if !demo_deb_available() {
        return;
    }

    let upstream_dir = std::env::temp_dir().join(format!("raptor-mock-uri-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&upstream_dir);

    let mock = MockUpstream::build(&upstream_dir, 1).unwrap();
    assert!(mock.upstream_uri().starts_with("file://"));
    assert!(upstream_dir.join("dists/stable/Release").exists());

    let _ = std::fs::remove_dir_all(&upstream_dir);
}
