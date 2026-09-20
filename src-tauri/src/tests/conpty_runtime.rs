use super::*;

fn fixture() -> (tempfile::TempDir, PathBuf, PinnedFile, Vec<u8>) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let mut bytes = vec![0u8; 128];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
    bytes[64..70].copy_from_slice(b"PE\0\0\x64\x86");
    let expected = PinnedFile { name: "conpty.dll".into(), bytes: 128, sha256: hash(&bytes).unwrap() };
    std::fs::write(dir.join(&expected.name), &bytes).unwrap();
    (temp, dir, expected, bytes)
}
#[test]
fn ConptyRuntime_KnownSha256_001() {
    assert_eq!(hash(b"abc").unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}
#[test]
fn ConptyRuntime_VerifiedFileDeniesReplacement_002() {
    let (_temp, dir, expected, _bytes) = fixture();
    let guard = verify_file(&dir, &expected).unwrap();
    assert!(OpenOptions::new().write(true).open(dir.join("conpty.dll")).is_err());
    assert!(std::fs::remove_file(dir.join("conpty.dll")).is_err());
    drop(guard);
    assert!(std::fs::remove_file(dir.join("conpty.dll")).is_ok());
}
#[test]
fn ConptyRuntime_HashMismatchFailsClosed_003() {
    let (_temp, dir, expected, mut bytes) = fixture();
    bytes[127] = 1;
    std::fs::write(dir.join(&expected.name), bytes).unwrap();
    assert!(verify_file(&dir, &expected).unwrap_err().contains("SHA-256"));
}
#[test]
fn ConptyRuntime_WrongArchitectureFails_004() {
    let (_temp, dir, mut expected, mut bytes) = fixture();
    bytes[68..70].copy_from_slice(&0x14cu16.to_le_bytes());
    expected.sha256 = hash(&bytes).unwrap();
    std::fs::write(dir.join(&expected.name), bytes).unwrap();
    assert!(verify_file(&dir, &expected).unwrap_err().contains("x64"));
}
#[test]
fn ConptyRuntime_MissingPairDoesNotUseSystem_005() {
    let temp = tempfile::tempdir().unwrap();
    assert!(matches!(load_from(temp.path()), Err(message) if message.contains("Missing conpty.dll")));
}
#[test]
fn ConptyRuntime_FileSizeBound_006() {
    let (_temp, dir, mut expected, _bytes) = fixture();
    expected.bytes = 129;
    assert!(verify_file(&dir, &expected).unwrap_err().contains("size"));
}
