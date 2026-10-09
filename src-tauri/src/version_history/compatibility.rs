//! No historical shared-data compatibility rule has been reviewed yet.
use crate::cli::profiles::error;
use crate::cli::types::SafeError;

#[derive(Clone, Copy)]
pub(crate) enum ReviewedDataMode {
    FreshSettings,
    KeepCurrentData,
}

pub(crate) fn admit_data_mode(mode: ReviewedDataMode) -> Result<(), SafeError> {
    match mode {
        ReviewedDataMode::FreshSettings => Ok(()),
        ReviewedDataMode::KeepCurrentData => Err(error("HISTORY_COMPATIBILITY_UNKNOWN")),
    }
}
