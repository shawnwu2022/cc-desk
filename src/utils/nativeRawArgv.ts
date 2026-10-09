export function parseNativeRawArgv(value: string): string[] {
  let parsed: unknown
  try {
    parsed = JSON.parse(value)
  } catch {
    throw new Error('INVALID_RAW_ARGV_JSON')
  }
  if (!Array.isArray(parsed) || parsed.some(item => typeof item !== 'string')) {
    throw new Error('INVALID_RAW_ARGV_JSON')
  }
  return [...parsed]
}
