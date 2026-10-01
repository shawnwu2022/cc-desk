import { describe, it, expect } from 'vitest'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import { readFileSync, readdirSync } from 'node:fs'
import { createSourceFile, ScriptTarget, isExportAssignment, isObjectLiteralExpression, isPropertyAssignment, isIdentifier, isStringLiteral } from 'typescript'

/** 获取对象的全部顶层 key */
function getKeys(obj: Record<string, unknown>): string[] {
  return Object.keys(obj)
}

describe('i18n 翻译文件一致性', () => {
  // en 和 zh 的 key 结构完全一致
  it('I18n_KeysMatch_001', () => {
    const enKeys = getKeys(en).sort()
    const zhKeys = getKeys(zh).sort()
    expect(enKeys).toEqual(zhKeys)
  })

  // 没有空值
  it('I18n_NoEmptyValues_001', () => {
    for (const [key, value] of Object.entries(en)) {
      expect(value, `en.${key} should not be empty`).toBeTruthy()
    }
    for (const [key, value] of Object.entries(zh)) {
      expect(value, `zh.${key} should not be empty`).toBeTruthy()
    }
  })

  // 插值占位符在两个 locale 中匹配
  it('I18n_InterpolationMatch_001', () => {
    const placeholderPattern = /\{(\w+)\}/g
    for (const key of getKeys(en) as Array<keyof typeof en>) {
      const enPlaceholders = [...(en[key] as string).matchAll(placeholderPattern)].map(m => m[1]).sort()
      const zhPlaceholders = [...(zh[key] as string).matchAll(placeholderPattern)].map(m => m[1]).sort()
      expect(zhPlaceholders, `zh.${key} placeholders should match en`).toEqual(enPlaceholders)
    }
  })

  // 没有重复 key（对象字面量中后面的会覆盖前面的）
  it('I18n_NoDuplicateKeys_001', () => {
    for (const locale of ['en', 'zh']) {
      const source = createSourceFile(locale + '.ts', readFileSync('src/i18n/locales/' + locale + '.ts', 'utf8'), ScriptTarget.Latest)
      const exportNode = source.statements.find(isExportAssignment)!
      expect(isObjectLiteralExpression(exportNode.expression)).toBe(true)
      if (!isObjectLiteralExpression(exportNode.expression)) throw new Error('Locale must export an object')
      const keys = exportNode.expression.properties.map(property => {
        if (!isPropertyAssignment(property) || !(isIdentifier(property.name) || isStringLiteral(property.name))) throw new Error('Unexpected locale entry')
        return property.name.text
      })
      expect(keys.filter((key, index) => keys.indexOf(key) !== index), locale + ' contains duplicate source keys').toEqual([])
    }
  })
})


// 实际活跃界面的字面量翻译引用须同时存在，不能由 fallback 隐藏拼写错误。
it('I18n_ActiveSurfaceKeys_002', () => {
  function files(directory: string): string[] { return readdirSync(directory, { withFileTypes: true }).flatMap(entry => entry.isDirectory() ? files(directory + '/' + entry.name) : entry.name.endsWith('.vue') ? [directory + '/' + entry.name] : []) }
  const sources = ['ui', 'shell', 'workspace', 'sessions', 'projects', 'settings'].flatMap(directory => files('src/components/' + directory))
  for (const file of sources) for (const match of readFileSync(file, 'utf8').matchAll(/\bt\(\s*['"]([A-Za-z][A-Za-z0-9_]*)['"]/g)) {
    expect(Object.prototype.hasOwnProperty.call(en, match[1]), file + ': en.' + match[1]).toBe(true)
    expect(Object.prototype.hasOwnProperty.call(zh, match[1]), file + ': zh.' + match[1]).toBe(true)
  }
})
