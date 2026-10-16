import { getSessionInfo, renameSession } from '@anthropic-ai/claude-agent-sdk'
import { normalize } from 'node:path'

const samePath = (left, right) => {
  const value = path => normalize(path).replace(/[\\/]+$/, '')
  return process.platform === 'win32' ? value(left).toLowerCase() === value(right).toLowerCase() : value(left) === value(right)
}
async function main() {
let issued = false
try {
  let input = ''
  for await (const chunk of process.stdin) {
    input += chunk
    if (Buffer.byteLength(input) > 32768) throw new Error('INVALID_REQUEST')
  }
  const request = JSON.parse(input)
  if (!request || Object.keys(request).sort().join() !== 'expectedCwd,nativeSessionId,title'
    || typeof request.nativeSessionId !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(request.nativeSessionId)
    || typeof request.title !== 'string' || !request.title.trim() || request.title !== request.title.trim() || Buffer.byteLength(request.title) > 1024 || /\p{Cc}/u.test(request.title)
    || typeof request.expectedCwd !== 'string' || !request.expectedCwd || /\p{Cc}/u.test(request.expectedCwd)) throw new Error('INVALID_REQUEST')
  const before = await getSessionInfo(request.nativeSessionId)
  if (!before || before.sessionId !== request.nativeSessionId || !before.cwd || !samePath(before.cwd, request.expectedCwd)) throw new Error('SESSION_NOT_FOUND')
  issued = true
  let writeFailed = false
  try { await renameSession(request.nativeSessionId, request.title) } catch { writeFailed = true }
  const after = await getSessionInfo(request.nativeSessionId)
  if (!after || after.sessionId !== request.nativeSessionId || after.customTitle !== request.title || !after.cwd || !samePath(after.cwd, request.expectedCwd)) throw new Error(writeFailed ? 'RENAME_OUTCOME_UNKNOWN' : 'RENAME_VERIFICATION_FAILED')
  process.stdout.write(JSON.stringify({ state:'renamed',nativeSessionId:request.nativeSessionId,title:request.title,cwd:after.cwd })+'\n')
} catch (error) {
  const known = ['INVALID_REQUEST','SESSION_NOT_FOUND','RENAME_VERIFICATION_FAILED']
  const code = error instanceof Error && known.includes(error.message) ? error.message : issued ? 'RENAME_OUTCOME_UNKNOWN' : 'RENAME_FAILED'
  process.stdout.write(JSON.stringify({ state:'failed',code })+'\n')
  process.exitCode = 1
}
}
void main()
