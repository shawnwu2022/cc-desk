//! 独立的无人值守公开记录；解析结果绝不转换为运行权限。
//! 当前无生产许可构造入口，也不连接 UI、网络、密钥供应或 Stage 1 owner。
#![allow(dead_code, reason = "分阶段存储模型，尚未接入原生批准链")]
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, MutexGuard,
};
use std::time::Instant;
#[path = "remote_unattended_store/dpapi.rs"]
mod dpapi;
#[path = "remote_unattended_store/journal.rs"]
pub(crate) mod journal;
use journal::CommitStamp;
#[cfg(test)]
#[path = "remote_unattended_store/tests.rs"]
mod tests;
const DOMAIN: &str = "cc-desk/native-unattended/v1";
pub(crate) const MAX_PLAINTEXT: usize = 8192;
pub(crate) const MAX_SEALED: usize = MAX_PLAINTEXT + 65536;
const MAX_MS: u64 = 30 * 24 * 60 * 60 * 1000;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoreError {
    Missing,
    Invalid,
    Conflict,
    Unauthorized,
    Busy,
    Io,
    Unknown,
    Poisoned,
    Expired,
    Clock,
    Capacity,
}
pub(crate) type Result<T> = std::result::Result<T, StoreError>;
pub(crate) trait Sealer {
    fn seal(&self, bytes: &[u8]) -> Result<Vec<u8>>;
    fn unseal(&self, bytes: &[u8]) -> Result<Vec<u8>>;
}
pub(crate) trait Transaction {
    fn read(&mut self) -> Result<Option<Vec<u8>>>;
    fn stamp(&self) -> Option<&CommitStamp>;
    fn commit(
        &mut self,
        bytes: &[u8],
        replacing: bool,
        stamp: &CommitStamp,
        authorize: &mut dyn FnMut() -> Result<()>,
    ) -> Result<()>;
}
pub(crate) trait Backend {
    type Guard: Transaction;
    fn begin(&self) -> Result<Self::Guard>;
    fn target_label(&self) -> Result<String>;
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    installation: [u8; 32],
    device: [u8; 32],
    target: [u8; 32],
    // 后续只能由已核实的原生完整绑定规范化生成，不接受 JS 自报摘要。
    binding_fingerprint: [u8; 32],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Status {
    Disabled,
    Enabled,
    Revoked,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: u32,
    domain: String,
    store_id: [u8; 16],
    revision: u64,
    // 存储 CAS 版本与授权版本分开；仅观察时钟不使连接票据失效。
    authority_revision: u64,
    status: Status,
    issued_ms: u64,
    cutoff_ms: u64,
    observed_wall_ms: u64,
    target: Target,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    domain: String,
    document: Document,
    digest: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Expected {
    store_id: [u8; 16],
    revision: u64,
    target: Target,
}
#[derive(Debug, PartialEq, Eq)]
struct LockedSnapshot(Document);
impl LockedSnapshot {
    fn expected(&self) -> Expected {
        Expected {
            store_id: self.0.store_id,
            revision: self.0.revision,
            target: self.0.target.clone(),
        }
    }
}
// 私有、不可序列化、不可克隆；只有测试构造，生产原生确认入口尚未实现。
struct NativePermit {
    instance: Arc<()>,
    expected: Expected,
    issued_ms: u64,
    cutoff_ms: u64,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}
impl NativePermit {
    fn check(&self, instance: &Arc<()>) -> Result<()> {
        if !Arc::ptr_eq(instance, &self.instance)
            || self.cancelled.load(Ordering::Acquire)
            || Instant::now() >= self.deadline
        {
            Err(StoreError::Unauthorized)
        } else {
            Ok(())
        }
    }
}
#[derive(Default)]
struct Local {
    unknown: bool,
    blocked: bool,
    last: Option<Document>,
}
struct Store<B, S> {
    backend: B,
    sealer: S,
    instance: Arc<()>,
    local: Mutex<Local>,
}
impl<B: Backend, S: Sealer> Store<B, S> {
    fn new(backend: B, sealer: S) -> Result<Self> {
        backend.target_label()?;
        Ok(Self {
            backend,
            sealer,
            instance: Arc::new(()),
            local: Mutex::new(Local::default()),
        })
    }
    fn lock(&self) -> Result<MutexGuard<'_, Local>> {
        let s = self.local.lock().map_err(|_| StoreError::Poisoned)?;
        if s.unknown {
            Err(StoreError::Poisoned)
        } else {
            Ok(s)
        }
    }
    // 只用于显式创建 Disabled；许可绑定本实例，不会在 load 缺失时调用。
    fn initialize(&self, permit: NativePermit) -> Result<LockedSnapshot> {
        let mut local = self.lock()?;
        permit.check(&self.instance)?;
        if local.blocked || local.last.is_some() || permit.expected.revision != 0 {
            return Err(StoreError::Conflict);
        }
        let mut tx = self.backend.begin()?;
        if tx.read()?.is_some() {
            return Err(StoreError::Conflict);
        }
        let doc = Document {
            schema: 1,
            domain: DOMAIN.into(),
            store_id: permit.expected.store_id,
            revision: 1,
            authority_revision: 1,
            status: Status::Disabled,
            issued_ms: permit.issued_ms,
            cutoff_ms: permit.issued_ms,
            observed_wall_ms: permit.issued_ms,
            target: permit.expected.target.clone(),
        };
        self.publish(&mut local, &mut tx, doc, false, &mut || {
            permit.check(&self.instance)
        })
    }
    fn read(&self, tx: &mut B::Guard, local: &mut Local) -> Result<Document> {
        let decoded = tx.read().and_then(|bytes| {
            let bytes = bytes.ok_or(StoreError::Missing)?;
            let doc = self.decode(&bytes)?;
            if tx.stamp() != Some(&self.stamp(&doc, &bytes)?) {
                return Err(StoreError::Unknown);
            }
            Ok(doc)
        });
        let doc = match decoded {
            Ok(doc) => doc,
            Err(error) => {
                local.blocked = true;
                return Err(error);
            }
        };
        check_known(local, &doc)?;
        Ok(doc)
    }
    fn cas(doc: &Document, expected: &Expected) -> Result<()> {
        if doc.store_id != expected.store_id
            || doc.revision != expected.revision
            || doc.target != expected.target
        {
            Err(StoreError::Conflict)
        } else {
            Ok(())
        }
    }
    // 锁内更新观察时钟并持久化；返回值仍 Locked，不能供应密钥或恢复 owner。
    fn observe(&self, expected: &Expected, now: u64) -> Result<LockedSnapshot> {
        let mut local = self.lock()?;
        if local.blocked {
            return Err(StoreError::Poisoned);
        }
        let mut tx = self.backend.begin()?;
        let mut doc = self.read(&mut tx, &mut local)?;
        Self::cas(&doc, expected)?;
        let closure = if now < doc.observed_wall_ms {
            Some(StoreError::Clock)
        } else if doc.status == Status::Enabled && now >= doc.cutoff_ms {
            Some(StoreError::Expired)
        } else {
            None
        };
        if let Some(reason) = closure {
            local.blocked = true;
            doc.revision = doc.revision.checked_add(1).ok_or(StoreError::Invalid)?;
            doc.authority_revision = doc
                .authority_revision
                .checked_add(1)
                .ok_or(StoreError::Invalid)?;
            doc.status = Status::Revoked;
            doc.observed_wall_ms = now.max(doc.observed_wall_ms);
            // 精确 CAS 已在同一锁内通过；先保存撤销，再报告时钟或截止原因。
            self.publish(&mut local, &mut tx, doc, true, &mut || Ok(()))?;
            return Err(reason);
        }
        if now == doc.observed_wall_ms {
            local.last = Some(doc.clone());
            return Ok(LockedSnapshot(doc));
        }
        doc.revision = doc.revision.checked_add(1).ok_or(StoreError::Invalid)?;
        doc.observed_wall_ms = now;
        self.publish(&mut local, &mut tx, doc, true, &mut || Ok(()))
    }
    fn enable(&self, permit: NativePermit, now: u64) -> Result<LockedSnapshot> {
        let mut local = self.lock()?;
        if local.blocked {
            return Err(StoreError::Poisoned);
        }
        permit.check(&self.instance)?;
        let mut tx = self.backend.begin()?;
        let mut doc = self.read(&mut tx, &mut local)?;
        Self::cas(&doc, &permit.expected)?;
        if now < doc.observed_wall_ms || permit.issued_ms != now {
            local.blocked = true;
            return Err(StoreError::Clock);
        }
        if doc.status == Status::Enabled {
            return Err(StoreError::Conflict);
        }
        doc.revision = doc.revision.checked_add(1).ok_or(StoreError::Invalid)?;
        doc.authority_revision = doc
            .authority_revision
            .checked_add(1)
            .ok_or(StoreError::Invalid)?;
        doc.status = Status::Enabled;
        doc.issued_ms = permit.issued_ms;
        doc.cutoff_ms = permit.cutoff_ms;
        doc.observed_wall_ms = now;
        self.publish(&mut local, &mut tx, doc, true, &mut || {
            permit.check(&self.instance)
        })
    }
    // 不要求旧启用许可，但严格核对存储身份、版本、设备、目标和完整绑定摘要。
    // 此处没有 live owner；即时关闭运行权限和跨进程通知属于后续接线。
    fn revoke(&self, expected: &Expected, now: u64) -> Result<LockedSnapshot> {
        let mut local = self.lock()?;
        local.blocked = true;
        let mut tx = self.backend.begin()?;
        let mut doc = self.read(&mut tx, &mut local)?;
        Self::cas(&doc, expected)?;
        doc.revision = doc.revision.checked_add(1).ok_or(StoreError::Invalid)?;
        doc.authority_revision = doc
            .authority_revision
            .checked_add(1)
            .ok_or(StoreError::Invalid)?;
        doc.status = Status::Revoked;
        doc.observed_wall_ms = now.max(doc.observed_wall_ms);
        self.publish(&mut local, &mut tx, doc, true, &mut || Ok(()))
    }
    fn publish(
        &self,
        local: &mut Local,
        tx: &mut B::Guard,
        doc: Document,
        replacing: bool,
        authorize: &mut dyn FnMut() -> Result<()>,
    ) -> Result<LockedSnapshot> {
        validate(&doc)?;
        let bytes = self.encode(&doc)?;
        if let Err(error) = tx.commit(&bytes, replacing, &self.stamp(&doc, &bytes)?, authorize) {
            if error == StoreError::Unknown {
                local.unknown = true
            }
            return Err(error);
        }
        local.last = Some(doc.clone());
        Ok(LockedSnapshot(doc))
    }
    fn stamp(&self, doc: &Document, bytes: &[u8]) -> Result<CommitStamp> {
        Ok(CommitStamp {
            store_id: doc.store_id,
            revision: doc.revision,
            authority_revision: doc.authority_revision,
            target: Sha256::digest(
                serde_json::to_vec(&doc.target).map_err(|_| StoreError::Invalid)?,
            )
            .into(),
            blob: Sha256::digest(bytes).into(),
        })
    }
    fn encode(&self, doc: &Document) -> Result<Vec<u8>> {
        let canonical = serde_json::to_vec(doc).map_err(|_| StoreError::Invalid)?;
        let bytes = serde_json::to_vec(&Envelope {
            domain: DOMAIN.into(),
            document: doc.clone(),
            digest: Sha256::digest(canonical).into(),
        })
        .map_err(|_| StoreError::Invalid)?;
        if bytes.len() > MAX_PLAINTEXT {
            return Err(StoreError::Invalid);
        }
        let sealed = self.sealer.seal(&bytes)?;
        if sealed.is_empty() || sealed.len() > MAX_SEALED {
            return Err(StoreError::Invalid);
        }
        Ok(sealed)
    }
    fn decode(&self, bytes: &[u8]) -> Result<Document> {
        if bytes.is_empty() || bytes.len() > MAX_SEALED {
            return Err(StoreError::Invalid);
        }
        let raw = self.sealer.unseal(bytes)?;
        if raw.len() > MAX_PLAINTEXT {
            return Err(StoreError::Invalid);
        }
        // 直接 struct 解析，拒绝重复、未知字段和不完整数据。
        let envelope: Envelope = serde_json::from_slice(&raw).map_err(|_| StoreError::Invalid)?;
        if envelope.domain != DOMAIN {
            return Err(StoreError::Invalid);
        }
        validate(&envelope.document)?;
        let canonical = serde_json::to_vec(&envelope.document).map_err(|_| StoreError::Invalid)?;
        let digest: [u8; 32] = Sha256::digest(canonical).into();
        if digest != envelope.digest {
            return Err(StoreError::Invalid);
        }
        Ok(envelope.document)
    }
}
fn validate(doc: &Document) -> Result<()> {
    let duration = doc
        .cutoff_ms
        .checked_sub(doc.issued_ms)
        .ok_or(StoreError::Invalid)?;
    if doc.schema != 1
        || doc.domain != DOMAIN
        || doc.store_id == [0; 16]
        || doc.revision == 0
        || doc.authority_revision == 0
        || doc.authority_revision > doc.revision
        || doc.issued_ms == 0
        || doc.observed_wall_ms < doc.issued_ms
        || duration > MAX_MS
        || (doc.status == Status::Enabled && duration == 0)
        || (doc.status == Status::Disabled && duration != 0)
        || [
            &doc.target.installation,
            &doc.target.device,
            &doc.target.target,
            &doc.target.binding_fingerprint,
        ]
        .iter()
        .any(|v| **v == [0; 32])
    {
        Err(StoreError::Invalid)
    } else {
        Ok(())
    }
}

fn check_known(local: &mut Local, doc: &Document) -> Result<()> {
    if local.last.as_ref().is_some_and(|old| {
        old.store_id != doc.store_id
            || old.revision > doc.revision
            || old.authority_revision > doc.authority_revision
            || old.observed_wall_ms > doc.observed_wall_ms
            || (old.revision == doc.revision && old != doc)
    }) {
        local.blocked = true;
        return Err(StoreError::Conflict);
    }
    Ok(())
}
