use std::{
    env,
    path::Path,
    process::{Command, ExitCode},
};

fn checked_args(args: &[String]) -> Result<Vec<String>, &'static str> {
    if args.first().map(String::as_str) != Some("build") {
        return Err("only the trial build is supported");
    }
    let mut remote_host = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--release" | "--verbose" | "-v" | "--lib" | "--bins" => {}
            "--bin" | "--features" | "--color" | "--target" => {
                let key = args[i].as_str();
                i += 1;
                let value = args.get(i).ok_or("missing argument value")?;
                match key {
                    "--bin" if value == "cc-desk" => {}
                    "--target" if value == "x86_64-pc-windows-msvc" => {}
                    "--color" if matches!(value.as_str(), "auto" | "always" | "never") => {}
                    "--features" => {
                        for feature in value.split(',') {
                            match feature {
                                "remote-host" => remote_host = true,
                                "tauri/custom-protocol" => {}
                                _ => return Err("unapproved feature"),
                            }
                        }
                    }
                    _ => return Err("unapproved argument value"),
                }
            }
            _ => return Err("unapproved build argument"),
        }
        i += 1;
    }
    if !remote_host {
        return Err("remote-host must be explicitly selected");
    }
    let mut result = args.to_vec();
    result.extend([
        "--offline".into(),
        "--locked".into(),
        "--manifest-path".into(),
        Path::new(env!("CC_DESK_TRIAL_BUILD_ROOT"))
            .join("src-tauri/Cargo.toml")
            .to_string_lossy()
            .into_owned(),
    ]);
    Ok(result)
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    let args = match checked_args(&args) {
        Ok(args) => args,
        Err(reason) => {
            eprintln!("remote-trial runner: {reason}; arguments={args:?}");
            return ExitCode::FAILURE;
        }
    };
    // 固定根目录与 manifest，显式使用根目录缓存而不修改子目录镜像配置。
    let root = env!("CC_DESK_TRIAL_BUILD_ROOT");
    eprintln!("remote-trial runner cwd={root}; manifest=src-tauri/Cargo.toml; offline+locked");
    match Command::new(env!("CC_DESK_TRIAL_BUILD_CARGO"))
        .current_dir(root)
        .args(args)
        .env("CARGO_NET_OFFLINE", "true")
        .status()
    {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) | Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::checked_args;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn explicit_trial_is_locked() {
        let result = checked_args(&args(&[
            "build",
            "--features",
            "remote-host,tauri/custom-protocol",
            "--bin",
            "cc-desk",
        ]))
        .unwrap();
        assert!(result.iter().any(|x| x == "--offline"));
        assert!(result.iter().any(|x| x == "--locked"));
    }
    #[test]
    fn source_and_feature_overrides_are_rejected() {
        for values in [
            vec!["build"],
            vec!["build", "--features", "other"],
            vec!["build", "--config", "evil"],
            vec!["build", "--manifest-path", "other"],
            vec!["run"],
        ] {
            assert!(checked_args(&args(&values)).is_err());
        }
    }
}
