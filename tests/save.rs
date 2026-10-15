use std::fs;

use simpleeditor::editor::{Document, OpenError, SaveError};

#[test]
fn unmodified_round_trip_preserves_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let cases: &[&[u8]] = &[
        b"fn main() {}\n",
        b"line one\r\nline two\r\n",
        b"mixed\r\nendings\nhere",
        "\u{FEFF}con BOM: café, 👩‍💻, e\u{301}\r\n".as_bytes(),
        b"",
    ];
    for (i, bytes) in cases.iter().enumerate() {
        let path = dir.path().join(format!("case{i}.txt"));
        fs::write(&path, bytes).unwrap();
        let mut doc = Document::open(&path).unwrap();
        assert!(!doc.is_dirty());
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), *bytes, "case {i}");
    }
}

#[test]
fn edit_and_save_keeps_crlf_and_bom() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("win.txt");
    fs::write(&path, "\u{FEFF}a\r\nb\r\n").unwrap();
    let mut doc = Document::open(&path).unwrap();
    let end = doc.len_chars();
    doc.move_to(end, false);
    doc.insert_text("ñ 🚀\n");
    assert!(doc.is_dirty());
    doc.save().unwrap();
    assert!(!doc.is_dirty());
    assert_eq!(
        fs::read(&path).unwrap(),
        "\u{FEFF}a\r\nb\r\nñ 🚀\r\n".as_bytes()
    );
    // No temporary files left behind.
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn external_change_is_detected_before_overwriting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "original").unwrap();
    let mut doc = Document::open(&path).unwrap();
    doc.select_all();
    doc.insert_text("mine");

    fs::write(&path, "changed by another program").unwrap();
    assert!(matches!(doc.save(), Err(SaveError::ModifiedOnDisk)));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "changed by another program"
    );
    assert!(doc.is_dirty(), "buffer keeps the user's edits");

    doc.save_overwriting().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "mine");
}

#[cfg(unix)]
#[test]
fn failed_save_keeps_original_file_and_buffer() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("locked");
    fs::create_dir(&sub).unwrap();
    let path = sub.join("file.txt");
    fs::write(&path, "original").unwrap();
    let mut doc = Document::open(&path).unwrap();
    doc.select_all();
    doc.insert_text("new content");

    // Read-only directory: the temporary file cannot be created.
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o555)).unwrap();
    let result = doc.save();
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();

    if is_root() {
        return; // root ignores directory permissions
    }
    assert!(matches!(result, Err(SaveError::Io(_))), "{result:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    assert_eq!(doc.text(), "new content");
    assert!(doc.is_dirty());
    assert_eq!(fs::read_dir(&sub).unwrap().count(), 1, "no temp files left");
}

#[cfg(unix)]
fn is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .map(|o| o.stdout.starts_with(b"0"))
        .unwrap_or(false)
}

#[cfg(unix)]
#[test]
fn save_preserves_permissions_and_symlinks() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("script.sh");
    fs::write(&target, "#!/bin/sh\necho hi\n").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o750)).unwrap();
    let link = dir.path().join("link.sh");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let mut doc = Document::open(&link).unwrap();
    let end = doc.len_chars();
    doc.move_to(end, false);
    doc.insert_text("echo bye\n");
    doc.save().unwrap();

    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "#!/bin/sh\necho hi\necho bye\n"
    );
    let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o750);
}

#[cfg(windows)]
#[test]
fn save_replaces_existing_file_on_windows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("win.txt");
    fs::write(&path, "one").unwrap();
    let mut doc = Document::open(&path).unwrap();
    doc.select_all();
    doc.insert_text("two");
    doc.save().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "two");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn untitled_needs_a_path_then_save_as_binds_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = Document::untitled();
    doc.select_all();
    doc.insert_text("hola");
    assert!(matches!(doc.save(), Err(SaveError::NoPath)));
    let path = dir.path().join("nuevo ñ.txt");
    doc.save_as(path.clone()).unwrap();
    assert_eq!(doc.path(), Some(path.as_path()));
    assert_eq!(fs::read_to_string(&path).unwrap(), "hola");
    assert!(!doc.is_dirty());
}

#[test]
fn binary_invalid_and_huge_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("a.png");
    fs::write(&bin, [0x89, b'P', b'N', b'G', 0, 0]).unwrap();
    assert!(matches!(Document::open(&bin), Err(OpenError::Decode(_))));

    let latin1 = dir.path().join("latin1.txt");
    fs::write(&latin1, b"caf\xe9").unwrap();
    assert!(matches!(Document::open(&latin1), Err(OpenError::Decode(_))));

    let big = dir.path().join("big.txt");
    let f = fs::File::create(&big).unwrap();
    f.set_len(simpleeditor::editor::document::MAX_OPEN_BYTES + 1)
        .unwrap();
    assert!(matches!(
        Document::open(&big),
        Err(OpenError::TooLarge { .. })
    ));

    assert!(matches!(
        Document::open(dir.path()),
        Err(OpenError::NotAFile)
    ));
}

#[test]
fn large_document_saves_by_streaming_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.txt");
    let line = "línea con acentos y emoji 🚀 — 0123456789\r\n";
    let content = line.repeat(400_000); // ~18 MiB, CRLF
    fs::write(&path, &content).unwrap();
    let mut doc = Document::open(&path).unwrap();
    assert!(doc.is_large());
    let mid = doc.len_chars() / 2;
    doc.move_to(mid, false);
    doc.insert_text("X");
    doc.backspace();
    doc.save().unwrap();
    assert_eq!(fs::read(&path).unwrap(), content.as_bytes());
}

/// Another process holds the file open without sharing delete/write access
/// (common with editors, antivirus and Office on Windows): saving must fail
/// cleanly, keeping the original file and the buffer.
#[cfg(windows)]
#[test]
fn locked_file_on_windows_keeps_original_and_buffer() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.txt");
    fs::write(&path, "original").unwrap();
    let mut doc = Document::open(&path).unwrap();
    doc.select_all();
    doc.insert_text("new");
    let holder = fs::OpenOptions::new()
        .read(true)
        .share_mode(0) // no FILE_SHARE_* flags
        .open(&path)
        .unwrap();
    let result = doc.save();
    drop(holder);
    assert!(matches!(result, Err(SaveError::Io(_))), "{result:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    assert_eq!(doc.text(), "new");
    assert!(doc.is_dirty());
    let leftovers = fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(leftovers, 1, "temporary file removed");
}

/// Real "disk full" on a 1 MiB disk image. macOS only, needs `hdiutil`;
/// run with `cargo test --test save -- --ignored`.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "mounts a disk image; run explicitly"]
fn disk_full_keeps_original_and_buffer() {
    use std::process::Command;

    let work = tempfile::tempdir().unwrap();
    let image = work.path().join("small.dmg");
    let mount = work.path().join("mnt");
    fs::create_dir(&mount).unwrap();
    let ok = Command::new("hdiutil")
        .args(["create", "-size", "1m", "-fs", "HFS+", "-volname", "SEFULL"])
        .arg(&image)
        .status()
        .unwrap()
        .success();
    assert!(ok, "hdiutil create");
    let ok = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-mountpoint"])
        .arg(&mount)
        .arg(&image)
        .status()
        .unwrap()
        .success();
    assert!(ok, "hdiutil attach");
    let detach = || {
        let _ = Command::new("hdiutil")
            .arg("detach")
            .arg(&mount)
            .arg("-force")
            .status();
    };

    let path = mount.join("file.txt");
    fs::write(&path, "original content").unwrap();
    let mut doc = Document::open(&path).unwrap();
    doc.select_all();
    doc.insert_text(&"x".repeat(4 * 1024 * 1024)); // larger than the disk
    let result = doc.save();
    let original = fs::read_to_string(&path).unwrap();
    let entries: Vec<_> = fs::read_dir(&mount)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with(".fseventsd") && !n.starts_with(".Trashes"))
        .collect();
    detach();
    assert!(matches!(result, Err(SaveError::Io(_))), "{result:?}");
    assert_eq!(original, "original content");
    assert!(doc.is_dirty());
    assert_eq!(doc.len_chars(), 4 * 1024 * 1024);
    assert_eq!(entries, vec!["file.txt".to_owned()], "no temp file left");
}
