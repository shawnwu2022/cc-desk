// 反馈环：单个会话状态变化后，会话列表必须局部更新（生产链路）。
// 生产接线为 App.vue → SidebarPanel → SessionsPanel :project-groups="management.visibleGroups"。
// 症状断言：无关项目的 ProjectNode 收到新 project prop 对象（Vue 必然重渲染该项目子树）。
// 期望：单行状态变化只换被刷新项目的组对象身份，无关项目组对象与行对象保持原引用。
import { beforeEach, describe, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { defineComponent, nextTick } from 'vue'
import { createI18n } from 'vue-i18n'
import { createPinia, setActivePinia } from 'pinia'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import ProjectNode from '@/components/sessions/ProjectNode.vue'
import SessionsPanel from '@/components/sessions/SessionsPanel.vue'
import SessionItem from '@/components/sessions/SessionItem.vue'
import { useProjectManagementStore } from '@/stores/projectManagement'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useProjectsStateStore } from '@/stores/projectsState'
import type { SessionAdapter, UnifiedSession } from '@/types/unifiedSession'

const now = new Date(2026, 9, 18, 9, 0).getTime()
const PROJECT_A = '/work/a'
const PROJECT_B = '/work/b'
let listCalls = 0

function row(id: string, projectPath: string, extra: Partial<UnifiedSession> = {}): UnifiedSession {
  return { id, projectKey: projectPath, projectPath, cli: 'claude', runtime: 'native-cli',
    title: id, processState: 'running', attentionState: 'none', lastActivityAt: now - 60_000,
    archived: false, opened: true, resumable: true, adapterSessionId: id, ...extra }
}

// 模拟真实 adapter：每次 listSessions 从可变源重建行对象（新身份），刷新由 store 层
// 保留未变化行身份（见 catalogProjectionIdentity.test.ts）。
const source: UnifiedSession[] = [
  row('a1', PROJECT_A), row('a2', PROJECT_A),
  row('b1', PROJECT_B), row('b2', PROJECT_B),
]
const adapter: SessionAdapter = {
  runtime: 'native-cli',
  listSessions: async (projectKey?: string) => {
    listCalls += 1
    return source.filter(s => !projectKey || s.projectKey === projectKey).map(s => ({ ...s }))
  },
  createSession: () => { throw new Error('unused') },
  resumeSession: () => { throw new Error('unused') },
  activateSession: () => { throw new Error('unused') },
  stopSession: async () => { throw new Error('unused') },
  restartSession: () => { throw new Error('unused') },
  closeSession: async () => { throw new Error('unused') },
  renameSession: () => { throw new Error('unused') },
  archiveSession: async () => { throw new Error('unused') },
  restoreArchivedSession: async () => { throw new Error('unused') },
}

// 与 App.vue 相同的生产接线：侧边栏组来自 management.visibleGroups。
const Harness = defineComponent({
  components: { SessionsPanel },
  setup: (_, { expose }) => {
    const management = useProjectManagementStore()
    expose({ management })
    return { management }
  },
  template: '<SessionsPanel :project-groups="management.visibleGroups" :archived-sessions="management.visibleArchiveSessions" />',
})

let wrapper: VueWrapper | null = null
let i18n: ReturnType<typeof createI18n>
beforeEach(() => {
  setActivePinia(createPinia())
  useProjectsStateStore().loaded = true
  i18n = createI18n({ legacy: false, locale: 'en', fallbackLocale: 'en', messages: { en, zh } })
})

async function projectProps(): Promise<Map<string, unknown>> {
  await nextTick()
  return new Map(wrapper!.findAllComponents(ProjectNode)
    .map(node => [node.vm.$props.project.projectKey as string, node.vm.$props.project]))
}
async function itemProps(): Promise<Map<string, unknown>> {
  await nextTick()
  return new Map(wrapper!.findAllComponents(SessionItem)
    .map(item => [item.vm.$props.session.id as string, item.vm.$props.session]))
}

describe('session list update scope on single status change', () => {
  it('SessionList_StatusChangeUpdatesChangedRowOnly_071', async () => {
    const catalog = useUnifiedSessionsStore()
    catalog.configureAdapters([adapter])
    await catalog.refresh()
    wrapper = mount(Harness, { global: { plugins: [i18n] } })
    for (const arrow of wrapper.findAll('.expand-arrow')) await arrow.trigger('click')
    const beforeProjects = await projectProps()
    const beforeItems = await itemProps()
    expect(beforeProjects.size).toBe(2)
    expect(beforeItems.size).toBe(4)

    listCalls = 0
    source.find(s => s.id === 'a2')!.attentionState = 'needs-user'
    await catalog.refresh(PROJECT_A)
    expect(listCalls).toBe(1)

    const afterProjects = await projectProps()
    const afterItems = await itemProps()
    // 被刷新项目内未变化行：store 层身份保留（既有回归已覆盖，此处作为基线）。
    expect(afterItems.get('a1')).toBe(beforeItems.get('a1'))
    // 变化行换新对象。
    expect(afterItems.get('a2')).not.toBe(beforeItems.get('a2'))
    // 无关项目：组对象与行对象都保持原引用（当前为红的症状点）。
    expect(afterProjects.get(PROJECT_B)).toBe(beforeProjects.get(PROJECT_B))
    expect(afterItems.get('b1')).toBe(beforeItems.get('b1'))
    expect(afterItems.get('b2')).toBe(beforeItems.get('b2'))
  })
})
