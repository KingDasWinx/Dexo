use std::fs;
use std::path::PathBuf;

#[test]
fn mdbook_summary_files_exist_and_docs_have_no_sentinels() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/src");
    let summary = fs::read_to_string(root.join("SUMMARY.md")).unwrap();
    for line in summary.lines() {
        if let Some(start) = line.find('(')
            && let Some(end) = line.rfind(')')
        {
            let rel = &line[start + 1..end];
            if rel.ends_with(".md") {
                assert!(root.join(rel).is_file(), "missing {rel}");
            }
        }
    }
    for entry in fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            let text = fs::read_to_string(&path).unwrap();
            assert!(
                !text.contains("SUPER_SECRET_SENTINEL"),
                "secret sentinel in {}",
                path.display()
            );
        }
    }
}

#[test]
fn release_checklist_has_no_open_product_boxes() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/testing/release-checklist.md");
    let text = fs::read_to_string(path).unwrap();
    assert!(
        !text.contains("- [ ]"),
        "unchecked product requirement remains in release-checklist.md"
    );
}
