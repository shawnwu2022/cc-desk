/** Legacy 启动选项按带引号的 argv 解析；shell 操作符与变量保持字面值。 */
export function parseLegacyCustomArgs(value: string): string[] {
  const args: string[] = []
  let word = ''
  let started = false
  let quote: "'" | '"' | undefined
  for (let index = 0; index < value.length; index++) {
    const char = value[index]
    if (char === '\\' && quote !== "'") {
      let end = index + 1
      while (value[end] === '\\') end++
      const count = end - index
      const next = value[end]
      // 双引号前按反斜杠奇偶区分转义与参数边界，其他位置完整保留 UNC/Windows 分隔符。
      if (next === '"') {
        word += '\\'.repeat(Math.floor(count / 2))
        if (count % 2) word += next
        else quote = quote ? undefined : '"'
        index = end
      } else if (!quote && count === 1 && next !== undefined && (next === "'" || /\s/u.test(next))) {
        word += next
        index = end
      } else {
        word += '\\'.repeat(count)
        index = end - 1
      }
      started = true
    } else if (quote) {
      if (char === quote) quote = undefined
      else word += char
    } else if (char === "'" || char === '"') {
      quote = char
      started = true
    } else if (/\s/u.test(char)) {
      if (started) args.push(word)
      word = ''
      started = false
    } else {
      word += char
      started = true
    }
  }
  if (quote) throw new Error('LEGACY_ARGS_UNCLOSED_QUOTE')
  if (started) args.push(word)
  return args
}
