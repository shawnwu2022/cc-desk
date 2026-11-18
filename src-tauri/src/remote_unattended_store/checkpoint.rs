//! 同原 owner 与事务身份的有界日志回收；不改变授权文档或时钟。
use super::*;
use crate::version_history::windows::unattended_store::{OwnerCustody, WindowsUnattendedBackend};

impl<S: Sealer> Store<WindowsUnattendedBackend, S> {
    pub(super) fn checkpoint(
        &self,
        expected: &Expected,
        custody: &OwnerCustody,
        authorize: &mut dyn FnMut() -> Result<()>,
    ) -> Result<LockedSnapshot> {
        custody.check(&self.backend)?;
        let mut local = self.lock()?;
        if local.blocked {
            return Err(StoreError::Unauthorized);
        }
        let mut tx = self.backend.begin()?;
        let doc = self.read(&mut tx, &mut local)?;
        Self::cas(&doc, expected)?;
        authorize()?;
        let result = (|| {
            let blob = tx.current_blob()?;
            let verified =
                journal::seal_checkpoint(&doc, &blob, tx.checkpoint_basis()?, &self.sealer)?;
            tx.checkpoint(&verified, &self.sealer, authorize)
        })();
        if let Err(error) = result {
            if error == StoreError::Unknown {
                local.unknown = true;
                local.blocked = true;
            }
            return Err(error);
        }
        local.last = Some(doc.clone());
        Ok(LockedSnapshot(doc))
    }
}
