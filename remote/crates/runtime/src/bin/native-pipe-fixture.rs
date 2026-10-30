//! 固定、无账号的 TEST_ONLY pipe fixture；禁止作为 Host 发布。
#[cfg(all(windows, feature = "conpty-qualification"))]
fn main() {
    use std::{
        io::{Read, Write},
        time::Duration,
    };
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 3 && args[1] == "TEST_ONLY_PTY_OUTPUT" {
        // Finite public output only. No stdin, account, subprocess or network.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(15));
            std::process::exit(90);
        });
        let marker = std::path::PathBuf::from(&args[2]);
        std::fs::write(marker.with_extension("entered"), b"0").unwrap();
        std::fs::write(marker.with_extension("progress"), b"0").unwrap();
        std::fs::write(&marker, b"TEST_ONLY_OUTPUT").unwrap();
        while std::fs::read(marker.with_extension("go")).ok().as_deref() != Some(b"TEST_ONLY_GO") {
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut out = std::io::stdout().lock();
        for i in 1..=64 {
            std::fs::write(marker.with_extension("entered"), i.to_string()).unwrap();
            out.write_all(&[b'A'; 16384]).unwrap();
            out.flush().unwrap();
            std::fs::write(marker.with_extension("progress"), i.to_string()).unwrap();
        }
        std::thread::sleep(Duration::from_secs(15));
        std::process::exit(90);
    }
    if args.len() == 3 && args[1] == "TEST_ONLY_SLEEP" {
        std::fs::write(&args[2], b"entered").unwrap();
        std::thread::sleep(Duration::from_secs(10));
        return;
    }
    if !(6..=8).contains(&args.len()) || args[1] != "TEST_ONLY_ECHO" {
        std::process::exit(64);
    }
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(10));
        std::process::exit(90);
    });
    if args.len() >= 7 {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        let unrelated = if args.len() == 8 {
            assert_eq!(args[7], "TEST_ONLY_UNRELATED_EVENT");
            let raw =
                unsafe { windows::Win32::System::Threading::CreateEventW(None, true, false, None) }
                    .unwrap();
            Some(unsafe { OwnedHandle::from_raw_handle(raw.0) })
        } else {
            None
        };
        let value = unrelated
            .as_ref()
            .map(|h| h.as_raw_handle() as usize)
            .unwrap_or_else(|| args[6].to_str().unwrap().parse::<usize>().unwrap());
        // A child may reuse the numeric value for another object. Only signaling
        // the original event in the parent establishes leaked object identity.
        let result = unsafe {
            windows::Win32::System::Threading::SetEvent(windows::Win32::Foundation::HANDLE(
                value as *mut _,
            ))
        };
        if unrelated.is_some() {
            assert!(result.is_ok(), "owned unrelated-event negative control");
        }
    }
    let marker = std::path::PathBuf::from(&args[2]);
    let cwd_ok = std::env::current_dir().unwrap().canonicalize().unwrap()
        == marker.parent().unwrap().canonicalize().unwrap();
    let argv_ok =
        args[3].is_empty() && args[4] == "中文🚀" && args[5] == "space quote\" trailing\\";
    let environment_empty = std::env::vars_os().next().is_none();
    let mut data = Vec::new();
    std::io::stdin().take(4097).read_to_end(&mut data).unwrap();
    if data.len() > 4096 {
        std::process::exit(65);
    }
    assert!(cwd_ok && argv_ok && environment_empty);
    std::fs::write(marker, b"executed").unwrap();
    std::io::stdout().write_all(&data).unwrap();
    std::io::stderr()
        .write_all(b"PUBLIC_TEST_ONLY_STDERR")
        .unwrap();
}
#[cfg(not(all(windows, feature = "conpty-qualification")))]
fn main() {
    std::process::exit(64);
}
