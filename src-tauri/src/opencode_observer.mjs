// 仅发送冻结 owned run 的有界状态元数据；不读取提示词、工具内容或凭证正文。
export const CCDeskObserver = async (input, options = {}) => {
  const record = value => value !== null && typeof value === 'object' && !Array.isArray(value)
  const validDirectory = value => typeof value === 'string' && value.length > 0 && value.length <= 32768 && !/[\u0000-\u001f]/.test(value)
  if (!record(input) || !record(options) || !validDirectory(input.directory) || !validDirectory(options.directory)) return {}
  const { client, directory } = input
  const env = process.env
  const port = env.CC_BOX_HOOK_PORT, run = env.CC_DESK_OBSERVER_RUN, generation = env.CC_DESK_OBSERVER_GENERATION, capability = env.CC_DESK_OBSERVER_CAPABILITY
  if (!/^[0-9]+$/.test(port ?? '') || Number(port) < 1 || Number(port) > 65535
    || !run || !/^[1-9][0-9]*$/.test(generation ?? '') || !/^[A-Za-z0-9_-]{32,128}$/.test(capability ?? '')) return {}
  const validID = id => typeof id === 'string' && /^ses_[A-Za-z0-9_]+$/.test(id) && id.length <= 256
  if (options.sessionID != null && !validID(options.sessionID)) return {}
  const normalize = value => { const text = String(value ?? '').replaceAll('\\', '/').replace(/\/+$/, ''); return /^[A-Za-z]:/.test(text) ? text.toLowerCase() : text }
  let selected = options.sessionID ?? null, busy = 'unknown', failed = false, halted = false, disposed = false, pending = 0, announced = false
  const metadata = new Map(), permissions = new Set(), questions = new Set(), retired = new Set(), seen = new Set()
  const counterKey = Symbol.for(`cc-desk-opencode-sequence:${run}:${generation}`)
  const counter = globalThis[counterKey] ??= { value: 0 }
  let queue = Promise.resolve(), lookup = null
  async function send(status, start = false, final = false) {
    if (disposed || halted && !final || !announced || !selected || !Number.isSafeInteger(counter.value) || counter.value >= Number.MAX_SAFE_INTEGER) return
    const eventID = `o${String(++counter.value).padStart(16, '0')}`
    try {
      await fetch(`http://127.0.0.1:${port}/observer`, { method: 'POST', redirect: 'error', signal: AbortSignal.timeout(700),
        headers: { 'content-type': 'application/json', 'x-cc-desk-run': run, 'x-cc-desk-generation': generation,
          'x-cc-desk-capability': capability, 'x-cc-desk-event': eventID, 'x-cc-desk-observer-source': 'opencode-hook' },
        body: JSON.stringify(start ? { hook_event_name: 'SessionStart', session_id: selected }
          : { hook_event_name: 'Notification', session_id: selected, notification_type: `opencode_${status}` }) })
    } catch { /* 可选观察失败不阻断 CLI，也不自动重发过期状态。 */ }
  }
  async function info(id, offered) {
    if (halted || !validID(id) || selected && id !== selected) return null
    let value = offered ?? metadata.get(id)
    if (!value) {
      // 忽略取消的 SDK 请求真实结束前不再启动查询；事件自带 metadata 仍可使用。
      if (lookup) return null
      const query = Promise.resolve().then(() => client.session.get({ path: { id }, query: { directory }, signal: AbortSignal.timeout(1000) }))
      lookup = { id, directory, query }
      const settled = () => { if (lookup?.query === query) lookup = null }
      query.then(settled, settled)
      let timer
      try {
        // SDK 可能忽略 signal；本地期限仍结束等待，不重试或扩大查询范围。
        const response = await Promise.race([
          query,
          new Promise(resolve => { timer = setTimeout(() => resolve(null), 1000); timer.unref?.() }),
        ])
        value = record(response) ? response.data : null
      }
      catch { return null }
      finally { clearTimeout(timer) }
    }
    if (halted || !record(value) || value.id !== id || value.parentID != null || !validDirectory(value.directory)) return null
    // resume 可进入会话记录目录；metadata 仍必须属于当前 CLI 实例目录。
    if (normalize(value.directory) !== normalize(directory) || !selected && normalize(directory) !== normalize(options.directory)) return null
    if (metadata.size >= 128 && !metadata.has(id)) return null
    metadata.set(id, { id, directory: value.directory })
    if (!selected) selected = id
    if (!announced) { announced = true; await send('unknown', true) }
    return halted ? null : value
  }
  async function invalidate() { halted = true; await send('unknown', false, true) }
  async function retire(key) {
    if (retired.size >= 4096 && !retired.has(key)) { await invalidate(); return false }
    retired.add(key)
    return true
  }
  async function retirePending() {
    // 1.19 取消会清理 ask 而不发 replied；官方 idle 终止本轮等待，错误原因另行保留。
    for (const [prefix, set] of [['p', permissions], ['q', questions]]) {
      for (const id of set) if (!await retire(`${prefix}:${id}`)) return
      set.clear()
    }
  }
  const publish = () => send(failed ? 'error' : permissions.size ? 'permission' : questions.size ? 'question' : busy)
  async function handle(event) {
    const p = event?.properties
    if (!record(p)) return
    const id = p.sessionID ?? p.info?.id
    if (event.type === 'session.created' || event.type === 'session.updated') {
      if (event.type === 'session.updated' && !selected) return
      if (await info(id, p.info)) await publish()
      return
    }
    if (!await info(id)) return
    if (event.type === 'session.status') {
      busy = ['idle', 'busy', 'retry'].includes(p.status?.type) ? p.status.type : 'unknown'
      if (busy === 'busy' || busy === 'retry') failed = false
      if (busy === 'idle') await retirePending()
    } else if (event.type === 'session.idle') {
      // 老事件没有更多完成证据；只保留其官方空闲语义。
      busy = 'idle'
      await retirePending()
    } else if (event.type === 'permission.asked' || event.type === 'question.asked') {
      const key = `${event.type.startsWith('permission') ? 'p' : 'q'}:${p.id}`
      if (typeof p.id !== 'string' || p.id.length > 256 || retired.has(key)) return
      const set = event.type.startsWith('permission') ? permissions : questions
      if (set.size >= 128 && !set.has(p.id)) { await invalidate(); return }
      set.add(p.id)
    } else if (['permission.replied', 'question.replied', 'question.rejected'].includes(event.type)) {
      if (typeof p.requestID !== 'string' || p.requestID.length > 256) return
      const permission = event.type.startsWith('permission'), key = `${permission ? 'p' : 'q'}:${p.requestID}`
      if (!await retire(key)) return
      ;(permission ? permissions : questions).delete(p.requestID)
    } else if (event.type === 'session.error') failed = true
    else return
    await publish()
  }
  // 初始化时同步请求自身 API 可能等待 Plugin.init；返回 hooks 后再查询，避免自等待。
  const initial = selected ? setTimeout(() => {
    queue = queue.then(() => halted ? undefined : info(selected)).catch(() => {})
  }, 0) : null
  initial?.unref?.()
  const supported = new Set(['session.created', 'session.updated', 'session.status', 'session.idle', 'session.error',
    'permission.asked', 'permission.replied', 'question.asked', 'question.replied', 'question.rejected'])
  return { dispose: () => { disposed = true; halted = true; if (initial) clearTimeout(initial) }, event: message => {
    const event = record(message) ? message.event : null
    // token delta 等高频无关载荷不得进入有界状态队列或去重账本。
    if (halted || !record(event) || !record(event.properties) || !supported.has(event.type)) return Promise.resolve()
    if (typeof event.id !== 'string' || !event.id || event.id.length > 128 || /[\u0000-\u001f]/.test(event.id)) return Promise.resolve()
    if (seen.has(event.id)) return Promise.resolve()
    if (pending >= 128 || seen.size >= 4096) {
      halted = true
      queue = queue.then(() => send('unknown', false, true))
      return queue
    }
    seen.add(event.id)
    pending++
    queue = queue.then(() => halted ? undefined : handle(event)).catch(invalidate).finally(() => { pending-- })
    return queue
  } }
}
