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
  // source: https://github.com/dracula/alacritty/blob/master/dracula.toml （Dracula 官方组织仓库；基础色板见 https://github.com/dracula/dracula-theme）
  {
    id: 'dracula',
    name: 'Dracula',
    category: 'dark',
    colors: {
      background: '#282a36',
      foreground: '#f8f8f2',
      cursor: '#f8f8f2',
      cursorAccent: '#282a36',
      selectionBackground: '#44475a',
      selectionForeground: '#f8f8f2',
      black: '#21222c',
      red: '#ff5555',
      green: '#50fa7b',
      yellow: '#f1fa8c',
      blue: '#bd93f9',
      magenta: '#ff79c6',
      cyan: '#8be9fd',
      white: '#f8f8f2',
      brightBlack: '#6272a4',
      brightRed: '#ff6e6e',
      brightGreen: '#69ff94',
      brightYellow: '#ffffa5',
      brightBlue: '#d6acff',
      brightMagenta: '#ff92df',
      brightCyan: '#a4ffff',
      brightWhite: '#ffffff',
    },
  },
  // source: https://gist.github.com/brayevalerien/cb94ac685ebc186f359deae113b6710c （Monokai Pro Classic 终端 ANSI 色值，整理自 https://monokai.pro 官方主题）
  {
    id: 'monokai-pro',
    name: 'Monokai Pro',
    category: 'dark',
    colors: {
      background: '#272822',
      foreground: '#f8f8f2',
      cursor: '#f8f8f0',
      black: '#333333',
      red: '#c4265e',
      green: '#86b42b',
      yellow: '#b3b42b',
      blue: '#6a7ec8',
      magenta: '#8c6bc8',
      cyan: '#56adbc',
      white: '#e3e3dd',
      brightBlack: '#666666',
      brightRed: '#f92672',
      brightGreen: '#a6e22e',
      brightYellow: '#e2e22e',
      brightBlue: '#819aff',
      brightMagenta: '#ae81ff',
      brightCyan: '#66d9ef',
      brightWhite: '#f8f8f2',
    },
  },
  // source: https://github.com/alacritty/alacritty-theme/blob/master/themes/gruvbox_dark.toml （基于 morhetz/gruvbox 官方色板）
  {
    id: 'gruvbox-dark',
    name: 'Gruvbox Dark',
    category: 'dark',
    colors: {
      background: '#282828',
      foreground: '#ebdbb2',
      cursor: '#ebdbb2',
      black: '#282828',
      red: '#cc241d',
      green: '#98971a',
      yellow: '#d79921',
      blue: '#458588',
      magenta: '#b16286',
      cyan: '#689d6a',
      white: '#a89984',
      brightBlack: '#928374',
      brightRed: '#fb4934',
      brightGreen: '#b8bb26',
      brightYellow: '#fabd2f',
      brightBlue: '#83a598',
      brightMagenta: '#d3869b',
      brightCyan: '#8ec07c',
      brightWhite: '#ebdbb2',
    },
  },
  // source: https://github.com/alacritty/alacritty-theme/blob/master/themes/nord.toml （基于 nordtheme.com 官方色板）
  {
    id: 'nord',
    name: 'Nord',
    category: 'dark',
    colors: {
      background: '#2e3440',
      foreground: '#d8dee9',
      cursor: '#d8dee9',
      black: '#3b4252',
      red: '#bf616a',
      green: '#a3be8c',
      yellow: '#ebcb8b',
      blue: '#81a1c1',
      magenta: '#b48ead',
      cyan: '#88c0d0',
      white: '#e5e9f0',
      brightBlack: '#4c566a',
      brightRed: '#bf616a',
      brightGreen: '#a3be8c',
      brightYellow: '#ebcb8b',
      brightBlue: '#81a1c1',
      brightMagenta: '#b48ead',
      brightCyan: '#8fbcbb',
      brightWhite: '#eceff4',
    },
  },
  // source: https://github.com/folke/tokyonight.nvim/blob/main/extras/alacritty/tokyonight_night.toml （Tokyo Night 上游官方 alacritty 生成模板）
  {
    id: 'tokyo-night',
    name: 'Tokyo Night',
    category: 'dark',
    colors: {
      background: '#1a1b26',
      foreground: '#c0caf5',
      cursor: '#c0caf5',
      black: '#15161e',
      red: '#f7768e',
      green: '#9ece6a',
      yellow: '#e0af68',
      blue: '#7aa2f7',
      magenta: '#bb9af7',
      cyan: '#7dcfff',
      white: '#a9b1d6',
      brightBlack: '#414868',
      brightRed: '#ff899d',
      brightGreen: '#9fe044',
      brightYellow: '#faba4a',
      brightBlue: '#8db0ff',
      brightMagenta: '#c7a9ff',
      brightCyan: '#a4daff',
      brightWhite: '#c0caf5',
    },
  },
  // source: https://github.com/alacritty/alacritty-theme/blob/master/themes/one_dark.toml （对应 Binaryify/OneDark-Pro）
  {
    id: 'one-dark',
    name: 'One Dark',
    category: 'dark',
    colors: {
      background: '#282c34',
      foreground: '#abb2bf',
      cursor: '#abb2bf',
      black: '#1e2127',
      red: '#e06c75',
      green: '#98c379',
      yellow: '#d19a66',
      blue: '#61afef',
      magenta: '#c678dd',
      cyan: '#56b6c2',
      white: '#abb2bf',
      brightBlack: '#5c6370',
      brightRed: '#e06c75',
      brightGreen: '#98c379',
      brightYellow: '#d19a66',
      brightBlue: '#61afef',
      brightMagenta: '#c678dd',
      brightCyan: '#56b6c2',
      brightWhite: '#ffffff',
    },
  },
  // source: https://ethanschoonover.com/solarized/ + https://github.com/alacritty/alacritty-theme/blob/master/themes/solarized_dark.toml
  {
    id: 'solarized-dark',
    name: 'Solarized Dark',
    category: 'dark',
    colors: {
      background: '#002b36',
      foreground: '#839496',
      cursor: '#839496',
      black: '#073642',
      red: '#dc322f',
      green: '#859900',
      yellow: '#b58900',
      blue: '#268bd2',
      magenta: '#d33682',
      cyan: '#2aa198',
      white: '#eee8d5',
      brightBlack: '#002b36',
      brightRed: '#cb4b16',
      brightGreen: '#586e75',
      brightYellow: '#657b83',
      brightBlue: '#839496',
      brightMagenta: '#6c71c4',
      brightCyan: '#93a1a1',
      brightWhite: '#fdf6e3',
    },
  },
  // source: https://github.com/catppuccin/catppuccin （官方色板 Mocha） + https://github.com/alacritty/alacritty-theme/blob/master/themes/catppuccin_mocha.toml
  {
    id: 'catppuccin-mocha',
    name: 'Catppuccin Mocha',
    category: 'dark',
    colors: {
      background: '#1e1e2e',
      foreground: '#cdd6f4',
      cursor: '#f5e0dc',
      cursorAccent: '#1e1e2e',
      selectionBackground: '#f5e0dc',
      selectionForeground: '#1e1e2e',
      black: '#45475a',
      red: '#f38ba8',
      green: '#a6e3a1',
      yellow: '#f9e2af',
      blue: '#89b4fa',
      magenta: '#f5c2e7',
      cyan: '#94e2d5',
      white: '#bac2de',
      brightBlack: '#585b70',
      brightRed: '#f38ba8',
      brightGreen: '#a6e3a1',
      brightYellow: '#f9e2af',
      brightBlue: '#89b4fa',
      brightMagenta: '#f5c2e7',
      brightCyan: '#94e2d5',
      brightWhite: '#a6adc8',
    },
  },
  // source: https://github.com/alacritty/alacritty-theme/blob/master/themes/ayu_mirage.toml （基于 ayu-theme/ayu-colors Mirage）
  {
    id: 'ayu-mirage',
    name: 'Ayu Mirage',
    category: 'dark',
    colors: {
      background: '#1f2430',
      foreground: '#cbccc6',
      cursor: '#cbccc6',
      black: '#212733',
      red: '#f08778',
      green: '#53bf97',
      yellow: '#fdcc60',
      blue: '#60b8d6',
      magenta: '#ec7171',
      cyan: '#98e6ca',
      white: '#fafafa',
      brightBlack: '#686868',
      brightRed: '#f58c7d',
      brightGreen: '#58c49c',
      brightYellow: '#ffd165',
      brightBlue: '#65bddb',
      brightMagenta: '#f17676',
      brightCyan: '#9debcf',
      brightWhite: '#ffffff',
    },
  },
  // source: https://github.com/primer/github-vscode-theme + https://github.com/alacritty/alacritty-theme/blob/master/themes/github_dark.toml
  {
    id: 'github-dark',
    name: 'GitHub Dark',
    category: 'dark',
    colors: {
      background: '#24292e',
      foreground: '#d1d5da',
      cursor: '#d1d5da',
      black: '#586069',
      red: '#ea4a5a',
      green: '#34d058',
      yellow: '#ffea7f',
      blue: '#2188ff',
      magenta: '#b392f0',
      cyan: '#39c5cf',
      white: '#d1d5da',
      brightBlack: '#959da5',
      brightRed: '#f97583',
      brightGreen: '#85e89d',
      brightYellow: '#ffea7f',
      brightBlue: '#79b8ff',
      brightMagenta: '#b392f0',
      brightCyan: '#56d4dd',
      brightWhite: '#fafbfc',
    },
  },
  // source: https://ethanschoonover.com/solarized/ + https://github.com/alacritty/alacritty-theme/blob/master/themes/solarized_light.toml
  {
    id: 'solarized-light',
    name: 'Solarized Light',
    category: 'light',
    colors: {
      background: '#fdf6e3',
      foreground: '#586e75',
      cursor: '#586e75',
      black: '#073642',
      red: '#dc322f',
      green: '#859900',
      yellow: '#b58900',
      blue: '#268bd2',
      magenta: '#d33682',
      cyan: '#2aa198',
      white: '#eee8d5',
      brightBlack: '#002b36',
      brightRed: '#cb4b16',
      brightGreen: '#586e75',
      brightYellow: '#657b83',
      brightBlue: '#839496',
      brightMagenta: '#6c71c4',
      brightCyan: '#93a1a1',
      brightWhite: '#fdf6e3',
    },
  },
  // source: https://github.com/primer/github-vscode-theme + https://github.com/alacritty/alacritty-theme/blob/master/themes/github_light.toml
  {
    id: 'github-light',
    name: 'GitHub Light',
    category: 'light',
    colors: {
      background: '#ffffff',
      foreground: '#24292f',
      cursor: '#24292f',
      black: '#24292e',
      red: '#d73a49',
      green: '#28a745',
      yellow: '#dbab09',
      blue: '#0366d6',
      magenta: '#5a32a3',
      cyan: '#0598bc',
      white: '#6a737d',
      brightBlack: '#959da5',
      brightRed: '#cb2431',
      brightGreen: '#22863a',
      brightYellow: '#b08800',
      brightBlue: '#005cc5',
      brightMagenta: '#5a32a3',
      brightCyan: '#3192aa',
      brightWhite: '#d1d5da',
    },
  },
  // source: https://github.com/alacritty/alacritty-theme/blob/master/themes/gruvbox_light.toml （基于 morhetz/gruvbox 官方色板 light）
  {
    id: 'gruvbox-light',
    name: 'Gruvbox Light',
    category: 'light',
    colors: {
      background: '#fbf1c7',
      foreground: '#3c3836',
      cursor: '#3c3836',
      black: '#fbf1c7',
      red: '#cc241d',
      green: '#98971a',
      yellow: '#d79921',
      blue: '#458588',
      magenta: '#b16286',
      cyan: '#689d6a',
      white: '#7c6f64',
      brightBlack: '#928374',
      brightRed: '#9d0006',
      brightGreen: '#79740e',
      brightYellow: '#b57614',
      brightBlue: '#076678',
      brightMagenta: '#8f3f71',
      brightCyan: '#427b58',
      brightWhite: '#3c3836',
    },
  },
  // source: https://github.com/catppuccin/catppuccin （官方色板 Latte） + https://github.com/alacritty/alacritty-theme/blob/master/themes/catppuccin_latte.toml
  {
    id: 'catppuccin-latte',
    name: 'Catppuccin Latte',
    category: 'light',
    colors: {
      background: '#eff1f5',
      foreground: '#4c4f69',
      cursor: '#dc8a78',
      cursorAccent: '#eff1f5',
      selectionBackground: '#dc8a78',
      selectionForeground: '#eff1f5',
      black: '#5c5f77',
      red: '#d20f39',
      green: '#40a02b',
      yellow: '#df8e1d',
      blue: '#1e66f5',
      magenta: '#ea76cb',
      cyan: '#179299',
      white: '#acb0be',
      brightBlack: '#6c6f85',
      brightRed: '#d20f39',
      brightGreen: '#40a02b',
      brightYellow: '#df8e1d',
      brightBlue: '#1e66f5',
      brightMagenta: '#ea76cb',
      brightCyan: '#179299',
      brightWhite: '#bcc0cc',
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
    '--terminal-surface-fg': colors.foreground,
    '--terminal-scrollbar': hexToRgba(colors.foreground, 0.35),
  }
}
