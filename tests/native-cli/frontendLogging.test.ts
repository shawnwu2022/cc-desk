import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const read = (path: string) => readFileSync(path, 'utf8')

describe('Frontend logging privacy boundary', () => {
  // 检查共享 IPC 入口的每个日志出口只接收摘要，不输出 Legacy 或 Native 原文。
  it('FrontendLogging_SummaryOnly_001', () => {
    const command = read('src-tauri/src/commands.rs').match(
      /pub async fn log_message\(level: String, message: String\) \{[\s\S]*?^\}/m,
    )?.[0]
    expect(command).toBeDefined()
    expect(command).toContain('frontend_message_summary(&message)')
    expect(command).not.toMatch(/log::(?:error|warn|info|debug)!\([\s\S]*?\bmessage\b[\s\S]*?\)/)
    const sinks = [...command!.matchAll(/log::(?:error|warn|info|debug)!\(([^)]*)\)/g)]
    expect(sinks.map(match => match[1].trim())).toEqual(
      Array(5).fill('"[Frontend] {}", summary'),
    )
  })

  // 检查已有四种级别和未知级别的 Info 回退保持原来的 Frontend 范围。
  it('FrontendLogging_LevelContract_002', () => {
    const command = read('src-tauri/src/commands.rs').match(
      /pub async fn log_message\(level: String, message: String\) \{[\s\S]*?^\}/m,
    )?.[0]
    expect(command).toBeDefined()
    const branches = [...command!.matchAll(/("(?:error|warn|info|debug)"|_)\s*=>\s*log::(error|warn|info|debug)!\(/g)]
    expect(branches.map(match => [match[1], match[2]])).toEqual([
      ['"error"', 'error'],
      ['"warn"', 'warn'],
      ['"info"', 'info'],
      ['"debug"', 'debug'],
      ['_', 'info'],
    ])
    expect(command!.match(/"\[Frontend\] \{\}"/g)).toHaveLength(5)
  })
})
