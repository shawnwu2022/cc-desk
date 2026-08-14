//! Phase 0 / benchmark 基础设施：首页/会话名称多变体 profiling + 真实数据 benchmark。
//! 从 store.rs 迁出（测试编写原则 §11：测试代码独立存放）。仅 #[cfg(test)] 编译。

#![allow(dead_code)]

use crate::store::{
    assemble_home_data, extract_session_name, get_claude_dir, get_project_last_modified,
    scan_home_projects_at, session_entry_from_path, HomeData, HomeProjectScan, Project,
    ProjectPathMapping, SessionInfo, PROJECT_PATH_MAPPING,
};
use anyhow::{bail, Result};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum HomeProfileVariant {
    LegacyDoubleScan,
    SnapshotLegacyName,
    SnapshotStreamName,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct HomeStageTimings {
    pub mapping_lock_ms: f64,
    pub project_scan_ms: f64,
    pub metadata_ms: f64,
    pub sort_page_ms: f64,
    pub name_parse_ms: f64,
    pub assemble_ms: f64,
    pub total_ms: f64,
    pub project_files: usize,
    pub session_files: usize,
    pub name_files: usize,
    pub name_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct HomeProfileReport {
    pub rounds: usize,
    pub variants_are_equivalent: bool,
    pub samples: BTreeMap<HomeProfileVariant, Vec<HomeStageTimings>>,
    pub p50: BTreeMap<HomeProfileVariant, HomeStageTimings>,
    pub snapshot_overhead_ms: f64,
    pub snapshot_overhead_percent: f64,
    pub stream_overhead_ms: f64,
    pub stream_overhead_percent: f64,
    pub residual_p50_ms: f64,
    pub final_warm_limit_ms: f64,
    pub phase0a_should_stop: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveFullScanSample {
    pub k: usize,
    pub name_parse_ms: f64,
    pub jsonl_bytes_read: u64,
}

#[derive(Debug)]
pub(crate) struct ActiveFullScanReport {
    pub rounds: usize,
    pub source_file_sizes: Vec<u64>,
    pub samples: BTreeMap<usize, Vec<ActiveFullScanSample>>,
    pub p50_name_parse_ms: BTreeMap<usize, f64>,
    pub p50_jsonl_bytes_read: BTreeMap<usize, u64>,
    pub estimated_active_total_p50_ms: BTreeMap<usize, f64>,
    pub active4_limit_ms: f64,
    pub append_resume_required: bool,
}

pub(crate) fn extract_project_path_legacy(project_dir: &Path) -> Option<String> {
    for entry in fs::read_dir(project_dir).ok()? {
        let path = entry.ok()?.path();
        let name = path.file_name()?.to_string_lossy();
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if (ext != "jsonl" && ext != "txt") || name.starts_with("agent-") {
            continue;
        }
        let content = fs::read_to_string(path).ok()?;
        for line in content.lines() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if let Some(cwd) = value.get("cwd").and_then(|item| item.as_str()) {
                return Some(cwd.to_string());
            }
        }
    }
    None
}

fn scan_home_projects_legacy_at(projects_dir: &Path) -> Result<HomeProjectScan> {
    if !projects_dir.exists() {
        return Ok(HomeProjectScan {
            projects: Vec::new(),
            mapping: ProjectPathMapping::new(),
        });
    }

    let mut projects = Vec::new();
    let mut mapping = ProjectPathMapping::new();

    for entry in fs::read_dir(projects_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let Some(real_path) = extract_project_path_legacy(&path) else {
            continue;
        };
        mapping
            .entry(real_path.clone())
            .or_default()
            .push(path.clone());

        if !Path::new(&real_path).exists() {
            continue;
        }

        projects.push(Project {
            path: real_path.clone(),
            name: Path::new(&real_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&real_path)
                .to_string(),
            last_session_id: None,
            last_cost: None,
            last_duration: Some(get_project_last_modified(&path)),
        });
    }

    Ok(HomeProjectScan { projects, mapping })
}

pub(crate) fn extract_session_name_legacy(path: &Path) -> String {
    let Ok(content) = fs::read_to_string(path) else {
        return "Unnamed session".to_string();
    };
    let mut custom_title = None;
    let mut first_user_message = None;
    for line in content.lines() {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        crate::session_name_index::apply_session_name_value(
            &json,
            &mut custom_title,
            &mut first_user_message,
        );
    }
    custom_title
        .or(first_user_message)
        .unwrap_or_else(|| "Unnamed session".to_string())
}

fn profile_project_scan_at(
    projects_dir: &Path,
    variant: HomeProfileVariant,
    timings: &mut HomeStageTimings,
) -> Result<HomeProjectScan> {
    if variant == HomeProfileVariant::LegacyDoubleScan {
        let first_started = Instant::now();
        let first = scan_home_projects_legacy_at(projects_dir)?;
        timings.project_scan_ms += first_started.elapsed().as_secs_f64() * 1000.0;
        timings.project_files += first.mapping.values().map(Vec::len).sum::<usize>();

        let lock_started = Instant::now();
        let mut cache = PROJECT_PATH_MAPPING
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        timings.mapping_lock_ms += lock_started.elapsed().as_secs_f64() * 1000.0;

        let second_started = Instant::now();
        let second = scan_home_projects_legacy_at(projects_dir)?;
        timings.project_scan_ms += second_started.elapsed().as_secs_f64() * 1000.0;
        timings.project_files += second.mapping.values().map(Vec::len).sum::<usize>();
        *cache = Some(second.mapping.clone());

        return Ok(HomeProjectScan {
            projects: first.projects,
            mapping: second.mapping,
        });
    }

    let lock_started = Instant::now();
    let mut cache = PROJECT_PATH_MAPPING
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    timings.mapping_lock_ms += lock_started.elapsed().as_secs_f64() * 1000.0;

    let scan_started = Instant::now();
    let scan = scan_home_projects_at(projects_dir)?;
    timings.project_scan_ms += scan_started.elapsed().as_secs_f64() * 1000.0;
    timings.project_files += scan.mapping.values().map(Vec::len).sum::<usize>();
    *cache = Some(scan.mapping.clone());
    Ok(scan)
}

fn profile_session_page(
    project_path: &str,
    project_dirs: &[PathBuf],
    variant: HomeProfileVariant,
    timings: &mut HomeStageTimings,
) -> Result<Vec<SessionInfo>> {
    let metadata_started = Instant::now();
    let mut entries = Vec::new();
    for project_dir in project_dirs {
        if !project_dir.exists() {
            continue;
        }
        for entry in fs::read_dir(project_dir)? {
            let entry = entry?;
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if (ext != "jsonl" && ext != "txt")
                || path
                    .file_name()
                    .map(|name| name.to_string_lossy().starts_with("agent-"))
                    .unwrap_or(false)
            {
                continue;
            }

            entries.push(session_entry_from_path(project_dir, path));
        }
    }
    timings.metadata_ms += metadata_started.elapsed().as_secs_f64() * 1000.0;
    timings.session_files += entries.len();

    let sort_started = Instant::now();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.last_active_at));
    entries.truncate(3);
    timings.sort_page_ms += sort_started.elapsed().as_secs_f64() * 1000.0;

    let name_started = Instant::now();
    let sessions = entries
        .into_iter()
        .map(|entry| {
            timings.name_files += 1;
            timings.name_bytes += fs::metadata(&entry.path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            let name = match variant {
                HomeProfileVariant::LegacyDoubleScan | HomeProfileVariant::SnapshotLegacyName => {
                    extract_session_name_legacy(&entry.path)
                }
                HomeProfileVariant::SnapshotStreamName => extract_session_name(&entry.path),
            };
            SessionInfo {
                session_id: entry.session_id,
                name,
                project_path: project_path.to_string(),
                last_active_at: entry.last_active_at,
            }
        })
        .collect();
    timings.name_parse_ms += name_started.elapsed().as_secs_f64() * 1000.0;
    Ok(sessions)
}

fn profile_home_data_at(
    projects_dir: &Path,
    variant: HomeProfileVariant,
) -> Result<(HomeData, HomeStageTimings)> {
    let total_started = Instant::now();
    let mut timings = HomeStageTimings::default();
    let scan = profile_project_scan_at(projects_dir, variant, &mut timings)?;

    let mut all_sessions = Vec::new();
    let mut sessions_by_path: HashMap<String, Vec<SessionInfo>> = HashMap::new();
    for project in &scan.projects {
        if !sessions_by_path.contains_key(&project.path) {
            let sessions = match scan.mapping.get(&project.path) {
                Some(project_dirs) => {
                    profile_session_page(&project.path, project_dirs, variant, &mut timings)
                        .unwrap_or_default()
                }
                None => Vec::new(),
            };
            sessions_by_path.insert(project.path.clone(), sessions);
        }
        if let Some(sessions) = sessions_by_path.get(&project.path) {
            all_sessions.extend(sessions.clone());
        }
    }

    let assemble_started = Instant::now();
    let home = assemble_home_data(scan.projects, all_sessions, "", &[], 12, 20);
    timings.assemble_ms = assemble_started.elapsed().as_secs_f64() * 1000.0;
    timings.total_ms = total_started.elapsed().as_secs_f64() * 1000.0;
    Ok((home, timings))
}

fn normalize_home_profile(home: &HomeData) -> (Vec<String>, Vec<(String, String, String, u64)>) {
    let projects = home
        .projects
        .iter()
        .map(|project| project.path.clone())
        .collect();
    let sessions = home
        .recent_sessions
        .iter()
        .map(|session| {
            (
                session.project_path.clone(),
                session.session_id.clone(),
                session.name.clone(),
                session.last_active_at,
            )
        })
        .collect();
    (projects, sessions)
}

fn profile_p50(samples: &[HomeStageTimings]) -> HomeStageTimings {
    fn median(mut values: Vec<f64>) -> f64 {
        values.sort_by(f64::total_cmp);
        values[values.len() / 2]
    }
    fn median_usize(mut values: Vec<usize>) -> usize {
        values.sort_unstable();
        values[values.len() / 2]
    }
    fn median_u64(mut values: Vec<u64>) -> u64 {
        values.sort_unstable();
        values[values.len() / 2]
    }

    HomeStageTimings {
        mapping_lock_ms: median(
            samples
                .iter()
                .map(|sample| sample.mapping_lock_ms)
                .collect(),
        ),
        project_scan_ms: median(
            samples
                .iter()
                .map(|sample| sample.project_scan_ms)
                .collect(),
        ),
        metadata_ms: median(samples.iter().map(|sample| sample.metadata_ms).collect()),
        sort_page_ms: median(samples.iter().map(|sample| sample.sort_page_ms).collect()),
        name_parse_ms: median(samples.iter().map(|sample| sample.name_parse_ms).collect()),
        assemble_ms: median(samples.iter().map(|sample| sample.assemble_ms).collect()),
        total_ms: median(samples.iter().map(|sample| sample.total_ms).collect()),
        project_files: median_usize(samples.iter().map(|sample| sample.project_files).collect()),
        session_files: median_usize(samples.iter().map(|sample| sample.session_files).collect()),
        name_files: median_usize(samples.iter().map(|sample| sample.name_files).collect()),
        name_bytes: median_u64(samples.iter().map(|sample| sample.name_bytes).collect()),
    }
}

pub(crate) fn profile_real_home_variants(rounds: usize) -> Result<HomeProfileReport> {
    if rounds == 0 {
        bail!("profile rounds must be greater than zero");
    }
    let projects_dir = get_claude_dir()?.join("projects");
    let orders = [
        [
            HomeProfileVariant::LegacyDoubleScan,
            HomeProfileVariant::SnapshotLegacyName,
            HomeProfileVariant::SnapshotStreamName,
        ],
        [
            HomeProfileVariant::SnapshotLegacyName,
            HomeProfileVariant::SnapshotStreamName,
            HomeProfileVariant::LegacyDoubleScan,
        ],
        [
            HomeProfileVariant::SnapshotStreamName,
            HomeProfileVariant::LegacyDoubleScan,
            HomeProfileVariant::SnapshotLegacyName,
        ],
    ];
    let mut samples: BTreeMap<HomeProfileVariant, Vec<HomeStageTimings>> = BTreeMap::new();

    for round in 0..rounds {
        let mut expected = None;
        for variant in orders[round % orders.len()] {
            let (home, timings) = profile_home_data_at(&projects_dir, variant)?;
            if home.projects.is_empty() || timings.project_files == 0 || timings.name_files == 0 {
                bail!(
                    "real home benchmark found no eligible project/session data under {:?}",
                    projects_dir
                );
            }
            let normalized = normalize_home_profile(&home);
            if let Some(expected) = &expected {
                if expected != &normalized {
                    bail!(
                        "profile variants changed business output during round {} at {:?}",
                        round + 1,
                        variant
                    );
                }
            } else {
                expected = Some(normalized);
            }
            samples.entry(variant).or_default().push(timings);
        }
    }

    let p50 = samples
        .iter()
        .map(|(variant, variant_samples)| (*variant, profile_p50(variant_samples)))
        .collect::<BTreeMap<_, _>>();
    let legacy_total = p50[&HomeProfileVariant::LegacyDoubleScan].total_ms;
    let snapshot_total = p50[&HomeProfileVariant::SnapshotLegacyName].total_ms;
    let stream_total = p50[&HomeProfileVariant::SnapshotStreamName].total_ms;
    let stream_name_parse = p50[&HomeProfileVariant::SnapshotStreamName].name_parse_ms;
    let snapshot_overhead_ms = snapshot_total - legacy_total;
    let stream_overhead_ms = stream_total - snapshot_total;
    let snapshot_overhead_percent = if legacy_total > 0.0 {
        snapshot_overhead_ms / legacy_total * 100.0
    } else {
        0.0
    };
    let stream_overhead_percent = if snapshot_total > 0.0 {
        stream_overhead_ms / snapshot_total * 100.0
    } else {
        0.0
    };
    let residual_p50_ms = (stream_total - stream_name_parse).max(0.0);
    let final_warm_limit_ms = ((residual_p50_ms * 1.5 / 25.0).ceil() * 25.0).clamp(250.0, 500.0);
    let phase0a_should_stop = (snapshot_overhead_ms > 100.0 && snapshot_overhead_percent > 5.0)
        || (stream_overhead_ms > 100.0 && stream_overhead_percent > 5.0)
        || residual_p50_ms > 350.0;

    Ok(HomeProfileReport {
        rounds,
        variants_are_equivalent: true,
        samples,
        p50,
        snapshot_overhead_ms,
        snapshot_overhead_percent,
        stream_overhead_ms,
        stream_overhead_percent,
        residual_p50_ms,
        final_warm_limit_ms,
        phase0a_should_stop,
    })
}

pub(crate) fn benchmark_active_full_scan_real(
    rounds: usize,
    residual_p50_ms: f64,
    final_warm_limit_ms: f64,
) -> Result<ActiveFullScanReport> {
    if rounds == 0 {
        bail!("active full-scan rounds must be greater than zero");
    }

    struct SourceFile {
        path: PathBuf,
        modified_nanos: u128,
        size: u64,
    }

    let projects_dir = get_claude_dir()?.join("projects");
    let mut sources = Vec::new();
    for project_entry in fs::read_dir(&projects_dir)? {
        let project_dir = project_entry?.path();
        if !project_dir.is_dir() {
            continue;
        }
        for file_entry in fs::read_dir(project_dir)? {
            let path = file_entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl")
                || path
                    .file_name()
                    .map(|name| name.to_string_lossy().starts_with("agent-"))
                    .unwrap_or(false)
            {
                continue;
            }
            let metadata = fs::metadata(&path)?;
            let modified_nanos = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            sources.push(SourceFile {
                path,
                modified_nanos,
                size: metadata.len(),
            });
        }
    }
    sources.sort_by_key(|source| std::cmp::Reverse(source.modified_nanos));
    if sources.len() < 8 {
        bail!(
            "active full-scan benchmark requires at least 8 eligible JSONL files, found {}",
            sources.len()
        );
    }
    sources.truncate(8);
    let source_file_sizes = sources.iter().map(|source| source.size).collect::<Vec<_>>();

    let temp = tempfile::tempdir()?;
    let mut copies: BTreeMap<usize, Vec<PathBuf>> = BTreeMap::new();
    for k in [1usize, 4, 8] {
        let group = temp.path().join(format!("active-{k}"));
        fs::create_dir_all(&group)?;
        let mut paths = Vec::with_capacity(k);
        for (index, source) in sources.iter().take(k).enumerate() {
            let destination = group.join(format!("{index:02}.jsonl"));
            fs::copy(&source.path, &destination)?;
            paths.push(destination);
        }
        copies.insert(k, paths);
    }

    const APPENDED_EVENT: &[u8] = b"\n{\"type\":\"assistant\",\"message\":{\"content\":[]}}\n";
    let orders = [[1usize, 4, 8], [4usize, 8, 1], [8usize, 1, 4]];
    let mut samples: BTreeMap<usize, Vec<ActiveFullScanSample>> = BTreeMap::new();
    for round in 0..rounds {
        for k in orders[round % orders.len()] {
            let paths = &copies[&k];
            for path in paths {
                fs::OpenOptions::new()
                    .append(true)
                    .open(path)?
                    .write_all(APPENDED_EVENT)?;
            }

            let jsonl_bytes_read = paths.iter().try_fold(0u64, |total, path| {
                fs::metadata(path).map(|metadata| total.saturating_add(metadata.len()))
            })?;
            let started = Instant::now();
            for path in paths {
                std::hint::black_box(extract_session_name(path));
            }
            samples.entry(k).or_default().push(ActiveFullScanSample {
                k,
                name_parse_ms: started.elapsed().as_secs_f64() * 1000.0,
                jsonl_bytes_read,
            });
        }
    }

    let p50_name_parse_ms = samples
        .iter()
        .map(|(k, values)| {
            let mut timings = values
                .iter()
                .map(|sample| sample.name_parse_ms)
                .collect::<Vec<_>>();
            timings.sort_by(f64::total_cmp);
            (*k, timings[timings.len() / 2])
        })
        .collect::<BTreeMap<_, _>>();
    let p50_jsonl_bytes_read = samples
        .iter()
        .map(|(k, values)| {
            let mut bytes = values
                .iter()
                .map(|sample| sample.jsonl_bytes_read)
                .collect::<Vec<_>>();
            bytes.sort_unstable();
            (*k, bytes[bytes.len() / 2])
        })
        .collect::<BTreeMap<_, _>>();
    let estimated_active_total_p50_ms = p50_name_parse_ms
        .iter()
        .map(|(k, name_parse_ms)| (*k, residual_p50_ms + name_parse_ms))
        .collect::<BTreeMap<_, _>>();
    let active4_limit_ms = 500.0f64.min(final_warm_limit_ms + 100.0);
    let append_resume_required = !(estimated_active_total_p50_ms[&4] <= active4_limit_ms
        && estimated_active_total_p50_ms[&8] <= 500.0);

    Ok(ActiveFullScanReport {
        rounds,
        source_file_sizes,
        samples,
        p50_name_parse_ms,
        p50_jsonl_bytes_read,
        estimated_active_total_p50_ms,
        active4_limit_ms,
        append_resume_required,
    })
}
