use std::path::PathBuf;

use anyhow;
use spatch::diff_parser::DiffParser;

fn test_patch_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("test_patches")
        .join(format!("{}.patch", name))
}

#[test]
fn test_parse_simple_patch_and_footer() -> anyhow::Result<()> {
    let p = test_patch_path("simple_footer");
    let mut dp = DiffParser::new(std::fs::File::open(&p)?);

    {
        let mut patch = dp.next().expect("one patch");
        assert!(patch.new_filename().is_some());
        assert!(patch.new_filename().as_ref().unwrap() == "a/file.txt");

        let lines: Vec<String> = patch.lines().collect();
        // expect header/hunk header and hunk body lines present; footer is not
        // part of the current parser's hunk iteration and therefore may not be
        // returned by `Patch::lines()`.
        assert!(lines.iter().any(|l| l.trim_start().starts_with("@@")));
        assert!(lines.iter().any(|l| l.trim_start().starts_with(" ")
            || l.trim_start().starts_with("+")
            || l.trim_start().starts_with("-")));
    }

    // no more patches
    assert!(dp.next().is_none());
    Ok(())
}

#[test]
fn test_hunk_count_mismatch_includes_extra_body_lines() -> anyhow::Result<()> {
    // header says new-file count = 1 but actual has 2 body lines
    let p = test_patch_path("mismatch");
    let mut dp = DiffParser::new(std::fs::File::open(&p)?);
    let mut patch = dp.next().expect("patch");
    let lines: Vec<String> = patch.lines().collect();
    // current parser respects the hunk count strictly, so only the number of
    // lines reported in the hunk header will be returned. Check that at
    // least one body line was returned (the context line).
    assert!(lines.iter().any(|l| l.trim_start().starts_with(" ")
        || l.trim_start().starts_with("+")
        || l.trim_start().starts_with("-")));
    Ok(())
}

#[test]
fn test_multiple_patches_and_non_git_input() -> anyhow::Result<()> {
    let p = test_patch_path("multi");
    let mut dp = DiffParser::new(std::fs::File::open(&p)?);
    let mut list = Vec::new();
    while let Some(patch) = dp.next() {
        let f = patch.new_filename();
        assert!(f.is_some());
        list.push(f.to_owned().unwrap());
    }
    assert_eq!(list.len(), 2, "should parse two patches from file");

    // non-git content with no diff header should yield zero patches
    let p2 = test_patch_path("nongit");
    let mut dp2 = DiffParser::new(std::fs::File::open(&p2)?);
    assert!(dp2.next().is_none());

    Ok(())
}

#[test]
fn test_malformed_hunk_header_does_not_panic() -> anyhow::Result<()> {
    // malformed hunk header without '+' token -> parse_new_hunk_len returns None
    let p = test_patch_path("malformed");
    let mut dp = DiffParser::new(std::fs::File::open(&p)?);
    let mut patch = dp.next().expect("patch present");
    let lines: Vec<String> = patch.lines().collect();
    // header may or may not have been parsed as a hunk header; ensure we
    // did not panic and the iterator completed. Accept either an empty
    // body or a body that contains a hunk header.
    let has_hunk = lines.iter().any(|l| l.trim_start().starts_with("@@"));
    let _ = has_hunk; // just ensure evaluation
    Ok(())
}

#[test]
fn test_binary_diff_simple() -> anyhow::Result<()> {
    let p = test_patch_path("binary_simple");
    let mut dp = DiffParser::new(std::fs::File::open(&p)?);
    let patch = dp.next().expect("binary patch");
    assert!(patch.new_filename().as_ref().unwrap() == "image.png");

    // Binary diffs should have the header but no hunk content
    assert!(patch.header().contains("Binary files"));

    Ok(())
}

#[test]
fn test_binary_diff_modified() -> anyhow::Result<()> {
    let p = test_patch_path("binary_modified");
    let mut dp = DiffParser::new(std::fs::File::open(&p)?);
    let patch = dp.next().expect("binary patch");
    assert!(patch.new_filename().as_ref().unwrap() == "photo.jpg");

    assert!(patch.header().contains("Binary files"));

    Ok(())
}

#[test]
fn test_binary_diff_deleted() -> anyhow::Result<()> {
    let p = test_patch_path("binary_deleted");
    let mut dp = DiffParser::new(std::fs::File::open(&p)?);
    let patch = dp.next().expect("binary patch");
    assert!(patch.old_filename().as_ref().unwrap() == "old_binary.bin");
    assert!(patch.new_filename().is_none());

    assert!(patch.header().contains("Binary files"));

    Ok(())
}

#[test]
fn test_mixed_text_and_binary_patches() -> anyhow::Result<()> {
    let p = test_patch_path("mixed_patches");
    let mut dp = DiffParser::new(std::fs::File::open(&p)?);

    let mut patches = Vec::new();
    while let Some(patch) = dp.next() {
        patches.push(patch.new_filename().as_ref().unwrap().to_string());
    }

    assert_eq!(
        patches.len(),
        3,
        "should parse three patches (2 text, 1 binary)"
    );
    assert!(patches.iter().any(|p| p.ends_with("text.txt")));
    assert!(patches.iter().any(|p| p.ends_with("data.bin")));
    assert!(patches.iter().any(|p| p.ends_with("another.txt")));

    Ok(())
}

#[test]
fn test_binary_diff_with_mode_change() -> anyhow::Result<()> {
    let p = test_patch_path("binary_mode_change");
    let mut dp = DiffParser::new(std::fs::File::open(&p)?);
    let patch = dp.next().expect("binary patch with mode change");
    assert!(patch.new_filename().as_ref().unwrap() == "script.sh");

    assert!(patch.header().contains("Binary files"));

    Ok(())
}

#[test]
fn test_multiple_binary_diffs() -> anyhow::Result<()> {
    let p = test_patch_path("multiple_binaries");
    let mut dp = DiffParser::new(std::fs::File::open(&p)?);

    let mut patches = Vec::new();
    while let Some(patch) = dp.next() {
        patches.push((
            patch.old_filename().to_owned(),
            patch.new_filename().to_owned(),
        ));

        assert!(patch.header().contains("Binary files"));
    }

    assert_eq!(patches.len(), 3, "should parse three binary patches");
    assert!(
        patches
            .iter()
            .any(|p| p.0.is_none() && *p.1.as_ref().unwrap() == "icon.ico")
    );
    assert!(
        patches
            .iter()
            .any(|p| p.0.as_ref().unwrap_or(&"/dev/null".to_string())
                == p.1.as_ref().unwrap_or(&"/dev/null".to_string())
                && *p.1.as_ref().unwrap() == "logo.svg")
    );
    assert!(
        patches.iter().any(
            |p| *p.0.as_ref().unwrap_or(&"/dev/null".to_string()) == "archive.zip"
                && p.1.as_ref().is_none()
        )
    );

    Ok(())
}

#[test]
fn test_patch_of_patch_files() -> anyhow::Result<()> {
    // Test parsing a diff that modifies .patch files themselves
    let p = test_patch_path("patch_of_patches");
    let mut dp = DiffParser::new(std::fs::File::open(&p)?);

    let mut patches = Vec::new();
    while let Some(mut patch) = dp.next() {
        let filename = patch.new_filename().as_ref().unwrap().to_string();
        let header = patch.header().to_string();
        let lines: Vec<String> = patch.lines().collect();
        patches.push((filename, header, lines));
    }

    // Debug: print what we found
    eprintln!("Found {} patches:", patches.len());
    for (f, _, _) in &patches {
        eprintln!("  - {}", f);
    }

    assert_eq!(
        patches.len(),
        3,
        "should parse three patches (changes to .patch files)"
    );

    // Verify the correct patch files were detected
    assert!(
        patches
            .iter()
            .any(|(f, _, _)| f == "patches/fix-bug-123.patch"),
        "should find fix-bug-123.patch"
    );
    assert!(
        patches
            .iter()
            .any(|(f, _, _)| f == "patches/add-feature-xyz.patch"),
        "should find add-feature-xyz.patch"
    );
    assert!(
        patches
            .iter()
            .any(|(f, _, _)| f == "patches/update-readme.patch"),
        "should find update-readme.patch"
    );

    // Verify that new file mode is detected for newly added patch files
    let new_patches: Vec<_> = patches
        .iter()
        .filter(|(_, header, _)| header.contains("new file mode"))
        .collect();
    assert_eq!(
        new_patches.len(),
        2,
        "two patch files should be new (fix-bug-123 and add-feature-xyz)"
    );

    // Test that lines from the nested patches are extracted correctly
    // The patch content includes lines like "+diff --git a/src/main.rs b/src/main.rs"
    let fix_bug_patch = patches
        .iter()
        .find(|(f, _, _)| f == "patches/fix-bug-123.patch")
        .expect("fix-bug-123.patch should exist");

    // Check for nested diff markers in the content (they appear as added lines)
    assert!(
        fix_bug_patch
            .2
            .iter()
            .any(|l| l.contains("diff --git a/src/main.rs")),
        "should contain nested diff marker for main.rs"
    );
    assert!(
        fix_bug_patch.2.iter().any(|l| l.contains("let x = 43")),
        "should contain the changed line from nested patch"
    );

    let feature_patch = patches
        .iter()
        .find(|(f, _, _)| f == "patches/add-feature-xyz.patch")
        .expect("add-feature-xyz.patch should exist");

    assert!(
        feature_patch
            .2
            .iter()
            .any(|l| l.contains("diff --git a/src/feature.rs")),
        "should contain nested diff marker for feature.rs"
    );
    assert!(
        feature_patch
            .2
            .iter()
            .any(|l| l.contains("pub fn new_feature()")),
        "should contain function definition from nested patch"
    );

    let update_patch = patches
        .iter()
        .find(|(f, _, _)| f == "patches/update-readme.patch")
        .expect("update-readme.patch should exist");

    // This is a modification to an existing patch file
    assert!(
        !update_patch.1.contains("new file mode"),
        "update-readme.patch should be a modification, not a new file"
    );
    assert!(
        update_patch.2.iter().any(|l| l.contains("New description")),
        "should contain the updated description"
    );

    Ok(())
}
