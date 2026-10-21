import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import WorkspaceSourceDetails from '@/components/workspace/WorkspaceSourceDetails.vue'
import { historyRecoveryHint, workspaceWarningKey, type WorkspaceHistoryIssue } from '@/utils/workspaceSourceWarnings'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'

describe('Actionable history diagnostics', () => {
  it.each(['project-metadata', 'configurations', 'registrations', 'catalog'] as const)('HistoryRecovery_NonHistoryGuidance_003 %s', source => {
    const view = mount(WorkspaceSourceDetails, { props: { warnings: [{ source, code: 'SOURCE_UNAVAILABLE' }], truncated: false }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
    expect(view.text()).not.toContain(en.historyRecoveryUnavailable)
    expect(view.text()).toContain('SOURCE_UNAVAILABLE')
    view.unmount()
  })
  // 匿名枚举问题明确保留未知范围，已确认身份的条目才显示会话与精确动作。
  it('HistoryRecovery_KnownAndUnknown_001', async () => {
    const warning = { source: 'codex-history' as const, code: 'SOURCE_UNSUPPORTED', stage: 'read-source-enumeration' as const }
    const base: WorkspaceHistoryIssue = { key: 'source', entryKey: 'entry', warningKey: workspaceWarningKey(warning), ...warning, configurationName: 'Codex work', projectPath: '/registered/project' }
    const view = mount(WorkspaceSourceDetails, { props: { warnings: [warning], truncated: false, partial: true, issues: [base, { ...base, key: 'session', sessionId: 'known-id', sessionTitle: 'Known session' }] }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
    expect(view.text()).toContain(en.historyIssueUnknownScope)
    expect(view.text()).toContain(en.historyRecoveryUnsupported)
    expect(view.findAll('[data-session-id]')).toHaveLength(1)
    expect(view.get('[data-session-id="known-id"]').text()).toContain('Known session')
    const rows = view.findAll('[data-history-issue]')
    await rows[1].get('[data-history-issue-retry]').trigger('click')
    await rows[1].get('[data-history-issue-open-project]').trigger('click')
    expect(view.emitted('retry')).toEqual([['session']])
    expect(view.emitted('openProject')).toEqual([['session']])
    await view.setProps({ busy: true })
    expect(rows[1].get('[data-history-issue-retry]').attributes('disabled')).toBeDefined()
    view.unmount()
  })
  // opencode 历史失败必须显示自己的来源名，并与其他历史源同样保留未知范围。
  it('HistoryRecovery_OpencodeSource_003', () => {
    const warning = { source: 'opencode-history' as const, code: 'SOURCE_BUSY', stage: 'read-source-enumeration' as const }
    const view = mount(WorkspaceSourceDetails, { props: { warnings: [warning], truncated: false, partial: true, issues: [] }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
    expect(view.text()).toContain(en.sourceWarningOpencodeHistory)
    expect(view.text()).toContain(en.historyIssueUnknownScope)
    expect(view.text()).toContain(en.historyRecoveryTemporary)
    view.unmount()
  })
  // 只在明确错误码支持时提供对应解释，不把泛化读取失败宣称为权限或文件损坏。
  it('HistoryRecovery_HonestReasons_002', () => {
    expect(historyRecoveryHint('SOURCE_READ_FORBIDDEN')).toBe('historyRecoveryPermission')
    expect(historyRecoveryHint('ENV_SOURCE_MISSING')).toBe('historyRecoveryMissing')
    expect(historyRecoveryHint('SOURCE_INVALID')).toBe('historyRecoveryInvalid')
    expect(historyRecoveryHint('SOURCE_CHANGED')).toBe('historyRecoveryTemporary')
    expect(historyRecoveryHint('SOURCE_READ_FAILED')).toBe('historyRecoveryUnavailable')
  })
})
