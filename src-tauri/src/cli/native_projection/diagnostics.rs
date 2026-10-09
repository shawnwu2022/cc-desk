//! Fixed diagnostic stages only; raw field/index/parser values never enter this DTO.
use crate::cli::types::SafeError;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProjectionStage {
    ScopeDocumentAdmission,
    ScopeRequestDecode,
    ScopeRequestValidation,
    ScopeProfileValidation,
    ScopeEnvironment,
    ScopeProjectRegistration,
    ScopeSourceSelection,
    ScopeSourceRoot,
    ScopeCapability,
    ScopeTask,
    ScopeResponseAdmission,
    ReadDocumentAdmission,
    ReadRequestDecode,
    ReadRequestValidation,
    ReadCapability,
    ReadSourceEnumeration,
    ReadTask,
    ReadResponseAdmission,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProjectionFailure {
    code: String,
    stage: ProjectionStage,
    retryable: bool,
    #[cfg(test)]
    #[serde(skip)]
    pub(crate) cause: SafeError,
}
impl ProjectionStage {
    pub(crate) fn failure(self, cause: SafeError) -> ProjectionFailure {
        ProjectionFailure {
            code: cause.code.clone(),
            stage: self,
            retryable: cause.retryable,
            #[cfg(test)]
            cause,
        }
    }
}
