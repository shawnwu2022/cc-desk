//! Native text-paste capability policy. Version strings are not capability evidence.
//! Without an advertised text Ctrl-V contract, retain Desk's mode-bound text path.

use super::profile_service::authorize_profile_window;
use super::profiles::error;
use super::storage::WorkspaceRepository;
use super::types::{SafeError, WireU64};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PasteSupportRequest {
    pub(crate) profile_id: String,
    pub(crate) expected_revision: WireU64,
}

impl PasteSupportRequest {
    fn validate(&self) -> Result<(), SafeError> {
        if self.profile_id.is_empty()
            || self.profile_id.len() > 128
            || !self
                .profile_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        {
            return Err(SafeError::invalid("profileId"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexNativePasteSupport {
    pub(crate) profile_id: String,
    pub(crate) native_text_paste: bool,
}

pub(crate) fn native_paste_support(
    repository: &WorkspaceRepository,
    caller: &str,
    request: &PasteSupportRequest,
) -> Result<CodexNativePasteSupport, SafeError> {
    authorize_profile_window(caller)?;
    request.validate()?;
    let profile = repository.get_profile(&request.profile_id)?;
    if profile.revision != request.expected_revision {
        return Err(error("REVISION_CONFLICT"));
    }
    // No passive Ctrl-V text capability is currently advertised. Do not infer
    // it from a version, executable path or bracketed-paste mode, and do not
    // execute the selected CLI merely to decide a local clipboard route.
    let native_text_paste = false;
    Ok(CodexNativePasteSupport {
        profile_id: profile.id,
        native_text_paste,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(
        program: &std::path::Path,
    ) -> (tempfile::TempDir, WorkspaceRepository, PasteSupportRequest) {
        use super::super::profiles::{Override, Profile};
        use super::super::storage::Patch;
        let root = tempfile::tempdir().unwrap();
        let repository = WorkspaceRepository::open(root.path().join("workspace.json")).unwrap();
        let mut profile = Profile::new("paste-test", super::super::types::CliKind::Codex);
        profile.program_path = Override::Set(program.to_str().unwrap().into());
        let document = repository
            .apply(WireU64::parse("0").unwrap(), Patch::Create { profile })
            .unwrap();
        let request = PasteSupportRequest {
            profile_id: "paste-test".into(),
            expected_revision: document.profiles["paste-test"].revision,
        };
        (root, repository, request)
    }

    #[cfg(windows)]
    #[test]
    #[allow(non_snake_case)]
    fn C5_NoEvidenceNeverUsesVersionOrExecutesProgram_001() {
        for version in ["0.163.0", "0.162.0", "9.99.99", "", "unknown", "0.163"] {
            let root = tempfile::tempdir().unwrap();
            let program = root.path().join("codex.cmd");
            let marker = root.path().join("executed.txt");
            std::fs::write(
                &program,
                format!(
                    "@echo off\r\n@echo executed > \"{}\"\r\n@echo codex-cli {version}\r\n",
                    marker.display()
                ),
            )
            .unwrap();
            let (_metadata, repository, request) = fixture(&program);
            let support = native_paste_support(&repository, "main", &request).unwrap();
            assert!(
                !support.native_text_paste,
                "a version string is not input-capability evidence: {version}"
            );
            assert!(
                !marker.exists(),
                "querying paste support must not execute the selected CLI"
            );
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn C5_ProfileAuthorizationAndRevisionRemainRequired_002() {
        let (_root, repository, mut request) = fixture(std::path::Path::new("/unused/program"));
        assert_eq!(
            native_paste_support(&repository, "other-window", &request)
                .unwrap_err()
                .code,
            "FORBIDDEN"
        );
        request.expected_revision = WireU64::parse("99").unwrap();
        assert_eq!(
            native_paste_support(&repository, "main", &request)
                .unwrap_err()
                .code,
            "REVISION_CONFLICT"
        );
        request.profile_id = "../private".into();
        assert_eq!(
            native_paste_support(&repository, "main", &request)
                .unwrap_err()
                .code,
            "INVALID_REQUEST"
        );
    }
}
