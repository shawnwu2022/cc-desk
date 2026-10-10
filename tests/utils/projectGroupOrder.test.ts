import { describe, expect, test } from 'vitest'
import { compareProjectGroups } from '@/utils/projectGroupOrder'

describe('project group order', () => {
  // 检查固定项目先展示，固定与未固定分区内均按显示名称排序。
  test('ProjectOrder_PinnedFirst_001', () => {
    const groups = [
      { projectKey: '/alpha', name: 'Alpha', pinned: false },
      { projectKey: '/zulu', name: 'Zulu', pinned: true },
      { projectKey: '/mike', name: 'Mike', pinned: false },
      { projectKey: '/bravo', name: 'Bravo', pinned: true },
    ]

    expect(groups.sort(compareProjectGroups).map(group => group.projectKey))
      .toEqual(['/bravo', '/zulu', '/alpha', '/mike'])
  })

  // 检查显示别名决定项目顺序，目录身份不覆盖不同名称的字母顺序。
  test('ProjectOrder_DisplayName_002', () => {
    const groups = [
      { projectKey: '/aardvark', name: 'Zulu alias', pinned: false },
      { projectKey: '/zebra', name: 'Alpha alias', pinned: false },
    ]

    expect(groups.sort(compareProjectGroups).map(group => group.projectKey))
      .toEqual(['/zebra', '/aardvark'])
  })

  // 检查名称忽略大小写，数字片段按 2、11、20 的自然顺序排列。
  test('ProjectOrder_CaseAndDigits_003', () => {
    const groups = [
      { projectKey: '/beta-2', name: 'Beta 2', pinned: false },
      { projectKey: '/alpha-20', name: 'alpha 20', pinned: false },
      { projectKey: '/alpha-11', name: 'ALPHA 11', pinned: false },
      { projectKey: '/alpha-2', name: 'Alpha 2', pinned: false },
    ]

    expect(groups.sort(compareProjectGroups).map(group => group.name))
      .toEqual(['Alpha 2', 'ALPHA 11', 'alpha 20', 'Beta 2'])
  })

  // 检查相同名称以项目身份确定顺序，输入目录发现顺序改变仍保持一致。
  test('ProjectOrder_IdenticalNames_004', () => {
    const groups = [
      { projectKey: '/work/b', name: 'Shared', pinned: false },
      { projectKey: '/work/a', name: 'Shared', pinned: false },
    ]

    expect([...groups].sort(compareProjectGroups).map(group => group.projectKey))
      .toEqual(['/work/a', '/work/b'])
    expect([...groups].reverse().sort(compareProjectGroups).map(group => group.projectKey))
      .toEqual(['/work/a', '/work/b'])
  })

  // 检查大小写、重音和数字格式等价的名称仍按精确项目身份排序。
  test('ProjectOrder_EquivalentNames_005', () => {
    const groups = [
      { projectKey: '/work/a', name: 'résumé 02', pinned: true },
      { projectKey: '/work/A', name: 'RESUME 2', pinned: true },
      { projectKey: '/work/b', name: 'Resume 2', pinned: true },
    ]

    expect([...groups].sort(compareProjectGroups).map(group => group.projectKey))
      .toEqual(['/work/A', '/work/a', '/work/b'])
    expect([...groups].reverse().sort(compareProjectGroups).map(group => group.projectKey))
      .toEqual(['/work/A', '/work/a', '/work/b'])
    expect(compareProjectGroups(groups[0], groups[1])).toBeGreaterThan(0)
    expect(compareProjectGroups(groups[1], groups[0])).toBeLessThan(0)
  })

  // 检查会话活动时间与运行计数改变不会移动项目行。
  test('ProjectOrder_ActivityIgnored_006', () => {
    const groups = [
      { projectKey: '/zulu', name: 'Zulu', pinned: false, lastActivityAt: 100, runningCount: 1 },
      { projectKey: '/alpha', name: 'Alpha', pinned: false, lastActivityAt: 1, runningCount: 0 },
      { projectKey: '/mike', name: 'Mike', pinned: true, lastActivityAt: 0, runningCount: 0 },
    ]
    const expected = ['/mike', '/alpha', '/zulu']

    expect([...groups].sort(compareProjectGroups).map(group => group.projectKey)).toEqual(expected)
    groups[0].lastActivityAt = 0
    groups[0].runningCount = 0
    groups[1].lastActivityAt = 1000
    groups[1].runningCount = 1
    groups[2].lastActivityAt = 2000
    expect([...groups].reverse().sort(compareProjectGroups).map(group => group.projectKey)).toEqual(expected)
  })

  // 检查项目排序保持组内既有会话顺序，不修改会话活动时间。
  test('ProjectOrder_SessionsPreserved_007', () => {
    const zuluSessions = [
      { id: 'z-recent', lastActivityAt: 200 },
      { id: 'z-older', lastActivityAt: 100 },
    ]
    const alphaSessions = [
      { id: 'a-recent', lastActivityAt: 300 },
      { id: 'a-older', lastActivityAt: 1 },
    ]
    const groups = [
      { projectKey: '/zulu', name: 'Zulu', pinned: false, sessions: zuluSessions },
      { projectKey: '/alpha', name: 'Alpha', pinned: false, sessions: alphaSessions },
    ]

    expect(groups.sort(compareProjectGroups).map(group => group.projectKey)).toEqual(['/alpha', '/zulu'])
    expect(groups[0].sessions).toBe(alphaSessions)
    expect(groups[1].sessions).toBe(zuluSessions)
    expect(groups.map(group => group.sessions.map(session => [session.id, session.lastActivityAt])))
      .toEqual([[['a-recent', 300], ['a-older', 1]], [['z-recent', 200], ['z-older', 100]]])
  })
})
