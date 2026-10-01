import { TERMINAL_THEMES } from '@/config/terminalThemes'
import { TERMINAL_FONTS } from '@/config/terminalPreferences'
import type { Platform } from '@/utils/platform'
const finite = (value: unknown, fallback: number, min: number, max: number) => typeof value === 'number' && Number.isFinite(value) ? Math.max(min, Math.min(max, value)) : fallback
const enumValue = <T extends string>(value: unknown, allowed: readonly T[], fallback: T): T => allowed.includes(value as T) ? value as T : fallback
/** Fresh object with explicit scalar fields only; never spreads runtime/config objects. */
export function safeAppDiagnostics(input: {
  version: unknown; commit: unknown; platform: unknown
  gui: { mode: unknown; density: unknown; sidebarWidth: unknown }
  terminal: { theme: unknown; font: unknown; size: unknown; lineHeight: unknown; cursor: unknown; blink: unknown; renderer: unknown }
  sessions: { open: unknown; running: unknown; starting: unknown; unknown: unknown }
}) {
  return {
    product: 'CC Desk',
    version: typeof input.version === 'string' && /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(input.version) && input.version.length <= 64 ? input.version : 'unknown',
    buildCommit: typeof input.commit === 'string' && /^[0-9a-f]{40}$/.test(input.commit) ? input.commit : 'unknown',
    platform: enumValue<Platform>(input.platform, ['windows', 'macos', 'linux', 'unknown'], 'unknown'),
    gui: { mode: enumValue(input.gui.mode, ['light', 'dark', 'system'], 'light'), density: enumValue(input.gui.density, ['standard', 'compact'], 'standard'), sidebarWidth: finite(input.gui.sidebarWidth, 288, 240, 360) },
    terminal: { theme: enumValue(input.terminal.theme, TERMINAL_THEMES.map(theme => theme.id), 'cc-box-light'),
      font: enumValue(input.terminal.font, TERMINAL_FONTS, 'system'), size: finite(input.terminal.size, 12, 10, 24), lineHeight: finite(input.terminal.lineHeight, 1.2, 1, 2),
      cursor: enumValue(input.terminal.cursor, ['bar', 'block', 'underline'], 'bar'), blink: input.terminal.blink === true, renderer: input.terminal.renderer === true ? 'webgl-next-terminal' : 'dom' },
    sessions: { open: finite(input.sessions.open, 0, 0, 100000), running: finite(input.sessions.running, 0, 0, 100000),
      starting: finite(input.sessions.starting, 0, 0, 100000), unknown: finite(input.sessions.unknown, 0, 0, 100000) },
  }
}
