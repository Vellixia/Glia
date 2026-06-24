//! Integration tests for glia-fs. Verifies V1: local FS operations work with
//! ⊥ Hub network call, and path traversal is rejected.

use glia_fs::Fs;

#[tokio::test]
async fn read_write_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file("hello.txt", "hello glia").await.unwrap();
    let got = fs.read_file("hello.txt").await.unwrap();
    assert_eq!(got, "hello glia");
}

#[tokio::test]
async fn write_creates_parent_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file("nested/deep/file.txt", "deep").await.unwrap();
    let got = fs.read_file("nested/deep/file.txt").await.unwrap();
    assert_eq!(got, "deep");
}

#[tokio::test]
async fn list_dir_returns_all_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file("a.txt", "1").await.unwrap();
    fs.write_file("b.txt", "2").await.unwrap();
    fs.write_file("c.txt", "3").await.unwrap();

    let entries = fs.list_dir(".").await.unwrap();
    assert!(entries.contains(&"a.txt".to_string()));
    assert!(entries.contains(&"b.txt".to_string()));
    assert!(entries.contains(&"c.txt".to_string()));
}

#[tokio::test]
async fn path_traversal_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    let err = fs.read_file("../../etc/passwd").await;
    assert!(err.is_err(), "traversal should be rejected");
    let msg = format!("{}", err.unwrap_err());
    assert!(
        msg.contains("escapes root"),
        "expected escape error, got: {msg}"
    );
}

#[tokio::test]
async fn file_info_works() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file("info.txt", "12345").await.unwrap();
    let info = fs.file_info("info.txt").await.unwrap();
    assert_eq!(info.size, 5);
    assert!(info.is_file);
    assert!(!info.is_dir);
    assert!(info.modified.is_some());
}

#[tokio::test]
async fn missing_file_returns_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    let err = fs.read_file("nope.txt").await;
    assert!(matches!(err, Err(glia_fs::FsError::NotFound(_))));
}

#[tokio::test]
async fn empty_file_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file("empty.txt", "").await.unwrap();
    let got = fs.read_file("empty.txt").await.unwrap();
    assert_eq!(got, "");
}

#[tokio::test]
async fn whitespace_only_file_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file("ws.txt", "\n\n  \n").await.unwrap();
    let got = fs.read_file("ws.txt").await.unwrap();
    assert_eq!(got, "\n\n  \n");
}

#[tokio::test]
async fn invalid_utf8_file_returns_nonutf8_error() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    // Write raw invalid-UTF-8 bytes directly (bypass write_file which takes &str).
    let path = tmp.path().join("binary.dat");
    tokio::fs::write(&path, &[0xFF, 0xFE, 0x00, 0x01]).await.unwrap();

    let err = fs.read_file("binary.dat").await;
    assert!(matches!(err, Err(glia_fs::FsError::NonUtf8)));
}

#[tokio::test]
async fn list_empty_dir_returns_empty_vec() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    // Create a subdirectory with nothing in it.
    fs.write_file("sub/.keep", "").await.unwrap();
    let entries = fs.list_dir("sub").await.unwrap();
    assert!(entries.contains(&".keep".to_string()));

    // Now test truly empty dir.
    tokio::fs::create_dir(tmp.path().join("empty")).await.unwrap();
    let entries = fs.list_dir("empty").await.unwrap();
    assert!(entries.is_empty());
}

#[tokio::test]
async fn absolute_path_outside_root_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    let err = fs.read_file("/etc/passwd").await;
    assert!(matches!(err, Err(glia_fs::FsError::PathEscape(_))));
}

#[tokio::test]
async fn large_file_roundtrip_1mb() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    let content = "x".repeat(1024 * 1024);
    fs.write_file("big.txt", &content).await.unwrap();
    let got = fs.read_file("big.txt").await.unwrap();
    assert_eq!(got.len(), 1024 * 1024);
}

#[tokio::test]
async fn list_nonexistent_dir_returns_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    let err = fs.list_dir("nope").await;
    assert!(matches!(err, Err(glia_fs::FsError::NotFound(_))));
}

#[tokio::test]
async fn list_dir_includes_dotfiles() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file(".hidden", "secret").await.unwrap();
    fs.write_file("visible.txt", "data").await.unwrap();
    let entries = fs.list_dir(".").await.unwrap();
    assert!(entries.contains(&".hidden".to_string()));
    assert!(entries.contains(&"visible.txt".to_string()));
}

#[tokio::test]
async fn deeply_nested_path_write_read() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    let deep = (0..20).map(|i| format!("level{i}")).collect::<Vec<_>>().join("/");
    let path = format!("{deep}/file.txt");
    fs.write_file(&path, "deep").await.unwrap();
    let got = fs.read_file(&path).await.unwrap();
    assert_eq!(got, "deep");
}

#[tokio::test]
async fn unicode_path_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file("héllo/日本語.txt", "unicode content").await.unwrap();
    let got = fs.read_file("héllo/日本語.txt").await.unwrap();
    assert_eq!(got, "unicode content");
}

#[tokio::test]
async fn trailing_slash_path_handled() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file("dir/file.txt", "data").await.unwrap();
    // list_dir with trailing slash should work.
    let entries = fs.list_dir("dir/").await.unwrap();
    assert!(entries.contains(&"file.txt".to_string()));
}

#[tokio::test]
async fn concurrent_reads_same_file() {
    use std::sync::Arc;
    let tmp = tempfile::tempdir().unwrap();
    let fs = Arc::new(Fs::new(tmp.path()));

    fs.write_file("shared.txt", "shared content").await.unwrap();

    let mut handles = Vec::new();
    for _ in 0..10 {
        let fs = fs.clone();
        handles.push(tokio::spawn(async move {
            fs.read_file("shared.txt").await.unwrap()
        }));
    }
    for h in handles {
        let got = h.await.unwrap();
        assert_eq!(got, "shared content");
    }
}

#[tokio::test]
async fn write_file_overwrites_existing() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file("overwrite.txt", "original").await.unwrap();
    fs.write_file("overwrite.txt", "replaced").await.unwrap();
    let got = fs.read_file("overwrite.txt").await.unwrap();
    assert_eq!(got, "replaced");
}

#[tokio::test]
async fn file_info_on_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file("file.txt", "x").await.unwrap();
    tokio::fs::create_dir(tmp.path().join("mydir")).await.unwrap();
    let info = fs.file_info("mydir").await.unwrap();
    assert!(info.is_dir);
    assert!(!info.is_file);
}

#[tokio::test]
async fn file_info_empty_file_size_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file("empty.txt", "").await.unwrap();
    let info = fs.file_info("empty.txt").await.unwrap();
    assert_eq!(info.size, 0);
    assert!(info.is_file);
}

#[tokio::test]
async fn file_info_missing_returns_error() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    let err = fs.file_info("nope.txt").await;
    assert!(err.is_err());
}

#[tokio::test]
async fn read_file_on_directory_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    tokio::fs::create_dir(tmp.path().join("adir")).await.unwrap();
    // Reading a directory should error (IO error or similar).
    let err = fs.read_file("adir").await;
    assert!(err.is_err());
}

#[tokio::test]
async fn resolve_strips_current_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    fs.write_file("./file.txt", "content").await.unwrap();
    let got = fs.read_file("./file.txt").await.unwrap();
    assert_eq!(got, "content");
}

#[tokio::test]
async fn unicode_fs_path_write_read() {
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());
    fs.write_file("héllo/日本語/🎫.txt", "unicode-content")
        .await
        .unwrap();
    let got = fs.read_file("héllo/日本語/🎫.txt").await.unwrap();
    assert_eq!(got, "unicode-content");
}

#[tokio::test]
async fn concurrent_fs_writes_different_files() {
    use std::sync::Arc;
    let tmp = tempfile::tempdir().unwrap();
    let fs = Arc::new(Fs::new(tmp.path()));
    let mut handles = Vec::new();
    for i in 0..20 {
        let f = fs.clone();
        handles.push(tokio::spawn(async move {
            let path = format!("file{i}.txt");
            f.write_file(&path, &format!("content-{i}")).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    for i in 0..20 {
        let got = fs.read_file(&format!("file{i}.txt")).await.unwrap();
        assert_eq!(got, format!("content-{i}"));
    }
}

#[tokio::test]
#[cfg(unix)]
async fn symlink_escape_outside_root_detected() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let fs = Fs::new(tmp.path());

    // Create a symlink inside the root that points outside.
    let outside = tmp.path().parent().unwrap().join("outside.txt");
    std::fs::write(&outside, "secret").unwrap();
    let link_path = tmp.path().join("escape");
    symlink(&outside, &link_path).unwrap();

    // Reading through the symlink should be rejected.
    let result = fs.read_file("escape").await;
    assert!(result.is_err(), "symlink escape should be detected");
    let err = result.unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("escapes root"),
        "expected escape error, got: {msg}"
    );
}
