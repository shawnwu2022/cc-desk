//! 返回检查点只记录原owner已证明的受管事务状态，不从旧receipt构造live权限。
use super::*;

#[cfg(test)]
mod tests;

const MAX_CHECKPOINT_BYTES: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReturnCheckpoint {
    schema: u32,
    anchor: AdmissionAnchor,
    roles: BTreeMap<ManifestRole, String>,
    materials: String,
}

/// 只读对账结果，不是原生恢复权限；对象重验和用户确认必须另行完成。
pub(crate) struct ReturnCheckpointInspection {
    pub(crate) digest: String,
    pub(crate) generation: u64,
    pub(crate) materials: Vec<u8>,
}

#[cfg(any(test, windows))]
impl JournalStore {
    fn read_return_checkpoint(&self, digest: &str) -> Result<ReturnCheckpoint, SafeError> {
        let bytes = self.protect_manifest(digest)?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        let checkpoint: ReturnCheckpoint = serde_json::from_slice(&bytes)
            .map_err(|_| error("HISTORY_RETURN_CHECKPOINT_BLOCKED"))?;
        if checkpoint.schema != 1 {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        checkpoint.anchor.binding.validate()?;
        validate_digest(&checkpoint.anchor.head)?;
        validate_digest(&checkpoint.materials)?;
        if self.protect_manifest(&checkpoint.materials)?.len() > MAX_CHECKPOINT_BYTES {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        for digest in checkpoint.roles.values() {
            self.protect_manifest(digest)?;
        }
        Ok(checkpoint)
    }

    pub(super) fn validate_return_checkpoint_artifact(
        &self,
        journal: &SwitchJournal,
        digest: &str,
        head: &str,
        identity: &str,
    ) -> Result<(), SafeError> {
        let checkpoint = self.read_return_checkpoint(digest)?;
        if !checkpoint.anchor.matches(journal, head, identity)
            || checkpoint.roles != journal.manifests
        {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        Ok(())
    }

    // 只能由下方持有原生证据的入口调用；测试模块可验证真实存储协议。
    fn retain_return_checkpoint(&mut self, materials: &[u8]) -> Result<String, SafeError> {
        self.check_writer_current()?;
        if materials.is_empty() || materials.len() > MAX_CHECKPOINT_BYTES {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        let state = self
            .writer
            .as_ref()
            .ok_or_else(|| error("HISTORY_TRANSACTION_CHANGED"))?;
        if !state.journal.return_checkpoint_candidate() {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        let checkpoint = ReturnCheckpoint {
            schema: 1,
            anchor: AdmissionAnchor {
                binding: state.journal.binding.clone(),
                generation: state.journal.generation,
                head: state.head.clone(),
                journal_identity: state.identity.clone(),
            },
            roles: state.journal.manifests.clone(),
            materials: self.retain_manifest_in_lane(materials, true)?,
        };
        let bytes = serde_json::to_vec(&checkpoint)
            .map_err(|_| error("HISTORY_RETURN_CHECKPOINT_BLOCKED"))?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        let digest = self.retain_manifest_in_lane(&bytes, true)?;
        // 再读原字节，只有完整不可变记录可进入日志；半写不补全。
        if self.protect_manifest(&digest)? != bytes {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        Ok(digest)
    }

    pub(crate) fn inspect_return_checkpoint(
        &self,
        binding: &JournalBinding,
        marker: &super::super::maintenance::ActiveContextMarker,
    ) -> Result<ReturnCheckpointInspection, SafeError> {
        let inspection = self.inspect(binding)?;
        if inspection.blocked {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        let journal = inspection
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RETURN_CHECKPOINT_BLOCKED"))?;
        let (digest, generation) = journal
            .return_checkpoint
            .as_ref()
            .ok_or_else(|| error("HISTORY_RETURN_CHECKPOINT_BLOCKED"))?;
        if journal.return_claim.is_some()
            || *generation != journal.generation
            || journal.phase != JournalPhase::Restoring
            || journal.requires_reconciliation()
        {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        marker.validate_checkpoint(
            journal,
            inspection
                .head()
                .ok_or_else(|| error("HISTORY_RETURN_CHECKPOINT_BLOCKED"))?,
        )?;
        let checkpoint = self.read_return_checkpoint(digest)?;
        Ok(ReturnCheckpointInspection {
            digest: digest.clone(),
            generation: *generation,
            materials: self.protect_manifest(&checkpoint.materials)?,
        })
    }

    #[cfg(windows)]
    pub(crate) fn seal_live_return_checkpoint(
        &mut self,
        evidence: &super::super::windows::return_checkpoint::LiveReturnCheckpoint<'_>,
        generation: u64,
    ) -> Result<u64, SafeError> {
        let materials = evidence.verify(self, generation)?;
        let checkpoint = self.retain_return_checkpoint(&materials)?;
        if evidence.verify(self, generation)? != materials {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        self.append_admitted(
            generation,
            JournalEvent::ReturnCheckpointSealed { checkpoint },
        )
    }

    #[cfg(windows)]
    pub(crate) fn claim_live_return_checkpoint(
        &mut self,
        evidence: &super::super::windows::return_checkpoint::LiveReturnCheckpoint<'_>,
        marker: &super::super::maintenance::ActiveContextMarker,
        generation: u64,
    ) -> Result<u64, SafeError> {
        let materials = evidence.verify(self, generation)?;
        let checkpoint = self.inspect_return_checkpoint(evidence.binding(), marker)?;
        if checkpoint.generation != generation || checkpoint.materials != materials {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        self.append_admitted(
            generation,
            JournalEvent::ReturnExecutionClaimed {
                checkpoint: checkpoint.digest,
                attempt_id: uuid::Uuid::new_v4().to_string(),
            },
        )
    }
}

impl SwitchJournal {
    pub(super) fn return_checkpoint_candidate(&self) -> bool {
        self.phase == JournalPhase::Restoring
            && self.return_checkpoint.is_none()
            && self.return_claim.is_none()
            && !self.requires_reconciliation()
            && !self.has_historical_uncertainty()
            && !self.context_return_only
            && !self.preinstall_return_only
            // 正常后来内容/安装备份本就收窄为return-only，不能视为恢复已开始。
            && [
                ManifestRole::ManagerHandoff,
                ManifestRole::SourceHandoffExit,
                ManifestRole::SourceContext,
                ManifestRole::SourceBundle,
                ManifestRole::FreshTargetContext,
                ManifestRole::RetainedTargetContext,
                ManifestRole::Registration,
                ManifestRole::Shortcuts,
            ]
            .into_iter()
            .all(|role| self.manifests.contains_key(&role))
            && [
                EffectKind::FenceSourceImage,
                EffectKind::VerifySourceBundleCopy,
                EffectKind::InstallerCreateSuspended,
                EffectKind::InstallerResume,
                EffectKind::InstallerTerminalOutcome,
                EffectKind::HistoricalCreateSuspended,
                EffectKind::HistoricalResume,
                EffectKind::HistoricalTerminalOutcome,
                EffectKind::FenceHistoricalImage,
            ]
            .into_iter()
            .all(|kind| self.effects.values().filter(|effect| effect.spec.kind == kind).count() == 1
                && self.applied(kind))
            && self.preserved(&self.binding.source_context)
            && self.preserved(&self.binding.target_context)
            && self.effects.values().all(|effect| {
                effect.result.as_ref().is_some_and(|result| result.observation == Observation::Applied)
                    && !matches!(effect.spec.kind,
                        EffectKind::RestoreSourceRoot { .. }
                        | EffectKind::RecoveryFilesystemEntry { .. }
                        | EffectKind::RecoveryRegistrationEntry { .. }
                        | EffectKind::RecoveryShortcutEntry { .. }
                        | EffectKind::FilesystemEntry { .. }
                        | EffectKind::RegistrationEntry { .. }
                        | EffectKind::VerifySourceBundleRestore
                        | EffectKind::VerifyRegistrationRestore { .. }
                        | EffectKind::RestoreShortcut { .. }
                        | EffectKind::ReverseSourceRoot { .. }
                        | EffectKind::ReverseSourceFence { .. })
            })
    }
}
