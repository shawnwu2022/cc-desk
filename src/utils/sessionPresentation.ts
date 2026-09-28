import type {
  SessionCatalogIdentity,
  SessionPrimaryAction,
  SessionVisualState,
  UnifiedSession,
} from '@/types/unifiedSession'

function requiredIdentity(value: string, code: string): string {
  const text = value.trim()
  if (!text) throw new Error(`${code}_REQUIRED`)
  if (text.includes('\0')) throw new Error(`${code}_INVALID`)
  return text
}

function normalizeProjectPath(value: string): string {
  let path = requiredIdentity(value, 'PROJECT_PATH').replace(/\\/g, '/')
  const unc = path.startsWith('//')
  path = path.replace(/\/{2,}/g, '/')
  if (unc) path = `/${path}`
  const isDriveRoot = /^[A-Za-z]:\/$/.test(path)
  if (path.length > 1 && !isDriveRoot) path = path.replace(/\/+$/, '')
  const windowsLike = /^[A-Za-z]:\//.test(path) || path.startsWith('//')
  return windowsLike ? path.toLocaleLowerCase('en-US') : path
}

export function makeSessionCatalogKey(identity: SessionCatalogIdentity): string {
  const projectPath = normalizeProjectPath(identity.projectPath)
  const adapterSessionId = requiredIdentity(identity.adapterSessionId, 'ADAPTER_SESSION_ID')
  const nativeSessionId = identity.nativeSessionId == null
    ? ''
    : requiredIdentity(identity.nativeSessionId, 'NATIVE_SESSION_ID')

  return JSON.stringify([
    'cc-desk-session-v1',
    identity.runtime,
    identity.cli,
    projectPath,
    adapterSessionId,
    nativeSessionId,
  ])
}

export function deriveSessionVisualState(session: UnifiedSession): SessionVisualState {
  if (session.processState === 'failed') return 'failed'
  if (session.attentionState === 'needs-user') return 'needs-user'
  if (session.processState === 'starting') return 'starting'
  if (session.processState === 'unknown') return 'confirming'
  if (session.processState === 'running') return 'running'
  return 'ended'
}

export function selectSessionPrimaryAction(session: UnifiedSession): SessionPrimaryAction | null {
  if (session.renameState === 'editing' || session.renameState === 'saving') return 'save-rename'
  if (session.archived) return 'restore-archive'

  switch (session.processState) {
    case 'starting':
      return 'cancel-start'
    case 'running':
      return session.attentionState === 'needs-user' ? null : 'stop'
    case 'unknown':
      return 'confirm-status'
    case 'failed':
      return 'retry'
    case 'stopped':
      return session.resumable ? 'resume' : null
  }
}
