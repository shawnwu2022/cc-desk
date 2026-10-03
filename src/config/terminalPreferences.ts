import type { ITerminalOptions } from '@xterm/xterm'
import type { AppConfig } from '@/types/app'
import { isMac } from '@/utils/platform'
import { getTerminalTheme, resolveTerminalThemeId, type TerminalThemeColors } from './terminalThemes'

export const TERMINAL_FONTS = ['system', 'Cascadia Code', 'Fira Code', 'JetBrains Mono', 'Consolas', 'Menlo'] as const
export type TerminalFont = typeof TERMINAL_FONTS[number]
export type TerminalCursorStyle = 'bar' | 'block' | 'underline'
export interface TerminalSettingFields {
  terminalTheme: string
  terminalFontFamily: TerminalFont
  fontSize: number
  terminalLineHeight: number
  terminalCursorStyle: TerminalCursorStyle
  terminalCursorBlink: boolean
  webglRenderer: boolean
}
export interface TerminalPreferences {
  themeId: string
  theme: TerminalThemeColors
  fontFamily: string
  fontSize: number
  lineHeight: number
  cursorStyle: TerminalCursorStyle
  cursorBlink: boolean
  renderer: 'dom' | 'webgl'
}
export function normalizeTerminalFont(value: unknown): TerminalFont {
  return TERMINAL_FONTS.includes(value as TerminalFont) ? value as TerminalFont : 'system'
}
export function normalizeTerminalFontSize(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? Math.round(Math.min(24, Math.max(10, value))) : 12
}
export function normalizeTerminalLineHeight(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? Math.round(Math.min(2, Math.max(1, value)) * 100) / 100 : 1.2
}
export function normalizeTerminalCursor(value: unknown): TerminalCursorStyle {
  return value === 'block' || value === 'underline' ? value : 'bar'
}
export function readTerminalSettings(config: AppConfig): TerminalSettingFields {
  return {
    terminalTheme: resolveTerminalThemeId(config.terminalTheme, config.theme),
    terminalFontFamily: normalizeTerminalFont(config.terminalFontFamily), fontSize: normalizeTerminalFontSize(config.fontSize),
    terminalLineHeight: normalizeTerminalLineHeight(config.terminalLineHeight), terminalCursorStyle: normalizeTerminalCursor(config.terminalCursorStyle),
    terminalCursorBlink: typeof config.terminalCursorBlink === 'boolean' ? config.terminalCursorBlink : true,
    webglRenderer: config.webglRenderer === true,
  }
}
export function terminalPreferences(settings: TerminalSettingFields): TerminalPreferences {
  const fonts = ['Cascadia Code', 'Fira Code', 'JetBrains Mono', 'Consolas',
    ...(isMac ? ['Apple Color Emoji'] : ['Microsoft YaHei', 'Noto Sans CJK SC', 'Segoe UI Emoji'])]
  if (settings.terminalFontFamily !== 'system') fonts.unshift(settings.terminalFontFamily)
  return { themeId: settings.terminalTheme, theme: getTerminalTheme(settings.terminalTheme),
    fontFamily: [...new Set(fonts)].map(name => `"${name}"`).join(', ') + ', monospace',
    fontSize: settings.fontSize, lineHeight: settings.terminalLineHeight,
    cursorStyle: settings.terminalCursorStyle, cursorBlink: settings.terminalCursorBlink,
    renderer: settings.webglRenderer ? 'webgl' : 'dom' }
}
export function terminalAppearanceOptions(preferences: TerminalPreferences): ITerminalOptions {
  return { theme: preferences.theme, fontFamily: preferences.fontFamily, fontSize: preferences.fontSize,
    lineHeight: preferences.lineHeight, cursorStyle: preferences.cursorStyle, cursorBlink: preferences.cursorBlink }
}
/** Mutate xterm options in place. Only metric changes request a visibility-aware fit. */
export function applyTerminalAppearance(options: ITerminalOptions, next: TerminalPreferences, previous: TerminalPreferences): boolean {
  if (next.themeId !== previous.themeId) options.theme = next.theme
  if (next.cursorStyle !== previous.cursorStyle) options.cursorStyle = next.cursorStyle
  if (next.cursorBlink !== previous.cursorBlink) options.cursorBlink = next.cursorBlink
  let metricsChanged = false
  for (const key of ['fontFamily', 'fontSize', 'lineHeight'] as const) {
    if (next[key] !== previous[key]) { Object.assign(options, { [key]: next[key] }); metricsChanged = true }
  }
  return metricsChanged
}
