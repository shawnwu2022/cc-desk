import { ref } from 'vue'
import { controlActivityDiagnostics } from '@/api/tauri'
import { activityDiagnostics } from './activityDiagnostics'

export function createCaptureControl(invoke: typeof controlActivityDiagnostics) {
  const state = ref<'off' | 'starting' | 'on' | 'stopping' | 'unknown'>('off')
  let serial = 0
  const snapshotPending = ref(false)
  let startPending = false
  let stopPending = false

  async function readReport() {
    const owner = serial
    const local = activityDiagnostics.snapshot()
    // 不读取尚未确收停止的后台；快照只关联仍在本地环中的操作。
    if (state.value === 'starting' || state.value === 'stopping' || state.value === 'unknown') {
      return { ...local, backend: { available: false } }
    }
    if (snapshotPending.value) return { ...local, backend: { available: false } }
    snapshotPending.value = true
    try {
      const request = Promise.resolve().then(() => invoke({ action: 'snapshot' }))
      void request.then(() => { snapshotPending.value = false }, () => { snapshotPending.value = false })
      const reply = await withDeadline(request)
      if (owner !== serial || !reply || typeof reply !== 'object' || !('state' in reply) ||
          reply.state !== 'snapshot' || !('report' in reply)) return { ...local, backend: { available: false } }
      const report = reply.report
      if (!report || typeof report !== 'object' || !('dropped' in report) || typeof report.dropped !== 'number' ||
          !Number.isInteger(report.dropped) || report.dropped < 0 || report.dropped > 0xffffffff ||
          !('events' in report) || !Array.isArray(report.events) || report.events.length > 256) {
        return { ...local, backend: { available: false } }
      }
      const ids = new Set(local.events.map(event => event.id))
      const events = report.events.flatMap(event => {
        if (!event || typeof event !== 'object' || typeof event.id !== 'string' || !ids.has(event.id) ||
            !['backend', 'provider'].includes(event.stage) ||
            !['started', 'ok', 'returned', 'failed', 'unsupported', 'unknown', 'blocked', 'stale', 'running', 'cancelled', 'exited'].includes(event.status) ||
            typeof event.ms !== 'number' || !Number.isInteger(event.ms) || event.ms < 0 || event.ms > 0xffffffff) return []
        return [{ id: event.id, stage: event.stage as string, status: event.status as string, ms: event.ms as number }]
      })
      return { ...local, backend: { available: true, dropped: report.dropped, omitted: report.events.length - events.length, events } }
    } catch { return { ...local, backend: { available: false } } }
  }

  async function withDeadline(request: Promise<unknown>) {
    let timeout: ReturnType<typeof setTimeout> | undefined
    try {
      return await Promise.race([
        request,
        new Promise<never>((_, reject) => { timeout = setTimeout(() => reject(new Error('DIAGNOSTICS_ACK_TIMEOUT')), 5000) }),
      ])
    } finally {
      if (timeout !== undefined) clearTimeout(timeout)
    }
  }

  async function enable() {
    if (state.value !== 'off' || startPending) return
    const owner = ++serial
    state.value = 'starting'
    try {
      startPending = true
      const request = Promise.resolve().then(() => invoke({ action: 'start', serial: owner }))
      void request.then(() => { startPending = false }, () => { startPending = false })
      const reply = await withDeadline(request)
      if (owner !== serial) return
      if (!reply || typeof reply !== 'object' || !('state' in reply) || reply.state !== 'started' ||
          !('grant' in reply) || typeof reply.grant !== 'string' || !/^[0-9a-f]{32}$/.test(reply.grant)) throw new Error()
      activityDiagnostics.enable(reply.grant)
      state.value = 'on'
    } catch {
      if (owner === serial) state.value = 'unknown'
    }
  }

  async function disable() {
    if (state.value === 'off' || stopPending) return
    activityDiagnostics.disable()
    const owner = ++serial
    state.value = 'stopping'
    try {
      stopPending = true
      const request = Promise.resolve().then(() => invoke({ action: 'stop', serial: owner }))
      const accepted = request.then(reply => {
        stopPending = false
        if (!reply || typeof reply !== 'object' || !('state' in reply) || reply.state !== 'stopped') throw new Error()
        // 超时不等于没停止；迟到的本代停止应答仍可确认，绝不恢复采集。
        if (owner === serial) state.value = 'off'
      }, failure => {
        stopPending = false
        throw failure
      })
      await withDeadline(accepted)
    } catch {
      if (owner === serial) state.value = 'unknown'
    }
  }

  return { state, snapshotPending, enable, disable, readReport }
}

export const diagnosticsCaptureControl = createCaptureControl(controlActivityDiagnostics)
