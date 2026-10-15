// 一次性诊断：按生产项目列表统计两个 CLI 的实际行数与归档数。勿提交。
use super::*;
use crate::cli::native_projection::scoped_fs::Limits;

fn report(label: &str, cli: CliKind, root_path: &str, cwds: &[String]) {
    let root = match Root::open(&PathBuf::from(root_path)) {
        Ok(root) => root,
        Err(code) => {
            println!("DIAG {label} ROOT_FAILED {code}");
            return;
        }
    };
    let owned: Vec<(String, Vec<PathBuf>)> = cwds
        .iter()
        .enumerate()
        .map(|(i, p)| (format!("p{i}"), vec![PathBuf::from(p)]))
        .collect();
    let granted: Vec<ProjectRef> = owned
        .iter()
        .map(|(id, paths)| ProjectRef { id, root: &root, paths })
        .collect();
    let catalog = Catalog {
        cli,
        root: &root,
        projects: &granted,
        user_config: None,
        check: &|| Ok(()),
    };
    let mut budget = Budget::new(Limits::default());
    match read(
        &catalog,
        &Options { kind: ResourceKind::History, query: None, session_id: None },
        &mut budget,
    ) {
        Ok(items) => {
            let archived = items
                .iter()
                .filter(|i| matches!(i, ResourceItem::Session { archived: true, .. }))
                .count();
            println!(
                "DIAG {label} total={} archived={} visible={} failures={:?}",
                items.len(),
                archived,
                items.len() - archived,
                budget.history_read_failures()
            );
            let mut per: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
            for i in &items {
                if let ResourceItem::Session { project_id, archived, .. } = i {
                    let e = per.entry(project_id.as_deref().unwrap_or("?")).or_default();
                    e.0 += 1;
                    if *archived {
                        e.1 += 1;
                    }
                }
            }
            for (id, (rows, arch)) in per {
                let idx: usize = id.trim_start_matches('p').parse().unwrap_or(0);
                println!("DIAG   {:<44} rows={rows:3} archived={arch:3}", cwds.get(idx).map(String::as_str).unwrap_or(id));
            }
        }
        Err(code) => println!("DIAG {label} READ_FAILED {code}"),
    }
}

#[test]
fn Diag_RealRowsPerCli() {
    let cwds: Vec<String> = vec![
        r"E:\qgs\qgs".into(),
        r"E:\source\DataTool".into(),
        r"E:\source\repos\aigm".into(),
        r"E:\source\repos\family-finance-os".into(),
        r"E:\source\repos\game_asset".into(),
        r"E:\source\repos\msai".into(),
        r"E:\source\repos\wsvdr".into(),
        r"e:\source\github\cc-box".into(),
        r"E:\source\github\cc-box".into(),
        r"f:\downloads\vbs".into(),
    ];
    report("CLAUDE", CliKind::Claude, "C:/Users/wusha/.claude", &cwds);
    report("CODEX", CliKind::Codex, "C:/Users/wusha/.codex", &cwds);
}
