import { message } from '@tauri-apps/plugin-dialog'
import { registerProject } from './workspace'
import type { SafeError } from '@/types/cli'

/** Explicit-open bookkeeping only; never launches a CLI or rewrites native configuration. */
export async function persistProjectRegistration(path: string): Promise<void> {
  const receipt = await registerProject(path)
  if (!receipt || typeof receipt.projectId !== 'string' || !receipt.projectId
      || receipt.projectId.includes('\0')) {
    throw { code: 'INVALID_WORKSPACE_RESPONSE', retryable: false } satisfies SafeError
  }
}

export async function registerSelectedDirectory(path: string): Promise<boolean> {
  try {
    await persistProjectRegistration(path)
    return true
  } catch {
    await message(
      'Unable to save this project registration. Existing terminals are unaffected. Please retry opening the directory.',
      { title: 'CC Desk', kind: 'error' },
    )
    return false
  }
}
