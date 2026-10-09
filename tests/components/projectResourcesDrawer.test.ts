import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { readFileSync } from 'node:fs'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import ProjectResourcesDrawer from '@/components/workspace/ProjectResourcesDrawer.vue'
import { useProjectResourcesStore } from '@/stores/projectResources'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { createProjectionClient } from '@/api/nativeProjection'
import type { ResourceItem } from '@/types/nativeProjection'
const host = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), createNativeProjectionClient: () => createProjectionClient({ instanceId: 'instance', invoke: host.invoke }) }))
let wrapper: VueWrapper | undefined
let rows: Partial<Record<string, ResourceItem[]>>
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks(); rows = {}
  const tab = useNativeTabsStore().create({ cli: 'claude', projectId: 'project', projectPath: '/work/project', profileId: 'profile', profileRevision: '1', action: { kind: 'new' } })
  useNativeTabsStore().tab(tab.tabId)!.status = 'running'
  useUnifiedSessionsStore().sessions = [{ id: 'selected', adapterSessionId: tab.tabId, projectKey: '/work/project', projectPath: '/work/project', runtime: 'native-cli', cli: 'claude', title: 'Selected',
    processState: 'running', attentionState: 'none', lastActivityAt: 1, archived: false, resumable: false }]
  useUnifiedSessionsStore().activeSessionId = 'selected'
  host.invoke.mockImplementation(async (command, request) => command === 'native_get_scope'
    ? { scopeId: 'scope', instanceId: 'instance', cli: 'claude', sourceRootKey: 'root', identityEpoch: '1', profileId: 'profile', profileRevision: '1', target: request, basis: 'launch-environment' }
    : { source: request.source, resourceKind: request.resourceKind, requestEpoch: request.requestEpoch, observedAt: '1', state: 'ready', reason: null, items: rows[request.resourceKind] ?? [], hasMore: false })
})
afterEach(() => { wrapper?.unmount(); wrapper = undefined; document.body.innerHTML = '' })
async function render(locale = 'en') {
  useProjectResourcesStore().setActive(true)
  wrapper = mount(ProjectResourcesDrawer, { global: { plugins: [createI18n({ legacy: false, locale, messages: { en, zh } })] } })
  await flushPromises(); return wrapper
}

describe('Read-only structured resource content', () => {
  // 六类资源作为上下文分类，不重复展示历史或第二套终端标签。
  it('ResourceView_SixCategories_001', async () => {
    const view = await render()
    expect(view.findAll('option').map(option => option.attributes('value'))).toEqual(['instructions', 'config', 'mcp', 'skills', 'agents', 'plugins'])
    expect(view.text()).toContain('No resources found'); expect(view.find('[role="dialog"]').exists()).toBe(false)
    expect(view.find('textarea').exists()).toBe(false)
    expect(view.findAll('button')).toHaveLength(1)
  })
  // 说明文档与配置使用结构化标签和值，不显示原始 JSON。
  it('ResourceView_StructuredFields_002', async () => {
    rows.instructions = [{ type: 'document', name: 'CLAUDE.md', text: 'Use focused tests.', truncated: false, origin: 'project' }]
    rows.config = [{ type: 'setting', name: 'model', value: 'sonnet', origin: 'global' }]
    const view = await render(); expect(view.text()).toContain('CLAUDE.md'); expect(view.text()).toContain('Use focused tests.')
    await view.get('select').setValue('config'); await flushPromises()
    expect(view.get('dt').text()).toBe('Model'); expect(view.get('dd').text()).toContain('sonnet'); expect(view.text()).not.toContain('"type"')
  })
  // MCP 只展示名称与 transport，不渲染指令、URL、环境或 header。
  it('ResourceView_McpTypedRows_003', async () => {
    rows.mcp = [{ type: 'mcp', name: 'docs', transport: 'http', origin: 'project' }]
    const view = await render(); await view.get('select').setValue('mcp'); await flushPromises()
    expect(view.text()).toContain('docs'); expect(view.text()).toContain('HTTP'); expect(view.findAll('a')).toHaveLength(0)
  })
  // Skill、Agent 和 Plugin 保留说明与显式状态并只读。
  it('ResourceView_ListsReadOnly_004', async () => {
    rows.skills = [{ type: 'skill', name: 'review', description: 'Inspect tests', origin: 'project' }]
    rows.agents = [{ type: 'agent', name: 'reviewer', description: 'Review changes', model: 'sonnet', origin: 'global' }]
    rows.plugins = [{ type: 'plugin', id: 'private-id', name: 'tools', version: '1.2.3', enabled: false, installed: null, origin: 'global' }]
    const view = await render()
    for (const [kind, text] of [['skills', 'Inspect tests'], ['agents', 'Review changes'], ['plugins', '1.2.3']]) {
      await view.get('select').setValue(kind); await flushPromises(); expect(view.text()).toContain(text)
    }
    expect(view.text()).toContain('Disabled'); expect(view.text()).toContain('Unknown'); expect(view.text()).not.toContain('private-id')
    expect(view.find('input').exists()).toBe(false)
  })
  // 所有可展示自由文本的可疑秘密与路径均不进入 DOM 属性或正文。
  it('ResourceView_NoSecretFields_005', async () => {
    rows.agents = [{ type: 'agent', name: 'sk-proj-DO_NOT_RENDER', description: 'Authorization: Bearer DO_NOT_RENDER', model: '/private/DO_NOT_RENDER', origin: '/private/DO_NOT_RENDER' }]
    const view = await render(); await view.get('select').setValue('agents'); await flushPromises()
    expect(view.html()).not.toContain('DO_NOT_RENDER'); expect(view.text()).toContain('Hidden for privacy')
  })
  // unavailable 与 ready-empty 使用不同提示，异常正文不输出。
  it('ResourceView_UnavailableNotEmpty_006', async () => {
    host.invoke.mockRejectedValue(new Error('TOKEN=DO_NOT_RENDER'))
    const view = await render(); expect(view.text()).toContain('could not be read'); expect(view.text()).not.toContain('No resources found'); expect(view.html()).not.toContain('DO_NOT_RENDER')
  })
  // bounded partial 与文档截断有独立可读提示。
  it('ResourceView_PartialTruncated_007', async () => {
    rows.instructions = [{ type: 'document', name: 'AGENTS.md', text: 'Excerpt', truncated: true, origin: 'project' }]
    const view = await render(); useProjectResourcesStore().partial = true; await flushPromises()
    expect(view.text()).toContain('Excerpt'); expect(view.text()).toContain('truncated'); expect(view.text()).toContain('Partial results')
  })
  // 空选择提示用户选择会话，不伪造项目默认资源。
  it('ResourceView_ChooseSession_008', async () => {
    useUnifiedSessionsStore().activeSessionId = null
    const view = await render(); expect(view.text()).toContain('Select a session'); expect(host.invoke).not.toHaveBeenCalled()
  })
  // 中文资源术语与说明完整翻译。
  it('ResourceView_ChineseLabels_009', async () => {
    const view = await render('zh'); expect(view.text()).toContain('项目说明'); expect(view.text()).toContain('配置'); expect(view.text()).toContain('未找到资源')
    expect(view.text()).not.toContain('Profile')
  })
  // 正常 App 接入同一个 shell context slot，无 JSON dump/额外全局抽屉。
  it('ResourceView_NormalAppWiring_010', () => {
    const app = readFileSync('src/App.vue', 'utf8')
    expect(app).toContain('<ProjectResourcesDrawer'); expect(app).not.toContain("t('contextResourcesHint')")
    for (const file of ['workspace/ProjectResourcesDrawer', 'resources/InstructionsView', 'resources/SettingsView', 'resources/McpList', 'resources/SkillList', 'resources/AgentList', 'resources/PluginList']) {
      const source = readFileSync(`src/components/${file}.vue`, 'utf8')
      expect(source).not.toContain('JSON.stringify'); expect(source).not.toContain('v-html'); expect(source).not.toContain('<AppDrawer'); expect(source).not.toContain('<AppDialog')
    }
  })
  // 未完整读取的空页不能宣称完整来源没有资源。
  it('ResourceView_PartialEmpty_011', async () => {
    const view = await render(); useProjectResourcesStore().partial = true; await flushPromises()
    expect(view.text()).toContain('Partial results'); expect(view.text()).not.toContain('No resources found')
  })
  // 六类资源的文本字段统一隐藏密钥外观，不写入 title 或其他属性。
  it.each(['instructions', 'config', 'mcp', 'skills', 'agents', 'plugins'])('ResourceView_PrivacyAllKinds_012 %s', async kind => {
    rows = {
      instructions: [{ type: 'document', name: 'CLAUDE.md', text: 'API_KEY=DO_NOT_RENDER', truncated: false, origin: 'project' }],
      config: [{ type: 'setting', name: 'model', value: 'sk-proj-DO_NOT_RENDER', origin: 'project' }],
      mcp: [{ type: 'mcp', name: 'TOKEN_DO_NOT_RENDER', transport: 'http', origin: 'project' }],
      skills: [{ type: 'skill', name: 'review', description: 'password: DO_NOT_RENDER', origin: 'project' }],
      agents: [{ type: 'agent', name: 'review', description: 'Inspect changes', model: '/private/DO_NOT_RENDER', origin: 'project' }],
      plugins: [{ type: 'plugin', name: 'review', id: '/private/DO_NOT_RENDER', version: 'credential:DO_NOT_RENDER', enabled: null, installed: null, origin: '/private/DO_NOT_RENDER' }],
    }
    const view = await render(); await view.get('select').setValue(kind); await flushPromises()
    expect(view.html()).not.toContain('DO_NOT_RENDER'); expect(view.text()).toContain('Hidden for privacy')
  })
})
