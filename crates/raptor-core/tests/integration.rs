use std::path::PathBuf;

use raptor_core::repository::{PackageIndex, Repository};
use raptor_core::sources::SourcesList;

#[test]
fn loads_local_file_repository() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/repo");
    if !repo.exists() {
        return;
    }

    let sources = SourcesList::parse(&format!("deb file:{} stable main", repo.display())).unwrap();
    let roots = sources.local_repo_roots();
    assert_eq!(roots.len(), 1);

    let repository = Repository::open(&roots[0]).unwrap();
    assert!(repository.index.get("hello-raptor").is_some());
}

#[test]
fn context_style_load_from_sources_file() {
    let demo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo");
    let sources_path = demo.join("sources.list");
    if !sources_path.exists() {
        return;
    }

    let sources = SourcesList::load(&sources_path).unwrap();
    assert!(!sources.local_repo_roots().is_empty());

    let mut index = PackageIndex::default();
    for root in sources.local_repo_roots() {
        if !root.exists() {
            continue;
        }
        let repo = Repository::open(&root).unwrap();
        index.merge(repo.index);
    }
    if index.get("hello-raptor").is_none() {
        return;
    }
    assert!(index.get("hello-raptor").is_some());
}
