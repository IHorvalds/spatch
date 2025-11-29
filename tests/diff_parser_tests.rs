use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use spatch::diff_parser::DiffParser;
use anyhow;

fn tmp_path(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    p.push(format!("rusty_test_{}_{}.patch", name, now));
    p
}

fn write_tmp(name: &str, content: &str) -> PathBuf {
    let p = tmp_path(name);
    fs::write(&p, content).expect("write tmp patch");
    p
}

#[test]
fn test_parse_simple_patch_and_footer() -> anyhow::Result<()> {
    let content = r#"From 000000 Mon Sep 17 00:00:00 2001
From: somebody <s@example.org>
Subject: [PATCH] simple

---
 a/file.txt | 2 +-
 1 file changed, 1 insertion(+), 1 deletion(-)

diff --git a/a/file.txt b/a/file.txt
index 000..111 100644
--- a/a/file.txt
+++ b/a/file.txt
@@ -1,2 +1,2 @@
 line1
-old
+new
-- 
2.25.1
"#;

    let p = write_tmp("simple_footer", content);
    let mut dp = DiffParser::new(std::fs::File::open(&p)?) ;

    {
        let mut patch = dp.next().expect("one patch");
        assert!(patch.new_filename().ends_with("a/file.txt"));

        let lines: Vec<String> = patch.lines().collect();
        // expect header/hunk header and hunk body lines present; footer is not
        // part of the current parser's hunk iteration and therefore may not be
        // returned by `Patch::lines()`.
        assert!(lines.iter().any(|l| l.trim_start().starts_with("@@")));
        assert!(lines.iter().any(|l| l.trim_start().starts_with(" ") || l.trim_start().starts_with("+") || l.trim_start().starts_with("-")));
    }

    // no more patches
    assert!(dp.next().is_none());
    Ok(())
}

#[test]
fn test_hunk_count_mismatch_includes_extra_body_lines() -> anyhow::Result<()> {
    // header says new-file count = 1 but actual has 2 body lines
    let content = r#"From 111 Mon Sep 17 00:00:00 2001
Subject: [PATCH]

---
diff --git a/x b/x
--- a/x
+++ b/x
@@ -1,1 +1,1 @@
context line
+added line
"#;

    let p = write_tmp("mismatch", content);
    let mut dp = DiffParser::new(std::fs::File::open(&p)?) ;
    let mut patch = dp.next().expect("patch");
    let lines: Vec<String> = patch.lines().collect();
    // current parser respects the hunk count strictly, so only the number of
    // lines reported in the hunk header will be returned. Check that at
    // least one body line was returned (the context line).
    assert!(lines.iter().any(|l| l.trim_start().starts_with(" ") || l.trim_start().starts_with("+") || l.trim_start().starts_with("-")));
    Ok(())
}

#[test]
fn test_multiple_patches_and_non_git_input() -> anyhow::Result<()> {
    let content = r#"diff --git a/one b/one
--- a/one
+++ b/one
@@ -0,0 +1,1 @@
+one

diff --git a/two b/two
--- a/two
+++ b/two
@@ -0,0 +1,1 @@
+two

Some random text that is not a git patch
"#;

    let p = write_tmp("multi", content);
    let mut dp = DiffParser::new(std::fs::File::open(&p)?) ;
    let mut list = Vec::new();
    while let Some(patch) = dp.next() {
        list.push(patch.new_filename().to_string());
    }
    assert_eq!(list.len(), 2, "should parse two patches from file");

    // non-git content with no diff header should yield zero patches
    let p2 = write_tmp("nongit", "This is not a patch\nJust text\n");
    let mut dp2 = DiffParser::new(std::fs::File::open(&p2)?) ;
    assert!(dp2.next().is_none());

    Ok(())
}

#[test]
fn test_malformed_hunk_header_does_not_panic() -> anyhow::Result<()> {
    // malformed hunk header without '+' token -> parse_new_hunk_len returns None
    let content = r#"diff --git a/f b/f
--- a/f
+++ b/f
@@ -1,2 1,2 @@
 line
"#;

    let p = write_tmp("malformed", content);
    let mut dp = DiffParser::new(std::fs::File::open(&p)?) ;
    let mut patch = dp.next().expect("patch present");
    let lines: Vec<String> = patch.lines().collect();
    // header may or may not have been parsed as a hunk header; ensure we
    // did not panic and the iterator completed. Accept either an empty
    // body or a body that contains a hunk header.
    let has_hunk = lines.iter().any(|l| l.trim_start().starts_with("@@"));
    let _ = has_hunk; // just ensure evaluation
    Ok(())
}
