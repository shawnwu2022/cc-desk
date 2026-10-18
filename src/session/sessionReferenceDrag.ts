/** 会话引用的文档内拖拽事件；坐标始终为 CSS 像素。 */
export type SessionReferenceDragEvent =
  | { phase: 'move' | 'drop'; payload: string; clientX: number; clientY: number }
  | { phase: 'cancel' }

const listeners = new Set<(event: SessionReferenceDragEvent) => void>()
let cancelCurrent: (() => void) | undefined

/**
 * 订阅当前文档内的会话引用拖拽；不经过 WebView2 的原生文件拖放处理器。
 * @param listener 接收 CSS 像素坐标及冻结的显示载荷。
 * @returns 释放该订阅的函数。
 */
export function subscribeSessionReferenceDrag(listener: (event: SessionReferenceDragEvent) => void): () => void {
  listeners.add(listener)
  return () => { listeners.delete(listener) }
}

function publish(event: SessionReferenceDragEvent) {
  for (const listener of listeners) listener(event)
}

// 指针释放后浏览器还会产生 click，不能让它恢复或激活来源行。
function suppressDragClick(pointerId: number) {
  let timer: ReturnType<typeof setTimeout> | undefined
  const clear = () => {
    window.removeEventListener('click', block, true)
    window.removeEventListener('pointerup', released, true)
    window.removeEventListener('pointerdown', clear, true)
    clearTimeout(timer)
  }
  const block = (event: MouseEvent) => {
    if (event.detail === 0) return
    event.preventDefault()
    event.stopImmediatePropagation()
    clear()
  }
  const released = (event: PointerEvent) => {
    if (event.pointerId === pointerId) timer = setTimeout(clear, 0)
  }
  // Escape/失焦取消后仍可能产生 click；保留到本次释放或下一次真实按下。
  window.addEventListener('click', block, true)
  window.addEventListener('pointerup', released, true)
  window.addEventListener('pointerdown', clear, true)
}

/**
 * 从主指针按下开始跟踪；移动超过 6 个 CSS 像素才成为拖拽，释放最多投递一次。
 * @param event 来源行收到的 pointerdown；调用方负责排除编辑器及嵌套控件。
 * @param payload 本次按下冻结的会话显示载荷。
 * @returns 取消函数；来源移除、隐藏或进入编辑态时必须调用。
 */
export function startSessionReferenceDrag(event: PointerEvent, payload: string): () => void {
  cancelCurrent?.()
  const source = event.currentTarget as HTMLElement
  const pointerId = event.pointerId
  const startX = event.clientX
  const startY = event.clientY
  let dragging = false
  let disposed = false
  const at = (phase: 'move' | 'drop', next: PointerEvent): SessionReferenceDragEvent => ({
    phase, payload, clientX: next.clientX, clientY: next.clientY,
  })
  const cleanup = () => {
    disposed = true
    if (cancelCurrent === cancel) cancelCurrent = undefined
    window.removeEventListener('pointermove', move, true)
    window.removeEventListener('pointerup', drop, true)
    window.removeEventListener('pointercancel', pointerCancel, true)
    window.removeEventListener('keydown', keydown, true)
    window.removeEventListener('blur', cancel)
    source.removeEventListener('lostpointercapture', cancel)
    if (source.hasPointerCapture?.(pointerId)) source.releasePointerCapture(pointerId)
  }
  const cancel = () => {
    if (disposed) return
    cleanup()
    if (dragging) publish({ phase: 'cancel' })
  }
  const pointerCancel = (next: PointerEvent) => { if (next.pointerId === pointerId) cancel() }
  const keydown = (next: KeyboardEvent) => {
    if (next.key !== 'Escape') return
    if (dragging) { next.preventDefault(); next.stopImmediatePropagation() }
    cancel()
  }
  const move = (next: PointerEvent) => {
    if (next.pointerId !== pointerId || disposed) return
    if (!(next.buttons & 1)) { cancel(); return }
    if (!dragging && Math.hypot(next.clientX - startX, next.clientY - startY) >= 6) {
      dragging = true
      suppressDragClick(pointerId)
    }
    if (!dragging) return
    next.preventDefault()
    publish(at('move', next))
  }
  const drop = (next: PointerEvent) => {
    if (next.pointerId !== pointerId || disposed) return
    cleanup()
    if (!dragging) return
    next.preventDefault()
    publish(at('drop', next))
  }
  cancelCurrent = cancel
  window.addEventListener('pointermove', move, true)
  window.addEventListener('pointerup', drop, true)
  window.addEventListener('pointercancel', pointerCancel, true)
  window.addEventListener('keydown', keydown, true)
  window.addEventListener('blur', cancel)
  source.addEventListener('lostpointercapture', cancel)
  source.setPointerCapture?.(pointerId)
  event.preventDefault()
  return cancel
}
