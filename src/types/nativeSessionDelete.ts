import type { SourceRef } from './nativeProjection'
import type { NativeCliKind } from './cli'

export interface SessionDeletePreviewRequest { operationId: string; source: SourceRef; sessionKey: string }
export interface SessionDeletePreview {
  operationId: string; previewId: string | null; nativeSessionId: string | null; cli: NativeCliKind
  state: 'ready' | 'blocked' | 'unsupported'; rootArchived: boolean | null
  scope: 'native-subtree'; knownDescendantCount: null; reason: string | null
}
export interface SessionDeleteConfirmation {
  operationId: string; previewId: string; acknowledgePermanentDelete: true; acknowledgeNativeSubtree: true
}
export interface SessionDeleteReceipt {
  operationId: string; cli: NativeCliKind; state: 'acknowledged' | 'partial' | 'unknown' | 'blocked' | 'unsupported'
  rootPresent: boolean | null; descendants: 'unverified'; reason: string | null
}
export interface SessionDeleteClient {
  preview(request: SessionDeletePreviewRequest): Promise<SessionDeletePreview>
  confirm(request: SessionDeleteConfirmation): Promise<SessionDeleteReceipt>
  inspect(request: { operationId: string }): Promise<SessionDeleteReceipt>
}
