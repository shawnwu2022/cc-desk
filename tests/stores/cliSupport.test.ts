import { beforeEach, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useCliSupportStore } from '@/stores/cliSupport'

beforeEach(() => { localStorage.clear(); setActivePinia(createPinia()) })

// 新安装的 Codex 偏好应回落到唯一默认启用的 Claude，读取不写入存储。
it('CliSupport_FreshDefaults_001', () => {
  const store = useCliSupportStore()
  expect(store.support).toEqual({ claude: true, codex: false })
  expect(store.fallbackDefault('codex')).toBe('claude')
  expect(localStorage.getItem('cc-desk-cli-support-v1')).toBeNull()
})

// 缺失或损坏的字段独立回退，不能覆盖另一字段明确保存的开关。
it.each([
  [{ claude: false }, { claude: false, codex: false }],
  [{ codex: true }, { claude: true, codex: true }],
  [{ claude: false, codex: 'invalid' }, { claude: false, codex: false }],
  [{ claude: 'invalid', codex: true }, { claude: true, codex: true }],
  [{}, { claude: true, codex: false }],
])('CliSupport_FieldFallback_002_%j', (stored, expected) => {
  localStorage.setItem('cc-desk-cli-support-v1', JSON.stringify(stored))
  expect(useCliSupportStore().support).toEqual(expected)
  expect(JSON.parse(localStorage.getItem('cc-desk-cli-support-v1')!)).toEqual(stored)
})

// 已保存的启用、关闭以及全部关闭状态在重新加载后保留。
it.each([
  { claude: false, codex: true },
  { claude: false, codex: false },
  { claude: true, codex: true },
])('CliSupport_SavedChoices_003_%j', stored => {
  localStorage.setItem('cc-desk-cli-support-v1', JSON.stringify(stored))
  const store = useCliSupportStore()
  expect(store.support).toEqual(stored)
  store.setCliEnabled('codex', !stored.codex)
  setActivePinia(createPinia())
  expect(useCliSupportStore().support).toEqual({ ...stored, codex: !stored.codex })
})

// 无法解析的偏好使用新默认值，不能阻止应用启动。
it('CliSupport_InvalidStorage_004', () => {
  localStorage.setItem('cc-desk-cli-support-v1', '{broken')
  expect(useCliSupportStore().support).toEqual({ claude: true, codex: false })
})
