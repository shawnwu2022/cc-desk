//! 固定公开字节的测试子进程；不作为 Host 发布。
use std::{
    io::{BufRead, Read, Write},
    time::Duration,
};
fn main() {
    let a: Vec<_> = std::env::args_os().collect();
    if a.len() != 3 || a[1] != "TEST_ONLY_PIPE" {
        std::process::exit(64);
    }
    // 测试父进程意外退出的有限后备；不是产品后台策略。
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(15));
        std::process::exit(90);
    });
    let mut input = std::io::stdin().lock();
    let mut line = Vec::new();
    loop {
        line.clear();
        let mut limited = (&mut input).take(16385);
        let n = limited.read_until(b'\n', &mut line).unwrap();
        if n == 0 {
            return;
        }
        if n > 16384 || line.last() != Some(&b'\n') {
            std::process::exit(65);
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&a[2])
            .unwrap();
        f.write_all(b"x").unwrap();
        f.sync_all().unwrap();
    }
}
