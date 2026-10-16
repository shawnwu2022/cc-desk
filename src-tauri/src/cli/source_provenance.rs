//! 退役后仍可核对的只读来源；不持有 PTY、环境、argv 或进程控制能力。
use super::native_projection::scoped_fs::Root;
use super::{
    native_projection::selection::{locations, read_scope_known},
    profiles::error,
    snapshot::LaunchSnapshot,
    types::{CliKind, LaunchAction, SafeError, WireU64},
};

pub(crate) struct SourceSelection {
    pub(crate) root: Root,
    pub(crate) user_config: Option<Root>,
    pub(crate) project: Option<Root>,
}
pub(crate) struct SourceProvenance {
    pub(crate) cli: CliKind,
    pub(crate) profile_id: String,
    pub(crate) profile_revision: WireU64,
    selection: Result<SourceSelection, SafeError>,
}
impl SourceProvenance {
    pub(crate) fn freeze(snapshot: &LaunchSnapshot) -> Self {
        let known = snapshot.raw_args().is_none()
            && read_scope_known(
                snapshot.request().cli,
                snapshot.launcher(),
                snapshot.default_args().iter().chain(snapshot.extra_args()),
            )
            && snapshot
                .legacy_default_args()
                .is_none_or(|args| args.is_empty());
        let selection = if known {
            (|| -> Result<SourceSelection, SafeError> {
                let locations =
                    locations(snapshot.request().cli, snapshot.environment()).map_err(error)?;
                let selected = SourceSelection {
                    root: Root::open(&locations.root).map_err(error)?,
                    user_config: locations
                        .user_config
                        .as_ref()
                        .map(|path| Root::open(path))
                        .transpose()
                        .map_err(error)?,
                    project: if matches!(snapshot.request().action, LaunchAction::New) {
                        Some(
                            Root::open(std::path::Path::new(&snapshot.request().launch_cwd))
                                .map_err(error)?,
                        )
                    } else {
                        None
                    },
                };
                selected.root.source_current().map_err(error)?;
                Ok(selected)
            })()
            .map_err(|_| error("SCOPE_UNKNOWN"))
        } else {
            Err(error("SCOPE_UNKNOWN"))
        };
        Self {
            cli: snapshot.request().cli,
            profile_id: snapshot.request().profile_id.clone(),
            profile_revision: snapshot.profile_revision(),
            selection,
        }
    }
    pub(crate) fn selection(&self) -> Result<&SourceSelection, SafeError> {
        let selected = self.selection.as_ref().map_err(Clone::clone)?;
        selected.root.source_current().map_err(error)?;
        for root in selected.user_config.iter().chain(selected.project.iter()) {
            root.source_current().map_err(error)?;
        }
        Ok(selected)
    }
}
#[cfg(test)]
#[path = "../tests/native_cli_source_provenance.rs"]
mod tests;
