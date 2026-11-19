// Real document bridge. No handler or call exists here for modifying a session.
async function runOwnedDeletePreview(target, fixture) {
  const bridge = window.__CC_DESK_DOCUMENT__
  let phase = 'scope'
  const result = (ok, stage) => window.__TAURI_INTERNALS__.invoke('preview_fixture_finish', { ok, stage })
  const select = response => {
    if (response.state !== 'ready' || response.hasMore || response.historyMetadataIncomplete || response.historyReadFailures?.length) throw Error()
    const one = (id, archived) => {
      const found = response.items.filter(row => row.type === 'session' && row.nativeSessionId === id)
      if (found.length !== 1 || found[0].archived !== archived || found[0].cwd !== fixture.cwd || !found[0].sessionKey) throw Error()
      return found[0]
    }
    return [one(fixture.active, false), one(fixture.archived, true)]
  }
  const read = (source, requestEpoch) => bridge.invoke('native_list_resources', {source, resourceKind:'history', requestEpoch})
  try {
    const source = await bridge.invoke('native_get_scope', target)
    phase = 'before-list'
    const [active, archived] = select(await read(source, '1'))
    phase = 'active-preview'
    const activeId = crypto.randomUUID()
    const denied = await bridge.invoke('cli_preview_session_delete', {operationId:activeId, source, sessionKey:active.sessionKey})
    if (denied.operationId !== activeId || denied.state !== 'blocked' || denied.previewId !== null ||
        denied.reason !== 'DELETE_ROOT_NOT_ARCHIVED') throw Error()
    phase = 'archived-preview'
    const archivedId = crypto.randomUUID()
    const preview = await bridge.invoke('cli_preview_session_delete', {operationId:archivedId, source, sessionKey:archived.sessionKey})
    if (preview.operationId !== archivedId || preview.state !== 'ready' || preview.nativeSessionId !== fixture.archived || preview.cli !== 'codex' ||
        preview.scope !== 'native-subtree' || preview.rootArchived !== true || typeof preview.previewId !== 'string' || !preview.previewId || preview.reason !== null) throw Error()
    phase = 'after-list'
    const [activeAfter, archivedAfter] = select(await read(source, '2'))
    if (activeAfter.sessionKey !== active.sessionKey || archivedAfter.sessionKey !== archived.sessionKey) throw Error()
    await result(true, 'complete')
  } catch { await result(false, phase) }
}
