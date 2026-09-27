import { describe, expect, it } from 'vitest'
import { parseNativeRawArgv } from '@/utils/nativeRawArgv'

describe('D23 native raw argv UI parser', () => {
  it('preserves empty, unicode and shell-looking arguments verbatim', () => {
    expect(parseNativeRawArgv('["", "中文", "|", ">", "$HOME", "--future"]')).toEqual([
      '',
      '中文',
      '|',
      '>',
      '$HOME',
      '--future',
    ])
  })

  it('rejects non-array and non-string values without shell parsing', () => {
    expect(() => parseNativeRawArgv('"--help"')).toThrow('INVALID_RAW_ARGV_JSON')
    expect(() => parseNativeRawArgv('["--help", 1]')).toThrow('INVALID_RAW_ARGV_JSON')
    expect(() => parseNativeRawArgv('not-json')).toThrow('INVALID_RAW_ARGV_JSON')
  })
})
