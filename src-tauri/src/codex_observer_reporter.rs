use crate::codex_observer::{metadata, ENV_NAMES};
use std::{io::Read, time::Duration};

/// 在任何桌面、日志或安装状态初始化之前运行；失败静默且不影响 CLI hook 决策。
pub fn run_entry() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    if args
        .get(1)
        .is_none_or(|arg| arg != "--cc-desk-codex-observer")
    {
        return false;
    }
    if args.len() == 2 {
        let _ = report();
    }
    true
}

fn report() -> Result<(), ()> {
    let get = |key| std::env::var(key).map_err(|_| ());
    let port = get(ENV_NAMES[0])?
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or(())?;
    let run = get(ENV_NAMES[1])?;
    let generation = get(ENV_NAMES[2])?;
    let capability = get(ENV_NAMES[3])?;
    let mut headers = axum::http::HeaderMap::new();
    for (name, value) in [
        ("x-cc-desk-run", run),
        ("x-cc-desk-generation", generation),
        ("x-cc-desk-capability", capability),
        (
            "x-cc-desk-event",
            crate::codex_observer::event_id().map_err(|_| ())?,
        ),
        ("x-cc-desk-observer-source", "codex-hook".into()),
    ] {
        headers.insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).map_err(|_| ())?,
            value.parse().map_err(|_| ())?,
        );
    }
    crate::observer_registry::parse_observer_headers(&headers).map_err(|_| ())?;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(crate::observer_registry::MAX_OBSERVER_PAYLOAD as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > crate::observer_registry::MAX_OBSERVER_PAYLOAD {
        return Err(());
    }
    let input = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let safe = metadata(&input).map_err(|_| ())?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|_| ())?;
    let request = client
        .post(format!("http://127.0.0.1:{port}/observer"))
        .json(&safe);
    let mut request = request;
    for (name, value) in &headers {
        request = request.header(name.as_str(), value.as_bytes());
    }
    request.send().map_err(|_| ())?;
    Ok(())
}
