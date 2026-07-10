import { describe, it, expect, beforeEach } from 'vitest'
import { applyThemeToDom } from '@/utils/theme'

describe('applyThemeToDom', () => {
  beforeEach(() => {
    // 每个用例前重置 DOM 状态，避免互相干扰
    document.documentElement.classList.remove('light', 'dark')
    document.documentElement.removeAttribute('data-theme')
  })

  // 应用 dark 主题：data-theme 属性 = dark，且 html 含 dark class
  it('ApplyTheme_Dark_SetsAttrAndClass_001', () => {
    applyThemeToDom('dark')
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')
    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })

  // 应用 light 主题：data-theme 属性 = light，且 html 含 light class
  it('ApplyTheme_Light_SetsAttrAndClass_001', () => {
    applyThemeToDom('light')
    expect(document.documentElement.getAttribute('data-theme')).toBe('light')
    expect(document.documentElement.classList.contains('light')).toBe(true)
  })

  // 从 dark 切换到 light：旧 dark class 被移除，data-theme 与新 class 一致
  it('ApplyTheme_Switch_RemovesOldClass_001', () => {
    applyThemeToDom('dark')
    applyThemeToDom('light')
    expect(document.documentElement.classList.contains('dark')).toBe(false)
    expect(document.documentElement.classList.contains('light')).toBe(true)
    expect(document.documentElement.getAttribute('data-theme')).toBe('light')
  })

  // 从 light 切换到 dark：旧 light class 被移除，data-theme 与新 class 一致
  it('ApplyTheme_SwitchReverse_RemovesOldClass_001', () => {
    applyThemeToDom('light')
    applyThemeToDom('dark')
    expect(document.documentElement.classList.contains('light')).toBe(false)
    expect(document.documentElement.classList.contains('dark')).toBe(true)
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')
  })
})
