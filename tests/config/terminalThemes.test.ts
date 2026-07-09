import { describe, it, expect } from 'vitest'
import {
  TERMINAL_THEMES,
  DEFAULT_TERMINAL_THEME_ID,
  normalizeTerminalThemeId,
  getTerminalTheme,
  hexToRgba,
  computeTerminalSurfaceVars,
} from '@/config/terminalThemes'

const REQUIRED_KEYS = [
  'background', 'foreground', 'cursor',
  'black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white',
  'brightBlack', 'brightRed', 'brightGreen', 'brightYellow',
  'brightBlue', 'brightMagenta', 'brightCyan', 'brightWhite',
]

describe('getTerminalTheme', () => {
  // 合法 id 返回对应 colors
  it('GetTerminalTheme_ValidId_ReturnsMatchingColors_001', () => {
    expect(getTerminalTheme('cc-box-light').background).toBe('#f8f9fa')
  })

  // undefined 返回默认主题 colors
  it('GetTerminalTheme_Undefined_ReturnsDefault_001', () => {
    const def = TERMINAL_THEMES.find(t => t.id === DEFAULT_TERMINAL_THEME_ID)!.colors
    expect(getTerminalTheme(undefined).background).toBe(def.background)
  })

  // 未知 id 返回默认主题 colors
  it('GetTerminalTheme_UnknownId_ReturnsDefault_001', () => {
    const def = TERMINAL_THEMES.find(t => t.id === DEFAULT_TERMINAL_THEME_ID)!.colors
    expect(getTerminalTheme('nope-not-a-theme').background).toBe(def.background)
  })
})

describe('normalizeTerminalThemeId', () => {
  // 非法/空 id 返回默认
  it('NormalizeTerminalThemeId_Invalid_ReturnsDefault_001', () => {
    expect(normalizeTerminalThemeId('nope')).toBe(DEFAULT_TERMINAL_THEME_ID)
    expect(normalizeTerminalThemeId(undefined)).toBe(DEFAULT_TERMINAL_THEME_ID)
    expect(normalizeTerminalThemeId('')).toBe(DEFAULT_TERMINAL_THEME_ID)
  })

  // 合法 id 原样返回
  it('NormalizeTerminalThemeId_Valid_ReturnsAsIs_001', () => {
    expect(normalizeTerminalThemeId('cc-box-light')).toBe('cc-box-light')
    expect(normalizeTerminalThemeId('cc-box-dark')).toBe('cc-box-dark')
  })
})

describe('hexToRgba', () => {
  // #d4d4d4 + 0.35 → rgba(212, 212, 212, 0.35)
  it('HexToRgba_ValidHex_ReturnsRgba_001', () => {
    expect(hexToRgba('#d4d4d4', 0.35)).toBe('rgba(212, 212, 212, 0.35)')
  })

  // 非 #rrggbb 原样返回（容错，避免崩溃）
  it('HexToRgba_InvalidHex_ReturnsAsIs_001', () => {
    expect(hexToRgba('#abc', 0.5)).toBe('#abc')
    expect(hexToRgba('not-a-color', 0.5)).toBe('not-a-color')
  })
})

describe('computeTerminalSurfaceVars', () => {
  // 返回 bg/scrollbar 两个 CSS 变量；scrollbar 为 foreground 的半透明
  it('ComputeSurfaceVars_ReturnsAllVars_001', () => {
    const vars = computeTerminalSurfaceVars(getTerminalTheme('cc-box-dark'))
    expect(vars['--terminal-surface-bg']).toBe('#1e1e1e')
    expect(vars['--terminal-surface-fg']).toBe('#d4d4d4')
    expect(vars['--terminal-scrollbar']).toBe(hexToRgba('#d4d4d4', 0.35))
  })
})

describe('TERMINAL_THEMES integrity', () => {
  // 每条 colors 必含 background/foreground/cursor + 完整 16 色（防新增主题漏字段）
  it('TerminalThemes_AllEntriesHaveCompleteColors_001', () => {
    for (const t of TERMINAL_THEMES) {
      const colors = t.colors as unknown as Record<string, string>
      for (const key of REQUIRED_KEYS) {
        expect(colors[key], `${t.id} 缺少 ${key}`).toBeTruthy()
      }
    }
  })

  // 所有 id 唯一
  it('TerminalThemes_IdsUnique_001', () => {
    const ids = TERMINAL_THEMES.map(t => t.id)
    expect(new Set(ids).size).toBe(ids.length)
  })

  // 共 16 条预设（CC-Box 2 + 第三方 14）
  it('TerminalThemes_CountIs16_001', () => {
    expect(TERMINAL_THEMES.length).toBe(16)
  })

  // 默认 id 必须在表中
  it('DefaultTerminalThemeId_ExistsInTable_001', () => {
    expect(TERMINAL_THEMES.some(t => t.id === DEFAULT_TERMINAL_THEME_ID)).toBe(true)
  })
})
