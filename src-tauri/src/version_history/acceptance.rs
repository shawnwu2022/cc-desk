//! Disposable-target acceptance only. No CLI, IPC, environment or downloaded
//! input can activate this binding. The committed binding always denies.
use crate::cli::{profiles::error, types::SafeError};
use serde::{Deserialize, Serialize};

const COMPILED_TARGET: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/fixtures/version-history-roundtrip/target.json"
));
const BASE_HEAD: &str = "9c981a5093a80b947817af8eebe4855293690185";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AcceptanceScenario {
    Success,
    BeforeInstallerResume,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TargetBinding {
    schema: u32,
    enabled: bool,
    base_head: String,
    build_id: String,
    run_id: String,
    scenario: AcceptanceScenario,
    target_sid: String,
    profile_directory: String,
    install_directory: String,
    evidence_directory: String,
}
pub(crate) fn require_payload(version: &str, digest: &str, size: u64) -> Result<(), SafeError> {
    if version != "0.17.7"
        || digest != "e9ffbc5ba627f0c133a4385db404342a7344729339185e6f9b8ee6b5969086ac"
        || size != 4_966_193
    {
        return Err(denied());
    }
    Ok(())
}
fn denied() -> SafeError {
    error("HISTORY_ACCEPTANCE_TARGET_DENIED")
}
fn bound_target() -> Result<TargetBinding, SafeError> {
    let target: TargetBinding = serde_json::from_str(COMPILED_TARGET).map_err(|_| denied())?;
    target.validate()?;
    Ok(target)
}
#[cfg(test)]
pub(crate) fn validate_target_bytes(bytes: &str) -> Result<(), SafeError> {
    serde_json::from_str::<TargetBinding>(bytes)
        .map_err(|_| denied())?
        .validate()
}
#[derive(Clone, Copy, Serialize)]
pub(crate) enum AcceptanceStage {
    M0,
    SourceSealed,
    FreshReady,
    InstallerSuspended,
    InjectedPreResumeFailure,
    CancelledBeforeResume,
    TargetVerified,
    HistoricalLaunched,
    LaterCaptured,
    FinalRestored,
}
impl TargetBinding {
    fn validate(&self) -> Result<(), SafeError> {
        let digest = |value: &str| {
            value.len() == 40
                && value
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        };
        if self.schema != 1
            || !self.enabled
            || self.base_head != BASE_HEAD
            || !digest(&self.build_id)
            || self.build_id == BASE_HEAD
            || uuid::Uuid::parse_str(&self.run_id).is_err()
            || !self.target_sid.starts_with("S-1-5-21-")
            || !self
                .target_sid
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'S' | b'-'))
            || [
                &self.profile_directory,
                &self.install_directory,
                &self.evidence_directory,
            ]
            .iter()
            .any(|path| !absolute_drive_path(path))
            || self
                .install_directory
                .eq_ignore_ascii_case(&self.evidence_directory)
        {
            return Err(denied());
        }
        Ok(())
    }
}
fn absolute_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() > 3
        && bytes.len() < 30_000
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..3] == *b":\\"
        && !path.ends_with('\\')
        && path[3..].split('\\').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !part
                    .chars()
                    .any(|c| c.is_control() || "/:*?\"<>|".contains(c))
        })
}

#[cfg(all(feature = "history-roundtrip-acceptance", not(test), windows))]
mod native {
    use super::*;
    use crate::version_history::windows::{
        files::{ComponentName, Directory, PrivateDirectory},
        scope::{ConfiguredInventory, ObservedPath, RegisteredInstallation},
        security::CurrentUser,
    };
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::Path};
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Profile, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
    };

    fn profile_directory() -> Result<std::sync::Arc<Directory>, SafeError> {
        let value = unsafe { SHGetKnownFolderPath(&FOLDERID_Profile, KF_FLAG_DEFAULT, None) }
            .map_err(|_| denied())?;
        struct Allocation(windows_core::PWSTR);
        impl Drop for Allocation {
            fn drop(&mut self) {
                unsafe {
                    CoTaskMemFree(Some(self.0 .0.cast()));
                }
            }
        }
        let allocation = Allocation(value);
        if allocation.0 .0.is_null() {
            return Err(denied());
        }
        let mut length = 0;
        while length < 32768 && unsafe { *allocation.0 .0.add(length) } != 0 {
            length += 1;
        }
        if length == 0 || length == 32768 {
            return Err(denied());
        }
        let path =
            OsString::from_wide(unsafe { std::slice::from_raw_parts(allocation.0 .0, length) });
        Directory::open_absolute(Path::new(&path)).map_err(|_| denied())
    }
    fn private_evidence(
        target: &TargetBinding,
        user: &CurrentUser,
    ) -> Result<PrivateDirectory, SafeError> {
        let path = Path::new(&target.evidence_directory);
        let parent =
            Directory::open_absolute(path.parent().ok_or_else(denied)?).map_err(|_| denied())?;
        let leaf =
            ComponentName::new(path.file_name().ok_or_else(denied)?).map_err(|_| denied())?;
        PrivateDirectory::open_existing(parent, leaf, user).map_err(|_| denied())
    }
    pub(crate) fn evidence_observation() -> Result<ObservedPath, SafeError> {
        let target = bound_target()?;
        let user = CurrentUser::capture().map_err(|_| denied())?;
        user.require_unelevated().map_err(|_| denied())?;
        if user.sid_text() != target.target_sid {
            return Err(denied());
        }
        let evidence = private_evidence(&target, &user)?;
        ObservedPath::from_directory(evidence.directory().clone()).map_err(|_| denied())
    }
    pub(crate) fn require_source_target(
        selection: &crate::version_history::catalog::SelectionMetadata,
    ) -> Result<(), SafeError> {
        require_payload(
            selection.version(),
            selection.installer().sha256(),
            selection.installer().size(),
        )?;
        let target = bound_target()?;
        if tauri::is_dev()
            || env!("CARGO_PKG_VERSION") != "0.18.0"
            || target.build_id != env!("CC_DESK_BUILD_SHA")
        {
            return Err(denied());
        }
        let user = CurrentUser::capture().map_err(|_| denied())?;
        user.require_unelevated().map_err(|_| denied())?;
        if user.sid_text() != target.target_sid {
            return Err(denied());
        }
        crate::version_history::windows::manager_process::require_job_free_source()
            .map_err(|_| denied())?;
        let profile = profile_directory()?;
        let expected_profile =
            Directory::open_absolute(Path::new(&target.profile_directory)).map_err(|_| denied())?;
        let expected_install =
            Directory::open_absolute(Path::new(&target.install_directory)).map_err(|_| denied())?;
        let installed = RegisteredInstallation::capture().map_err(|_| denied())?;
        if profile.identity() != expected_profile.identity()
            || installed.directory().identity() != expected_install.identity()
            || std::env::current_exe().map_err(|_| denied())?
                != Path::new(&target.install_directory).join("cc-desk.exe")
        {
            return Err(denied());
        }
        let evidence = evidence_observation()?;
        let install =
            ObservedPath::from_directory(expected_install.clone()).map_err(|_| denied())?;
        ConfiguredInventory::capture()
            .map_err(|_| denied())?
            .require_disjoint(&[&install, &evidence])
            .map_err(|_| denied())?;
        installed.recheck().map_err(|_| denied())?;
        profile.recheck().map_err(|_| denied())?;
        expected_profile.recheck().map_err(|_| denied())?;
        expected_install.recheck().map_err(|_| denied())
    }

    pub(crate) fn scenario() -> AcceptanceScenario {
        bound_target()
            .map(|target| target.scenario)
            .unwrap_or(AcceptanceScenario::Success)
    }
    pub(crate) fn before_installer_resume() -> Result<(), SafeError> {
        if scenario() == AcceptanceScenario::BeforeInstallerResume {
            return Err(error("HISTORY_ACCEPTANCE_INJECTED_PRE_RESUME"));
        }
        Ok(())
    }

    static SINK_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    fn contain_passive(operation: impl FnOnce() -> Result<(), SafeError>) {
        use std::sync::atomic::Ordering;
        if SINK_FAILED.load(Ordering::Acquire) {
            return;
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
        if !matches!(result, Ok(Ok(()))) {
            // A missing or failed M0 never lets later phases write after the
            // original configuration has been rotated out of its ordinary slot.
            SINK_FAILED.store(true, Ordering::Release);
        }
    }
    /// Uses the coordinator's already captured exact M0/later scope. Refusal
    /// poisons reporting only and cannot reject or authorize a native effect.
    pub(crate) fn check_evidence_scope(
        exclusions: Option<&crate::version_history::windows::scope::ConfiguredExclusions>,
    ) {
        contain_passive(|| {
            let target = bound_target()?;
            let user = CurrentUser::capture().map_err(|_| denied())?;
            let evidence = private_evidence(&target, &user)?;
            let evidence =
                ObservedPath::from_directory(evidence.directory().clone()).map_err(|_| denied())?;
            exclusions
                .ok_or_else(denied)?
                .acceptance_require_disjoint(&evidence)
                .map_err(|_| denied())
        });
    }
    /// Every fallible collection, conversion and write stays inside this sink.
    /// Missing/truncated reports fail the external driver, never the transaction.
    pub(crate) fn observe<T: Serialize>(
        stage: AcceptanceStage,
        binding: &crate::version_history::journal::JournalBinding,
        generation: u64,
        collect: impl FnOnce() -> Result<T, SafeError>,
    ) {
        contain_passive(|| {
            let value = collect()?;
            write_observation(stage, binding, generation, &value)
        });
    }
    fn write_observation(
        stage: AcceptanceStage,
        binding: &crate::version_history::journal::JournalBinding,
        generation: u64,
        value: &impl Serialize,
    ) -> Result<(), SafeError> {
        use std::{fs::OpenOptions, io::Write, os::windows::fs::OpenOptionsExt};
        let target = bound_target()?;
        let user = CurrentUser::capture().map_err(|_| denied())?;
        user.require_unelevated().map_err(|_| denied())?;
        if user.sid_text() != target.target_sid {
            return Err(denied());
        }
        let directory = private_evidence(&target, &user)?;
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Report<'a, T: Serialize> {
            schema: u32,
            run_id: &'a str,
            base_head: &'a str,
            build_id: &'a str,
            binding_sha256: String,
            scenario: AcceptanceScenario,
            transaction_id: &'a str,
            generation: u64,
            stage: AcceptanceStage,
            value: &'a T,
        }
        let report = Report {
            schema: 1,
            run_id: &target.run_id,
            base_head: &target.base_head,
            build_id: &target.build_id,
            binding_sha256: crate::version_history::verified_package::sha256(
                COMPILED_TARGET.as_bytes(),
            ),
            scenario: target.scenario,
            transaction_id: &binding.transaction_id,
            generation,
            stage,
            value,
        };
        struct BoundedBytes(Vec<u8>);
        impl Write for BoundedBytes {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if self.0.len().saturating_add(bytes.len()) > 128 * 1024 * 1024 {
                    return Err(std::io::Error::other("acceptance report limit"));
                }
                self.0.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut bytes = BoundedBytes(Vec::new());
        serde_json::to_writer(&mut bytes, &report).map_err(|_| denied())?;
        let stage = serde_json::to_value(stage).map_err(|_| denied())?;
        let stage = stage.as_str().ok_or_else(denied)?;
        let path = Path::new(&directory.directory().path().map_err(|_| denied())?)
            .join(format!("{}-{stage}.json", target.run_id));
        directory.verify(&user).map_err(|_| denied())?;
        // Ancestors are pinned no-delete and final name is create-new only.
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .custom_flags(0x00200000)
            .open(path)
            .map_err(|_| denied())?;
        file.write_all(&bytes.0).map_err(|_| denied())?;
        file.sync_all().map_err(|_| denied())?;
        directory.verify(&user).map_err(|_| denied())
    }
}
#[cfg(all(feature = "history-roundtrip-acceptance", not(test), windows))]
pub(crate) use native::{
    before_installer_resume, check_evidence_scope, evidence_observation, observe,
    require_source_target, scenario,
};
