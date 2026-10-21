import { invoke } from '@tauri-apps/api/core'

export interface CodexNativePasteSupport {
  profileId: string
  nativeTextPaste: boolean
}

function canonicalRevision(value: string): boolean {
  return /^(0|[1-9][0-9]{0,19})$/.test(value)
    && BigInt(value) <= BigInt('18446744073709551615')
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** 只读能力探测；失败保守返回 false。 */
export async function cliCodexNativePasteSupport(
  profileId: string,
  expectedRevision: string,
): Promise<CodexNativePasteSupport> {
  if (typeof profileId !== 'string' || !/^[A-Za-z0-9_-]{1,128}$/.test(profileId)
    || typeof expectedRevision !== 'string' || !canonicalRevision(expectedRevision)) {
    throw new Error('INVALID_PASTE_SUPPORT_REQUEST')
  }
  const value = await invoke<unknown>('cli_codex_native_paste_support', {
    request: { profileId, expectedRevision },
  })
  if (!isRecord(value) || value.profileId !== profileId
    || typeof value.nativeTextPaste !== 'boolean') {
    throw new Error('INVALID_PASTE_SUPPORT_RESPONSE')
  }
  return { profileId: value.profileId, nativeTextPaste: value.nativeTextPaste }
}

// 每个启动 attempt 独立查询；旧的未完成响应不能成为新运行的能力证据。
export function codexNativeTextPasteForAttempt(
  profileId: string,
  expectedRevision: string,
): Promise<boolean> {
  return cliCodexNativePasteSupport(profileId, expectedRevision)
    .then(support => support.nativeTextPaste)
    .catch(() => false)
}
