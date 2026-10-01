import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import ProjectNode from '@/components/sessions/ProjectNode.vue'
import SessionsPanel from '@/components/sessions/SessionsPanel.vue'
import ArchivedSessionsDrawer from '@/components/sessions/ArchivedSessionsDrawer.vue'
import SessionItem from '@/components/sessions/SessionItem.vue'
import SidebarPanel from '@/components/sidebar/SidebarPanel.vue'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useProjectsStateStore } from '@/stores/projectsState'
import type { SessionAdapter, UnifiedProjectGroup, UnifiedSession } from '@/types/unifiedSession'

const now = new Date(2026, 8, 30, 9, 0).getTime()
function session(extra: Partial<UnifiedSession> = {}): UnifiedSession {
  return { id: 'claude-1', projectKey: '/work/game', projectPath: '/work/game', cli: 'claude',
    runtime: 'legacy-claude', title: 'Claude work', processState: 'running', attentionState: 'none',
    lastActivityAt: now - 60_000, archived: false, resumable: true, adapterSessionId: 'adapter-1', ...extra }
}
function group(sessions = [session()], extra: Partial<UnifiedProjectGroup> = {}): UnifiedProjectGroup {
  return { projectKey: '/work/game', projectPath: '/work/game', name: 'Game', sessions,
    pinned: false, hidden: false, runningCount: sessions.filter(s => s.processState === 'running').length,
    needsUserCount: sessions.filter(s => s.attentionState === 'needs-user').length, lastActivityAt: now, ...extra }
}
const mounted: VueWrapper[] = []
let i18n: ReturnType<typeof createI18n>
beforeEach(() => {
  vi.useFakeTimers(); vi.setSystemTime(now)
  setActivePinia(createPinia())
  useProjectsStateStore().loaded = true
  i18n = createI18n({ legacy: false, locale: 'en', fallbackLocale: 'en', messages: { en, zh } })
})
afterEach(() => {
  mounted.splice(0).forEach(w => w.unmount())
  document.body.innerHTML = ''
  document.head.querySelectorAll('[data-test-project-tree]').forEach(style => style.remove())
  vi.useRealTimers(); vi.restoreAllMocks()
})
function node(project = group(), props: Record<string, unknown> = {}) {
  const wrapper = mount(ProjectNode, { attachTo: document.body, props: { project, expanded: true, ...props }, global: { plugins: [i18n] } })
  mounted.push(wrapper); return wrapper
}
function panel(props: Record<string, unknown> = {}) {
  const wrapper = mount(SessionsPanel, { attachTo: document.body, props, global: { plugins: [i18n] } })
  mounted.push(wrapper); return wrapper
}
async function selectMenu(id: string) {
  await nextTick()
  document.querySelector<HTMLElement>(`[data-item-id="${id}"]`)!.click()
  await nextTick()
}
function rules(file: string) {
  const source = readFileSync(resolve(file), 'utf8')
  const style = document.createElement('style'); style.dataset.testProjectTree = ''
  style.textContent = source.match(/<style[^>]*>([\s\S]*?)<\/style>/)![1]
  document.head.append(style)
  return Array.from(style.sheet!.cssRules).filter((rule): rule is CSSStyleRule => rule instanceof CSSStyleRule)
}

describe('Unified project session tree', () => {
  // The normal App's F2 reveal is owned by the catalog, including across actual adapter refreshes.
  it.each(['collapsed', 'filtered'])('Tree_ExternalRenameDraftRefresh_016: %s', async mode => {
    const store = useUnifiedSessionsStore()
    const original = session({ renameState: 'idle' })
    store.configureAdapters([{ runtime: 'legacy-claude', listSessions: vi.fn(async () => [{ ...original }]) } as unknown as SessionAdapter])
    await store.refresh()
    const wrapper = panel()
    if (mode === 'filtered') await wrapper.get('.search-input').setValue('unmatched query')
    expect(wrapper.find('[data-session-row]').exists()).toBe(false)
    store.beginRename(original.id)
    await flushPromises()
    await wrapper.get('[data-session-row] input').setValue('My unsaved draft')
    await store.refresh(); await flushPromises()
    expect(wrapper.find('[data-session-row] input').exists()).toBe(true)
    expect((wrapper.get('[data-session-row] input').element as HTMLInputElement).value).toBe('My unsaved draft')
    store.cancelRename(original.id)
    await flushPromises()
    expect(wrapper.find('[data-session-row]').exists()).toBe(false)
    await store.refresh(); await flushPromises()
    expect(wrapper.find('[data-session-row]').exists()).toBe(false)
    if (mode === 'filtered') expect((wrapper.get('.search-input').element as HTMLInputElement).value).toBe('unmatched query')
  })

  it('Tree_RenameSourceInvalidation_017', async () => {
    const store = useUnifiedSessionsStore()
    let original = session({ renameState: 'idle' })
    store.configureAdapters([{ runtime: 'legacy-claude', listSessions: vi.fn(async () => [{ ...original }]) } as unknown as SessionAdapter])
    await store.refresh()
    const wrapper = panel()
    await wrapper.get('.project-main').trigger('click')
    store.beginRename(original.id); await flushPromises()
    await wrapper.get('[data-session-row] input').setValue('Old source draft')
    original = { ...original, adapterSessionId: 'replacement-source', title: 'Replacement source' }
    await store.refresh(); await flushPromises()
    expect(wrapper.find('[data-session-row] input').exists()).toBe(false)
    expect(wrapper.get('[data-session-row]').text()).toContain('Replacement source')
    store.beginRename(original.id); await flushPromises()
    expect((wrapper.get('[data-session-row] input').element as HTMLInputElement).value).toBe('Replacement source')
    store.cancelRename(original.id); await flushPromises()
    expect(wrapper.find('[data-session-row] input').exists()).toBe(false)
  })

  // Migrating ProjectNode's props is what makes mixed runtime sessions render once under their project.
  it('Tree_MixedCliSessions_001', async () => {
    const codex = session({ id: 'codex-1', cli: 'codex', runtime: 'native-cli', title: 'Codex work' })
    const wrapper = node(group([session(), codex]), { selectedId: codex.id })
    const rows = wrapper.findAllComponents(SessionItem)
    expect(rows).toHaveLength(2)
    expect(wrapper.get('.session-list').element.parentElement!.closest('[role="treeitem"]')).toBe(wrapper.element)
    expect(rows.map(r => r.get('.cli-app-icon').attributes('aria-label'))).toEqual(['Claude Code', 'Codex CLI'])
    expect(rows[1].classes('active')).toBe(true)
    await rows[1].trigger('click')
    expect(wrapper.emitted('activate')).toEqual([[codex.id]])
    expect(wrapper.text()).not.toMatch(/Native|Legacy|Profile|Running/)
  })

  // Collapsed attention comes from unified project counts, not the legacy PTY attention store.
  it('Tree_CollapseAttention_002', async () => {
    const wrapper = node(group([session({ attentionState: 'needs-user' })]), { expanded: false })
    expect(wrapper.findAllComponents(SessionItem)).toHaveLength(0)
    expect(wrapper.get('[data-project-attention]').attributes('aria-label')).toBe('Needs your reply: 1')
    await wrapper.get('.project-main').trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('toggle-expand')).toEqual([['/work/game']])
    await wrapper.setProps({ expanded: true })
    expect(wrapper.find('[data-project-attention]').exists()).toBe(false)
  })

  // The plus is the only project quick action; all administrative actions share AppMenu.
  it('Tree_SinglePlusAndProjectMenu_003', async () => {
    const wrapper = node()
    const quick = wrapper.findAll('[data-project-quick-action]')
    expect(quick).toHaveLength(1)
    await quick[0].trigger('click'); await nextTick()
    expect(wrapper.emitted('new-session-request')).toBeUndefined()
    await selectMenu('codex')
    expect(wrapper.emitted('new-session-request')).toEqual([[{ projectKey: '/work/game', projectPath: '/work/game', intent: 'codex' }]])
    await wrapper.get('.project-overflow-trigger button').trigger('click')
    const ids = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]'), item => item.dataset.itemId)
    expect(ids).toEqual(['pin', 'rename', 'view-archive', 'open-project-directory', 'remove-project'])
    await selectMenu('pin')
    expect(wrapper.emitted('project-action')).toEqual([[{ action: 'pin', projectKey: '/work/game', projectPath: '/work/game' }]])
    await wrapper.get('.project-row').trigger('contextmenu')
    expect(Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]'), item => item.dataset.itemId)).toEqual(ids)
    await selectMenu('remove-project')
    expect(wrapper.emitted('project-action')?.[1]).toEqual([{ action: 'remove-project', projectKey: '/work/game', projectPath: '/work/game' }])
  })

  // Running archive must bypass raw lifecycle actions and ask the caller to confirm stop-and-archive.
  it('Tree_RunningArchiveConfirmation_004', async () => {
    const store = useUnifiedSessionsStore()
    const stop = vi.spyOn(store, 'stopSession'); const archive = vi.spyOn(store, 'archiveSession')
    const wrapper = node()
    await wrapper.get('.session-item').trigger('contextmenu')
    expect(document.querySelector('[data-item-id="archive"]')!.textContent).toBe('Stop and archive')
    await selectMenu('archive')
    expect(wrapper.emitted('confirmation-request')).toEqual([[{ kind: 'stop-and-archive', sessionId: 'claude-1', projectKey: '/work/game', projectPath: '/work/game' }]])
    expect(wrapper.emitted('menu-action')).toBeUndefined()
    expect(stop).not.toHaveBeenCalled(); expect(archive).not.toHaveBeenCalled()
    await wrapper.setProps({ project: group([session({ processState: 'stopped' })]) })
    await wrapper.get('.session-item').trigger('contextmenu'); await selectMenu('archive')
    expect(wrapper.emitted('menu-action')).toEqual([['claude-1', 'archive']])
  })

  // Drawer restoration uses the same unified row and retains archived index records until the caller publishes success.
  it('Tree_ArchivedDrawerRestore_005', async () => {
    const archived = session({ id: 'archived-codex', cli: 'codex', runtime: 'native-cli', processState: 'stopped', archived: true })
    const other = session({ id: 'other-archive', projectKey: '/other', projectPath: '/other', archived: true })
    const wrapper = mount(ArchivedSessionsDrawer, { attachTo: document.body,
      props: { open: true, sessions: [archived, other, session()], project: { projectKey: '/work/game', projectPath: '/work/game' } }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    await nextTick()
    const rows = wrapper.findAllComponents(SessionItem)
    expect(rows).toHaveLength(1)
    expect(rows[0].props('session')).toEqual(archived)
    await rows[0].get('.session-primary-action button').trigger('click')
    expect(wrapper.emitted('restore-request')).toEqual([[archived.id]])
    expect(rows[0].props('session').archived).toBe(true)
    await rows[0].trigger('contextmenu'); await selectMenu('restore-archive')
    expect(wrapper.emitted('restore-request')).toEqual([[archived.id], [archived.id]])
    await wrapper.setProps({ sessions: [] })
    expect(document.querySelector('[role="dialog"]')!.textContent).toContain('No archived sessions')
  })

  // Menus in the modal drawer must stay inside its focus boundary rather than being teleported outside it.
  it('Tree_DrawerMenuFocusBoundary_012', async () => {
    const wrapper = mount(ArchivedSessionsDrawer, { attachTo: document.body,
      props: { open: true, sessions: [session({ archived: true, processState: 'stopped' })] }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    await nextTick(); await nextTick()
    await wrapper.getComponent(SessionItem).trigger('contextmenu')
    await nextTick(); await nextTick()
    const menu = document.querySelector<HTMLElement>('[role="menu"]')!
    expect(document.querySelector('[role="dialog"]')!.contains(menu)).toBe(true)
    expect(menu.contains(document.activeElement)).toBe(true)
    document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await nextTick()
    expect(document.querySelector('[role="menu"]')).toBeNull()
    expect(wrapper.emitted('update:open')).toBeUndefined()
    expect(document.querySelector('[role="dialog"]')).not.toBeNull()
  })

  // Normal Task 5 groups omit archived rows, but losing the final normal session must not strand its project's archive.
  it('Tree_AllArchivedProjectReachable_006', async () => {
    const store = useUnifiedSessionsStore()
    store.sessions = [session({ id: 'last-session', archived: true, processState: 'stopped' })]
    useProjectsStateStore().displayNames.set('/work/game', 'Archived game')
    expect(store.projectGroups).toHaveLength(0)
    const wrapper = panel()
    expect(wrapper.get('.project-name').text()).toBe('Archived game')
    expect(wrapper.findAll('.session-item')).toHaveLength(0)
    await wrapper.get('.project-overflow-trigger button').trigger('click'); await selectMenu('view-archive')
    expect(document.querySelector('[role="dialog"]')).not.toBeNull()
    const drawer = wrapper.getComponent(ArchivedSessionsDrawer)
    await drawer.getComponent(SessionItem).get('.session-primary-action button').trigger('click')
    expect(wrapper.emitted('restore-request')).toEqual([['last-session']])
    store.sessions = [session({ id: 'last-session', processState: 'stopped' })]
    await nextTick()
    expect(store.projectGroups).toHaveLength(1)
    expect(wrapper.findAllComponents(ProjectNode)).toHaveLength(1)
    await drawer.getComponent({ name: 'AppDrawer' }).vm.$emit('update:open', false)
    await wrapper.get('.project-main').trigger('click')
    expect(wrapper.findAll('.session-item')).toHaveLength(1)
  })

  // Search is temporary expansion: project match shows all rows, session match shows only matching rows, clearing restores explicit collapse.
  it('Tree_SearchAndExpandState_007', async () => {
    const normal = group([session(), session({ id: 'codex-1', cli: 'codex', title: 'Fix auth' })])
    const wrapper = panel({ projectGroups: [normal], archivedSessions: [] })
    const search = wrapper.get('.search-input')
    expect(wrapper.findAll('.session-item')).toHaveLength(0)
    await search.setValue('AUTH')
    expect(wrapper.findAll('.session-item')).toHaveLength(1)
    expect(wrapper.get('.session-name').text()).toBe('Fix auth')
    await wrapper.get('.project-main').trigger('click')
    await search.setValue('game')
    expect(wrapper.findAll('.session-item')).toHaveLength(2)
    await search.setValue('/WORK')
    expect(wrapper.findAll('.session-item')).toHaveLength(2)
    await search.setValue('')
    expect(wrapper.findAll('.session-item')).toHaveLength(0)
    await wrapper.get('.project-main').trigger('click')
    await search.setValue('auth'); await search.setValue('')
    expect(wrapper.findAll('.session-item')).toHaveLength(2)
    await search.setValue('missing')
    expect(wrapper.text()).toContain('No sessions found')
  })

  // Explicit archived-view access remains available while search filters every project out.
  it('Tree_GlobalArchivedAccessDuringSearch_008', async () => {
    const wrapper = panel({ projectGroups: [], archivedSessions: [session({ archived: true })] })
    await wrapper.get('.search-input').setValue('missing')
    await wrapper.get('[data-view-archived]').trigger('click')
    expect(wrapper.getComponent(ArchivedSessionsDrawer).props('project')).toBeNull()
    expect(wrapper.getComponent(ArchivedSessionsDrawer).findAllComponents(SessionItem)).toHaveLength(1)
    expect(wrapper.find('.options-content').exists()).toBe(false)
    expect(wrapper.find('input[type="checkbox"]').exists()).toBe(false)
    expect(wrapper.html()).not.toContain('skip-permissions')
  })

  // Long names cannot shrink arrow/marker/action columns; CSS-rule assertions are not real Windows scaling certification.
  it('Tree_LongNameFixedColumns_009', () => {
    const name = '项'.repeat(80)
    const wrapper = node(group([session({ title: 'S'.repeat(80) })], { name }))
    expect(wrapper.get('.session-name').text()).toBe('S'.repeat(80))
    const sessionCss = rules('src/components/sessions/SessionItem.vue')
    expect(sessionCss.find(rule => rule.selectorText === '.session-item')!.style.getPropertyValue('grid-template-columns')).toBe('16px 18px minmax(0, 1fr) 38px 20px')
    expect(wrapper.get('.project-name').text()).toBe(name)
    expect(wrapper.findAll('[data-project-quick-action]')).toHaveLength(1)
    const css = rules('src/components/sessions/ProjectNode.vue')
    const row = css.find(rule => rule.selectorText === '.project-row')!
    expect(row.style.getPropertyValue('display')).toBe('grid')
    expect(row.style.getPropertyValue('grid-template-columns')).toBe('20px minmax(0, 1fr) 20px 28px 28px')
    expect(row.style.getPropertyValue('height')).toBe('40px')
    const title = css.find(rule => rule.selectorText === '.project-name')!
    expect(title.style.getPropertyValue('overflow')).toBe('hidden')
    expect(title.style.getPropertyValue('text-overflow')).toBe('ellipsis')
    expect(title.style.getPropertyValue('white-space')).toBe('nowrap')
    expect(css.filter(rule => /:hover|:focus-within/.test(rule.selectorText)).every(rule => !rule.style.getPropertyValue('grid-template-columns'))).toBe(true)
  })

  // Inner controls and session rename consume their keys rather than collapsing the project or closing the panel.
  it('Tree_NestedKeyboardControls_010', async () => {
    const wrapper = panel({ projectGroups: [group()], archivedSessions: [] })
    await wrapper.get('.project-main').trigger('click')
    await wrapper.get('.project-overflow-trigger button').trigger('keydown', { key: 'Enter' })
    expect(wrapper.findAll('.session-item')).toHaveLength(1)
    await wrapper.get('.session-item').trigger('keydown', { key: 'F2' })
    const input = wrapper.get('.session-name-wrapper input')
    await input.setValue('New name'); await input.trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('rename-commit')).toEqual([['claude-1', 'New name']])
    await wrapper.get('.session-item').trigger('keydown', { key: 'F2' })
    await wrapper.get('.session-name-wrapper input').trigger('keydown', { key: 'Escape' })
    expect(wrapper.emitted('close')).toBeUndefined()
    expect(wrapper.findAll('.session-item')).toHaveLength(1)
    await wrapper.get('.project-main').trigger('keydown', { key: ' ' })
    expect(wrapper.findAll('.session-item')).toHaveLength(0)
  })

  // The intermediate older container must not interpret a unified create request as its legacy PTY launch event.
  it('Tree_NewSessionIsolatedFromLegacyContainer_013', async () => {
    useUnifiedSessionsStore().sessions = [session()]
    const wrapper = mount(SidebarPanel, { attachTo: document.body,
      props: { visible: true, activePanel: 'sessions' }, global: { plugins: [i18n],
        stubs: { SkillsPanel: true, AgentsPanel: true, McpPanel: true, PluginsPanel: true } } })
    mounted.push(wrapper)
    await wrapper.get('[data-project-quick-action]').trigger('click'); await nextTick(); await selectMenu('codex')
    expect(wrapper.emitted('newSession')).toBeUndefined()
    expect(wrapper.getComponent(SessionsPanel).emitted('new-session-request')).toEqual([[{ projectKey: '/work/game', projectPath: '/work/game', intent: 'codex' }]])
  })

  // Shared menu's Escape handling must precede the panel's close handler.
  it('Tree_MenuEscapeDoesNotClosePanel_011', async () => {
    const wrapper = panel({ projectGroups: [group()], archivedSessions: [] })
    await wrapper.get('.project-overflow-trigger button').trigger('click'); await nextTick()
    document.querySelector<HTMLElement>('[role="menu"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await nextTick()
    expect(document.querySelector('[role="menu"]')).toBeNull()
    expect(wrapper.emitted('close')).toBeUndefined()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    expect(wrapper.emitted('close')).toEqual([[]])
  })
})
