import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { relativizePath } from '@/utils/path'
describe('relativizePath', () => {
  // Windows 下文件位于项目目录内 → 返回用 '/' 分隔的相对路径
  it('RelativizePath_WinInside_001', () => {
    expect(
      relativizePath('C:\\proj\\src\\main.ts', 'C:\\proj'),
    ).toBe('src/main.ts')
  })

  // Windows 下盘符大小写不同仍视为同根
  it('RelativizePath_WinDriveCase_002', () => {
    expect(
      relativizePath('c:\\proj\\src\\a.ts', 'C:\\proj'),
    ).toBe('src/a.ts')
  })

  // Windows 下目录名大小写不同仍视为同根
  it('RelativizePath_WinDirCase_003', () => {
    expect(
      relativizePath('C:\\Proj\\Src\\a.ts', 'C:\\proj'),
    ).toBe('Src/a.ts')
  })

  // Windows 下文件位于项目目录外 → 原样返回
  it('RelativizePath_WinOutside_004', () => {
    expect(
      relativizePath('D:\\other\\file.ts', 'C:\\proj'),
    ).toBe('D:\\other\\file.ts')
  })

  // Windows 下文件与项目根完全相同 → 返回 '.'
  it('RelativizePath_WinSameRoot_005', () => {
    expect(relativizePath('C:\\proj', 'C:\\proj')).toBe('.')
  })

  // Windows 下文件与项目根仅大小写不同 → 返回 '.'
  it('RelativizePath_WinSameRootCase_006', () => {
    expect(relativizePath('C:\\Proj', 'c:\\proj')).toBe('.')
  })

  // 项目路径末尾带反斜杠也应正确识别
  it('RelativizePath_WinTrailingSep_007', () => {
    expect(
      relativizePath('C:\\proj\\src\\a.ts', 'C:\\proj\\'),
    ).toBe('src/a.ts')
  })

  // 项目根名是其他项目根的前缀时不应误判（C:\proj vs C:\proj-other）
  it('RelativizePath_WinPrefixSibling_008', () => {
    expect(
      relativizePath('C:\\proj-other\\a.ts', 'C:\\proj'),
    ).toBe('C:\\proj-other\\a.ts')
  })

  // macOS 下文件位于项目目录内 → 返回相对路径
  it('RelativizePath_MacInside_001', () => {
    expect(
      relativizePath('/Users/u/proj/src/a.ts', '/Users/u/proj'),
    ).toBe('src/a.ts')
  })

  // macOS 下大小写敏感：目录名大小写不同视为不同项目
  it('RelativizePath_MacCaseSensitive_002', () => {
    expect(
      relativizePath('/Users/u/Proj/src/a.ts', '/Users/u/proj'),
    ).toBe('/Users/u/Proj/src/a.ts')
  })

  // macOS 下文件位于项目目录外 → 原样返回
  it('RelativizePath_MacOutside_003', () => {
    expect(
      relativizePath('/etc/hosts', '/Users/u/proj'),
    ).toBe('/etc/hosts')
  })

  // macOS 下文件与项目根完全相同 → 返回 '.'
  it('RelativizePath_MacSameRoot_004', () => {
    expect(relativizePath('/Users/u/proj', '/Users/u/proj')).toBe('.')
  })

  // macOS 下兄弟前缀目录不误判（/a/proj vs /a/proj-other）
  it('RelativizePath_MacPrefixSibling_005', () => {
    expect(
      relativizePath('/a/proj-other/a.ts', '/a/proj'),
    ).toBe('/a/proj-other/a.ts')
  })

  // 项目路径为空 → 原样返回
  it('RelativizePath_EmptyProject_001', () => {
    expect(relativizePath('/foo/bar.ts', '')).toBe('/foo/bar.ts')
  })

  // 文件路径为空 → 原样返回
  it('RelativizePath_EmptyFile_002', () => {
    expect(relativizePath('', '/foo')).toBe('')
  })

  // 嵌套子目录也能正确计算相对路径
  it('RelativizePath_DeepNested_001', () => {
    expect(
      relativizePath('/a/b/c/d/e.ts', '/a/b'),
    ).toBe('c/d/e.ts')
  })
})

// 平台感知需 mock utils/platform；先 doMock 再动态 import 被测模块
describe('normalizePath 平台感知', () => {
  beforeEach(() => {
    vi.resetModules()
  })
  afterEach(() => {
    vi.doUnmock('@/utils/platform')
  })

  // Windows：反斜杠规范 + 去尾斜杠 + 小写
  it('NormalizePath_WindowsLower_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'windows', platform: 'windows',
      isMac: false, isWindows: true,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'win32-x64',
    }))
    const { normalizePath } = await import('@/utils/path')
    expect(normalizePath('E:\\Source\\Foo\\')).toBe('e:/source/foo')
  })

  // macOS：同样小写 + 斜杠规范
  it('NormalizePath_MacLower_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'macos', platform: 'macos',
      isMac: true, isWindows: false,
      ctrl: 'Control', alt: 'Option', cmd: 'Cmd', getClaudePlatformKey: () => 'darwin-arm64',
    }))
    const { normalizePath } = await import('@/utils/path')
    expect(normalizePath('/Users/Tester/Repo')).toBe('/users/tester/repo')
  })

  // Linux：大小写敏感 -> 不 lower，仅斜杠规范 + 去尾斜杠
  it('NormalizePath_LinuxNoLower_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'linux', platform: 'linux',
      isMac: false, isWindows: false,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'linux-x64',
    }))
    const { normalizePath } = await import('@/utils/path')
    expect(normalizePath('/work/Foo/')).toBe('/work/Foo')
  })

  // Linux 大小写敏感：/work/Foo 与 /work/foo 不等价（身份区分）
  it('NormalizePath_LinuxCaseSensitive_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'linux', platform: 'linux',
      isMac: false, isWindows: false,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'linux-x64',
    }))
    const { normalizePath } = await import('@/utils/path')
    expect(normalizePath('/work/Foo')).not.toBe(normalizePath('/work/foo'))
  })

  // Windows 大小写不敏感：E:\Repo 与 e:/repo 等价（身份合并）
  it('NormalizePath_WindowsCaseInsensitive_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'windows', platform: 'windows',
      isMac: false, isWindows: true,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'win32-x64',
    }))
    const { normalizePath } = await import('@/utils/path')
    expect(normalizePath('E:\\Repo')).toBe(normalizePath('e:/repo'))
  })

  // POSIX 根路径：/ 去尾斜杠不成空串，保留 '/'
  it('NormalizePath_PosixRoot_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'linux', platform: 'linux',
      isMac: false, isWindows: false,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'linux-x64',
    }))
    const { normalizePath } = await import('@/utils/path')
    expect(normalizePath('/')).toBe('/')
    expect(normalizePath('///')).toBe('/')
  })

  // Windows drive 根：C:\ / C: / C:/ 均归一为 c:（盘符小写 + 去尾斜杠）
  it('NormalizePath_WindowsDriveRoot_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'windows', platform: 'windows',
      isMac: false, isWindows: true,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'win32-x64',
    }))
    const { normalizePath } = await import('@/utils/path')
    expect(normalizePath('C:\\')).toBe('c:')
    expect(normalizePath('C:')).toBe('c:')
    expect(normalizePath('C:/')).toBe('c:')
    expect(normalizePath('D:\\Proj\\')).toBe('d:/proj')
  })
})

// sameProjectPath：统一项目路径比较（normalizePath 比较），平台感知
describe('sameProjectPath 平台感知等价', () => {
  beforeEach(() => { vi.resetModules() })
  afterEach(() => { vi.doUnmock('@/utils/platform') })

  // Windows：反斜杠 vs 正斜杠 + 大小写差异 -> 同一项目
  it('SameProjectPath_Windows_Equal_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'windows', platform: 'windows',
      isMac: false, isWindows: true,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'win32-x64',
    }))
    const { sameProjectPath } = await import('@/utils/path')
    expect(sameProjectPath('E:\\Source\\Foo', 'e:/source/foo')).toBe(true)
  })

  // Windows：尾斜杠差异 -> 同一项目
  it('SameProjectPath_Windows_TrailingSlash_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'windows', platform: 'windows',
      isMac: false, isWindows: true,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'win32-x64',
    }))
    const { sameProjectPath } = await import('@/utils/path')
    expect(sameProjectPath('E:\\Source\\Foo\\', 'e:/source/foo')).toBe(true)
  })

  // Windows：不同项目 -> false
  it('SameProjectPath_Windows_Distinct_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'windows', platform: 'windows',
      isMac: false, isWindows: true,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'win32-x64',
    }))
    const { sameProjectPath } = await import('@/utils/path')
    expect(sameProjectPath('E:\\Foo', 'E:\\Bar')).toBe(false)
  })

  // Linux：大小写敏感 -> /work/Foo 与 /work/foo 不同项目
  it('SameProjectPath_Linux_CaseSensitive_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'linux', platform: 'linux',
      isMac: false, isWindows: false,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'linux-x64',
    }))
    const { sameProjectPath } = await import('@/utils/path')
    expect(sameProjectPath('/work/Foo', '/work/foo')).toBe(false)
  })

  // Linux：斜杠规范 + 尾斜杠 -> 同一项目（仅大小写保留）
  it('SameProjectPath_Linux_SlashNormalized_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'linux', platform: 'linux',
      isMac: false, isWindows: false,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'linux-x64',
    }))
    const { sameProjectPath } = await import('@/utils/path')
    expect(sameProjectPath('/work/Foo\\Bar/', '/work/Foo/Bar')).toBe(true)
  })
})
