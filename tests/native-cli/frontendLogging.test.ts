import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const read = (path: string) => readFileSync(path, 'utf8')
const frontendSink = () => read('src-tauri/src/logger.rs').match(
  /pub\(crate\) fn log_frontend_message\([^\n]+\) \{[\s\S]*?^\}/m,
)?.[0]

describe('Frontend logging privacy boundary', () => {
  // Wiring checks complement the executable Rust privacy/schema/file-sink tests.
  it('FrontendLogging_SummaryOnly_001', () => {
    const command = read('src-tauri/src/commands.rs').match(
      /pub async fn log_message\([^\n]+\) \{[\s\S]*?^\}/m,
    )?.[0]
    expect(command).toContain('webview: tauri::WebviewWindow')
    expect(command).toContain('crate::logger::log_frontend_message(webview.label(), &level, &message);')
    expect(command).not.toMatch(/log::(?:error|warn|info|debug)!/)
    const sink = frontendSink()
    expect(sink).toContain('budget.format(caller, message)')
    expect(sink).toContain('let summary = frontend_message_summary(message);')
    expect(sink).not.toMatch(/log::(?:error|warn|info|debug)!\([^)]*\bmessage\b/)
    expect(sink!.match(/"\[Frontend\] \{summary\}"/g)).toHaveLength(4)
  })

  it('FrontendLogging_LevelContract_002', () => {
    const sink = frontendSink()
    expect(sink).toBeDefined()
    const branches = [...sink!.matchAll(/("(?:error|warn|info|debug)"|_)\s*=>\s*log::(error|warn|info|debug)!\(/g)]
    // Explicit info and unknown levels both use the unchanged default info sink.
    expect(branches.map(match => [match[1], match[2]])).toEqual([
      ['"error"', 'error'], ['"warn"', 'warn'], ['"debug"', 'debug'], ['_', 'info'],
    ])
  })
})
