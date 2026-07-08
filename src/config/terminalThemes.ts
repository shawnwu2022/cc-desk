/**
 * 终端主题预设表
 * 与 GUI 浅/暗主题完全独立，仅驱动 xterm 字符栅格 + 终端容器表面色
 */

/** 终端配色（xterm ITheme 子集，补 selectionInactiveBackground） */
export interface TerminalThemeColors {
  background: string
  foreground: string
  cursor: string
  cursorAccent?: string
  selectionBackground?: string
  selectionForeground?: string
  selectionInactiveBackground?: string
  // 16 色 ANSI
  black: string
  red: string
  green: string
  yellow: string
  blue: string
  magenta: string
  cyan: string
  white: string
  brightBlack: string
  brightRed: string
  brightGreen: string
  brightYellow: string
  brightBlue: string
  brightMagenta: string
  brightCyan: string
  brightWhite: string
}

/** 终端主题预设 */
export interface TerminalTheme {
  id: string
  name: string                              // 显示名（品牌名，不走 i18n）
  category: 'builtin' | 'dark' | 'light'    // 下拉分组
  colors: TerminalThemeColors
}

/** 容错回退默认主题 id（配置非法时使用；与首次迁移推断独立） */
export const DEFAULT_TERMINAL_THEME_ID = 'cc-box-dark'

export const TERMINAL_THEMES: TerminalTheme[] = [
  {
    id: 'cc-box-light',
    name: 'CC-Box Light',
    category: 'builtin',
    colors: {
      background: '#f8f9fa',
      foreground: '#1a1a2e',
      cursor: '#6c5ce7',
      cursorAccent: '#f8f9fa',
      selectionBackground: '#ede9fe',
      selectionForeground: '#1a1a2e',
      black: '#1a1a2e',
      red: '#e74c3c',
      green: '#27ae60',
      yellow: '#f39c12',
      blue: '#3498db',
      magenta: '#9b59b6',
      cyan: '#1abc9c',
      white: '#ecf0f1',
      brightBlack: '#2c3e50',
      brightRed: '#e74c3c',
      brightGreen: '#27ae60',
      brightYellow: '#f39c12',
      brightBlue: '#3498db',
      brightMagenta: '#9b59b6',
      brightCyan: '#1abc9c',
      brightWhite: '#ffffff',
    },
  },
  {
    id: 'cc-box-dark',
    name: 'CC-Box Dark',
    category: 'builtin',
    colors: {
      background: '#1e1e1e',
      foreground: '#d4d4d4',
      cursor: '#f0d4a8',
      cursorAccent: '#1e1e1e',
      selectionBackground: '#4a7aad40',
      selectionForeground: '#d4d4d4',
      black: '#1c1a17',
      red: '#e8705a',
      green: '#5dad8e',
      yellow: '#f0b460',
      blue: '#4a7aad',
      magenta: '#b8956a',
      cyan: '#5dad8e',
      white: '#c4c0b8',
      brightBlack: '#3a3734',
      brightRed: '#f0886e',
      brightGreen: '#7abd9e',
      brightYellow: '#f8ca80',
      brightBlue: '#6a9acd',
      brightMagenta: '#d0a87e',
      brightCyan: '#7abd9e',
      brightWhite: '#f8f6f3',
    },
  },
]

/** 归一化主题 id：非法/空 → 默认 */
export function normalizeTerminalThemeId(id: string | undefined): string {
  if (id && TERMINAL_THEMES.some(t => t.id === id)) return id
  return DEFAULT_TERMINAL_THEME_ID
}

/** 取主题配色：内部先归一化，保证返回合法 colors */
export function getTerminalTheme(id: string | undefined): TerminalThemeColors {
  const normalized = normalizeTerminalThemeId(id)
  return TERMINAL_THEMES.find(t => t.id === normalized)!.colors
}

/** hex(#rrggbb) → rgba 字符串；非合法 hex 原样返回（容错） */
export function hexToRgba(hex: string, alpha: number): string {
  const m = /^#?([0-9a-fA-F]{6})$/.exec(hex.trim())
  if (!m) return hex
  const n = parseInt(m[1], 16)
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`
}

/** 终端表面色 CSS 变量：供容器/滚动条/空态绑定，随终端主题变化 */
export function computeTerminalSurfaceVars(colors: TerminalThemeColors): Record<string, string> {
  return {
    '--terminal-surface-bg': colors.background,
    '--terminal-scrollbar': hexToRgba(colors.foreground, 0.35),
  }
}
