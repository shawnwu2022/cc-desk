use super::*;
use std::fs;
fn budget() -> Budget {
    Budget::new(Limits::default())
}

#[test]
fn same_name_is_read_from_each_held_root() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a");
    let b = t.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    fs::write(a.join("same.json"), b"LEFT").unwrap();
    fs::write(b.join("same.json"), b"RIGHT").unwrap();
    let left = Root::open(&a).unwrap();
    let right = Root::open(&b).unwrap();
    assert_ne!(left.key(), right.key());
    assert_eq!(
        left.read(Path::new("same.json"), &mut budget()).unwrap(),
        Some(b"LEFT".to_vec())
    );
    assert_eq!(
        right.read(Path::new("same.json"), &mut budget()).unwrap(),
        Some(b"RIGHT".to_vec())
    );
    left.current().unwrap();
    right.current().unwrap();
}
#[test]
fn absent_root_is_not_defaulted_or_created() {
    let t = tempfile::tempdir().unwrap();
    let missing = t.path().join("missing");
    assert!(Root::open(&missing).is_err());
    assert!(!missing.exists());
    assert!(Root::open(Path::new("relative")).is_err());
}
#[test]
fn missing_optional_file_is_distinct_from_permission_failure() {
    let t = tempfile::tempdir().unwrap();
    let r = Root::open(t.path()).unwrap();
    assert_eq!(
        r.read(Path::new("missing.json"), &mut budget()).unwrap(),
        None
    );
    assert!(r.read(Path::new("../secret"), &mut budget()).is_err());
    assert!(r.read(t.path(), &mut budget()).is_err());
    assert!(r.read(Path::new("x:alternate"), &mut budget()).is_err());
    assert!(r.read(Path::new("x\\y"), &mut budget()).is_err());
}
#[test]
fn file_and_aggregate_budgets_reject_without_truncation() {
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("x"), b"12345").unwrap();
    let r = Root::open(t.path()).unwrap();
    let mut b = Budget::new(Limits {
        file_bytes: 4,
        total_bytes: 100,
        entries: 10,
    });
    assert_eq!(
        r.read(Path::new("x"), &mut b).unwrap_err(),
        "SOURCE_TOO_LARGE"
    );
    let mut b = Budget::new(Limits {
        file_bytes: 5,
        total_bytes: 5,
        entries: 10,
    });
    assert_eq!(
        r.read(Path::new("x"), &mut b).unwrap(),
        Some(b"12345".to_vec())
    );
    assert_eq!(
        r.read(Path::new("x"), &mut b).unwrap_err(),
        "SOURCE_TOO_LARGE"
    );
}
#[test]
fn enumeration_is_bounded_and_never_returns_partial_success() {
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("b"), b"2").unwrap();
    fs::write(t.path().join("a"), b"1").unwrap();
    let r = Root::open(t.path()).unwrap();
    let mut b = Budget::new(Limits {
        file_bytes: 100,
        total_bytes: 100,
        entries: 1,
    });
    assert_eq!(
        r.list(Path::new(""), &mut b).err(),
        Some("SOURCE_TOO_MANY_ENTRIES")
    );
    let entries = r.list(Path::new(""), &mut budget()).unwrap();
    assert_eq!(
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(entries.iter().all(|e| e.is_file && !e.is_dir));
}
#[test]
fn held_root_cannot_silently_follow_replacement() {
    let t = tempfile::tempdir().unwrap();
    let active = t.path().join("active");
    fs::create_dir(&active).unwrap();
    fs::write(active.join("x"), b"old").unwrap();
    let r = Root::open(&active).unwrap();
    match fs::rename(&active, t.path().join("retired")) {
        Ok(()) => {
            fs::create_dir(&active).unwrap();
            fs::write(active.join("x"), b"new").unwrap();
            assert_eq!(r.current().err(), Some("SOURCE_CHANGED"));
            assert!(r.read(Path::new("x"), &mut budget()).is_err());
        }
        Err(e) => {
            // Windows directory capabilities intentionally deny delete-sharing.
            #[cfg(not(windows))]
            panic!("unexpected rename denial: {e}");
            #[cfg(windows)]
            {
                // Delete-sharing denial can surface as ERROR_SHARING_VIOLATION
                // rather than Rust's PermissionDenied classification.
                assert!(
                    matches!(e.raw_os_error(), Some(5 | 32)),
                    "unexpected rename error: {e:?}"
                );
                assert_eq!(
                    r.read(Path::new("x"), &mut budget()).unwrap(),
                    Some(b"old".to_vec())
                );
            }
        }
    }
}
#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected() {
    use std::os::unix::fs::symlink;
    let t = tempfile::tempdir().unwrap();
    let inside = t.path().join("inside");
    fs::create_dir(&inside).unwrap();
    fs::write(t.path().join("secret"), b"secret").unwrap();
    symlink(t.path().join("secret"), inside.join("escape")).unwrap();
    let r = Root::open(&inside).unwrap();
    assert!(r.read(Path::new("escape"), &mut budget()).is_err());
}
#[cfg(unix)]
#[test]
fn invalid_unicode_paths_and_stored_names_are_rejected() {
    use std::os::unix::ffi::OsStringExt;
    let t = tempfile::tempdir().unwrap();
    let r = Root::open(t.path()).unwrap();
    let name = std::ffi::OsString::from_vec(vec![255]);
    // This exercises production rejection even when the filesystem itself
    // rejects creating the filename (observed as EILSEQ on the macOS runner).
    assert_eq!(
        r.read(Path::new(&name), &mut budget()).err(),
        Some("SOURCE_INVALID_TEXT")
    );
    match fs::write(t.path().join(&name), b"bad-name") {
        Ok(()) => assert_eq!(
            r.list(Path::new(""), &mut budget()).err(),
            Some("SOURCE_INVALID_TEXT")
        ),
        Err(error) => {
            #[cfg(target_os = "macos")]
            {
                assert_eq!(error.raw_os_error(), Some(libc::EILSEQ));
                assert!(r.list(Path::new(""), &mut budget()).unwrap().is_empty());
            }
            #[cfg(not(target_os = "macos"))]
            panic!("unexpected invalid-name fixture failure: {error}");
        }
    }
}
#[cfg(windows)]
#[test]
fn windows_junction_escape_cannot_read_external_bytes() {
    use std::process::Command;
    let t = tempfile::tempdir().unwrap();
    let inside = t.path().join("inside");
    let outside = t.path().join("outside");
    fs::create_dir(&inside).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret"), b"secret").unwrap();
    let status = Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(inside.join("escape"))
        .arg(&outside)
        .status()
        .unwrap();
    assert!(status.success());
    let r = Root::open(&inside).unwrap();
    assert!(r.read(Path::new("escape/secret"), &mut budget()).is_err());
}
