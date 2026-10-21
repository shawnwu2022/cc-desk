import highlights from '../../docs/release-notes.json'
import type { UpdateInfo } from '@/types/app'

/** 仅使用明确按版本/语言维护的摘要；旧版未知格式保持原文，不按词语猜测或删改。 */
export function releaseNotesForLocale(info: UpdateInfo | null, locale: string): string {
  if (!info) return ''
  const language = locale.toLowerCase().startsWith('zh') ? 'zh' : 'en'
  const bundled = /^\d+\.\d+\.\d+$/.test(info.version)
    ? (highlights.releases as Record<string, Partial<Record<'en' | 'zh', string>>>)[info.version] : undefined
  return info.releaseNotesByLocale?.[language]?.trim() || info.releaseNotesByLocale?.en?.trim()
    || bundled?.[language]?.trim() || bundled?.en?.trim() || info.releaseNotes
}
