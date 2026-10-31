use super::*;
use std::sync::mpsc;
use std::time::Duration;

struct BlockedDrop {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl Drop for BlockedDrop {
    fn drop(&mut self) {
        self.entered.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
#[test]
#[allow(non_snake_case)]
fn OriginalIo_DropStartedIsNotReturned_001() {
    let witness = IoDropWitness::default();
    drop(TrackedDrop::new((), witness.clone(), 2));
    drop(TrackedDrop::new((), witness.clone(), 4));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let owner = TrackedDrop::new(
        BlockedDrop {
            entered: entered_tx,
            release: release_rx,
        },
        witness.clone(),
        1,
    );
    let worker = std::thread::spawn(move || drop(owner));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let before = witness.observe();
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(before, IoDropState::Pending);
    assert!(witness.all_returned());
}

struct PanicDrop;
impl Drop for PanicDrop {
    fn drop(&mut self) {
        panic!("TEST_ONLY_IO_DROP_PANIC");
    }
}
#[test]
#[allow(non_snake_case)]
fn OriginalIo_UnwindNeverMarksSuccessfulReturn_002() {
    let witness = IoDropWitness::default();
    drop(TrackedDrop::new((), witness.clone(), 2));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(TrackedDrop::new(PanicDrop, witness.clone(), 4));
    }));
    assert!(result.is_err());
    assert_eq!(
        witness.observe(),
        IoDropState::Pending,
        "the remaining reader is still owned"
    );
    drop(TrackedDrop::new((), witness.clone(), 1));
    assert_eq!(witness.observe(), IoDropState::Unknown);
    assert!(!witness.all_returned());
}

#[test]
#[allow(non_snake_case)]
fn OriginalIo_TransferredReaderDelegatesAndRetainsOwnership_003() {
    let witness = IoDropWitness::default();
    let mut reader: Box<dyn Read + Send> = Box::new(TrackedDrop::new(
        std::io::Cursor::new(b"TEST_ONLY".to_vec()),
        witness.clone(),
        1,
    ));
    drop(TrackedDrop::new((), witness.clone(), 2));
    drop(TrackedDrop::new((), witness.clone(), 4));
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"TEST_ONLY");
    assert_eq!(
        witness.observe(),
        IoDropState::Pending,
        "EOF is not reader Drop"
    );
    drop(reader);
    assert_eq!(witness.observe(), IoDropState::Returned);
}
