import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const nestedInteractiveHeaders = [
  'src/components/agents/AgentItem.vue',
  'src/components/skills/SkillItem.vue',
  'src/components/mcp/McpItem.vue',
  'src/components/plugins/PluginItem.vue',
]

describe('SidebarKeyboardHandlers', () => {
  it('NestedControlKeydown_DoesNotToggleParent_SequenceNum01 — 子控件按键不触发父级折叠', () => {
    for (const file of nestedInteractiveHeaders) {
      const source = readFileSync(resolve(__dirname, '..', file), 'utf-8')
      expect(source, file).toMatch(/@keydown\.enter\.self\.prevent/)
      expect(source, file).toMatch(/@keydown\.space\.self\.prevent/)
    }
  })
})
