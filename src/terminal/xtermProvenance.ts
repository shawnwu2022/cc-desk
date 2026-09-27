export interface DisposableLike {
  dispose(): void
}

export interface XtermProvenanceSource {
  onData(listener: (data: string) => void): DisposableLike
  onBinary(listener: (data: string) => void): DisposableLike
  _core?: {
    coreService?: {
      onUserInput?: (listener: () => void) => DisposableLike
    }
  }
}

export interface XtermInputRoutes {
  user(data: string): Promise<void> | void
  protocol(data: string): Promise<void> | void
  binary(data: string): Promise<void> | void
}

export interface XtermProvenanceBinding extends DisposableLike {
  drain(): Promise<void>
}

/**
 * xterm 5.5 keeps the wasUserInput bit internally: triggerDataEvent(data, true)
 * fires coreService.onUserInput immediately before the corresponding onData.
 * Terminal-generated replies use triggerDataEvent without that user signal.
 *
 * The public onData API erases this distinction, so D19 uses the existing
 * pinned xterm 5.5 internal signal as a version-locked provenance tap. If the
 * runtime shape changes, binding fails closed instead of falling back to
 * escape-sequence/content heuristics.
 */
export function bindXtermInputProvenance(
  term: XtermProvenanceSource,
  routes: XtermInputRoutes,
): XtermProvenanceBinding {
  const onUserInput = term._core?.coreService?.onUserInput
  if (typeof onUserInput !== 'function') {
    throw new Error('XTERM_USER_INPUT_PROVENANCE_UNAVAILABLE')
  }

  let pendingUserSignals = 0
  let disposed = false
  const pending = new Set<Promise<void>>()
  let hasFailure = false
  let firstFailure: unknown

  const track = (operation: Promise<void> | void) => {
    const task = Promise.resolve(operation)
    pending.add(task)
    // Do not create a detached rejected promise via finally(). drain() owns
    // observation of route failures; this completion hook only maintains the set.
    void task.then(
      () => pending.delete(task),
      error => {
        pending.delete(task)
        if (!hasFailure) {
          hasFailure = true
          firstFailure = error
        }
      },
    )
  }

  const userDisposable = onUserInput(() => {
    if (!disposed) pendingUserSignals += 1
  })

  const dataDisposable = term.onData(data => {
    if (disposed) return
    if (pendingUserSignals > 0) {
      pendingUserSignals -= 1
      track(routes.user(data))
      return
    }
    track(routes.protocol(data))
  })

  const binaryDisposable = term.onBinary(data => {
    if (!disposed) track(routes.binary(data))
  })

  return {
    async drain() {
      while (pending.size > 0) {
        await Promise.all([...pending].map(task => task.catch(() => undefined)))
      }
      if (hasFailure) {
        const error = firstFailure
        hasFailure = false
        firstFailure = undefined
        throw error
      }
    },

    dispose() {
      if (disposed) return
      disposed = true
      pendingUserSignals = 0
      dataDisposable.dispose()
      binaryDisposable.dispose()
      userDisposable.dispose()
    },
  }
}
