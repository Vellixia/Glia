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

    fs.write_file("nested/deep/file.txt", "deep")
        .await
        .unwrap();
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