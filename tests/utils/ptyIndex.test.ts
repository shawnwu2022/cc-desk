import { describe, it, expect, beforeEach } from 'vitest'
import { PtyIndex } from '@/utils/ptyIndex'

// PTY → tab 反查索引：覆盖映射建立、注销、未知 PTY、空 ptyId、覆盖、clear 等分支。
// 对应 XTermTerminal.vue 的 onPtyOutput/onPtyExit 路由 + spawn link + 销毁 unlink + cleanup clear。
describe('PtyIndex', () => {
  let index: PtyIndex

  beforeEach(() => {
    index = new PtyIndex()
  })

  // 注册 + 查询（onPtyOutput 路由基础）
  it('PtyIndex_LinkGet_001', () => {
    index.link('pty-a', 'tab-1')
    expect(index.get('pty-a')).toBe('tab-1')
  })

  // 注销后查不到（onPtyExit / 实例销毁后不再路由）
  it('PtyIndex_Unlink_001', () => {
    index.link('pty-a', 'tab-1')
    index.unlink('pty-a')
    expect(index.get('pty-a')).toBeUndefined()
  })

  // 空 ptyId 不入索引（spawn 未成功赋 ptyId='' 时不建立映射）
  it('PtyIndex_EmptyPtyId_Ignored_001', () => {
    index.link('', 'tab-1')
    expect(index.get('')).toBeUndefined()
    expect(index.size).toBe(0)
  })

  // 未知 ptyId 返回 undefined（迟到/伪造事件不误路由）
  it('PtyIndex_UnknownPtyId_001', () => {
    index.link('pty-a', 'tab-1')
    expect(index.get('pty-unknown')).toBeUndefined()
  })

  // 同 ptyId 重 link 更新 tabId（防御：ptyId 唯一但覆盖语义明确）
  it('PtyIndex_Overwrite_001', () => {
    index.link('pty-a', 'tab-1')
    index.link('pty-a', 'tab-2')
    expect(index.get('pty-a')).toBe('tab-2')
    expect(index.size).toBe(1)
  })

  // 多映射独立：注销一个不影响其他（并行会话场景）
  it('PtyIndex_MultipleIndependent_001', () => {
    index.link('pty-a', 'tab-1')
    index.link('pty-b', 'tab-2')
    expect(index.get('pty-a')).toBe('tab-1')
    expect(index.get('pty-b')).toBe('tab-2')
    expect(index.size).toBe(2)
    index.unlink('pty-a')
    expect(index.get('pty-a')).toBeUndefined()
    expect(index.get('pty-b')).toBe('tab-2') // b 不受影响
    expect(index.size).toBe(1)
  })

  // clear 清空（组件 cleanup）
  it('PtyIndex_Clear_001', () => {
    index.link('pty-a', 'tab-1')
    index.link('pty-b', 'tab-2')
    index.clear()
    expect(index.size).toBe(0)
    expect(index.get('pty-a')).toBeUndefined()
  })

  // unlink 空/未知 ptyId 无副作用（防御）
  it('PtyIndex_UnlinkEmptyOrUnknown_Noop_001', () => {
    index.link('pty-a', 'tab-1')
    index.unlink('')
    index.unlink('pty-unknown')
    expect(index.size).toBe(1)
    expect(index.get('pty-a')).toBe('tab-1')
  })
})
