use std::{
    io::Write,
    process::{Command, Stdio},
    time::Duration,
};
fn main() {
    let mut args = std::env::args_os().skip(1);
    let role = args.next().expect("fixture role");
    if role == "--exit" {
        return;
    }
    if role == "--grandchild" {
        let marker = args.next().expect("fixture marker");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--idle")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        std::fs::write(marker, child.id().to_string()).unwrap();
        let _ = child.wait();
        return;
    }
    if role == "--write" {
        std::fs::write(args.next().expect("fixture marker"), b"ready").unwrap();
        let mut stdout = std::io::stdout().lock();
        for _ in 0..1024 {
            stdout.write_all(&[0x78; 16384]).unwrap();
        }
        return;
    }
    if role == "--marker" || role == "--marker-alive" {
        std::fs::write(args.next().expect("fixture marker"), b"executed").unwrap();
    }
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
