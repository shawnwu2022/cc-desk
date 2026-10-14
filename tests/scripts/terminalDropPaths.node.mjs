import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'

// 执行真实 SFC 的 drop 函数及共享生产 formatter；不启动宿主或替换路径算法。
const require = createRequire(import.meta.url)
const Module = require('node:module')
const { transpileProductionSource } = require('../../.github/scripts/transpile_production_source.cjs')
const sourceRoot = resolve('src')
const cache = new Map()
function loadSource(filename) {
  if (cache.has(filename)) return cache.get(filename).exports
  const m = new Module(filename)
  m.filename = filename
  cache.set(filename, m)
  m.require = name => {
    // 只固定浏览器平台输入；路径与 formatter 仍执行真实生产模块。
    if (name === '@/utils/platform') return { isWindows: false, isMac: false }
    const dependency = name.startsWith('@/') ? join(sourceRoot, name.slice(2))
      : name.startsWith('.') ? resolve(dirname(filename), name) : undefined
    assert.ok(dependency, 'unexpected non-production import: ' + name)
    return loadSource(dependency + '.ts')
  }
  m._compile(transpileProductionSource(readFileSync(filename, 'utf8')), filename)
  return m.exports
}
const { relativizePath } = loadSource(resolve('src/utils/path.ts'))
const { selectSessionMenuActions } = loadSource(resolve('src/utils/sessionPresentation.ts'))
const { formatDroppedPaths } = loadSource(resolve('src/utils/terminalDropPaths.ts'))

function controller(kind, projectPath = '/repo') {
  const file = kind === 'native' ? 'NativeCliTerminal.vue' : 'XTermTerminal.vue'
  const source = readFileSync(resolve('src/components', file), 'utf8')
  const body = source.match(/function dropPaths\(paths: string\[\]\): boolean \{([\s\S]*?)\n\}/)?.[1]
  assert.ok(body, 'production drop function must be discoverable')
  const writes = []
  const send = text => { writes.push(text); return true }
  const drop = Function('paths', 'props', 'inputEnabled', 'term', 'binding', 'tabs',
    'currentAttempt', 'markInputFailure', 'visible', 'currentDisplayTabId',
    'sessionStore', 'sendText', 'relativizePath', 'formatDroppedPaths', body)
  return { writes, drop: paths => drop(paths, { active: true }, true, {},
    { sendUserText: async text => { send(text) } }, { tab: () => ({ projectPath }) },
    () => ({}), () => {}, { value: true }, { value: 'tab' },
    { tabs: new Map([['tab', { projectPath }]]) }, send, relativizePath, formatDroppedPaths) }
}

for (const kind of ['native', 'legacy']) {
  for (const control of ['\r', '\n', '\x1b', '\t', '\x00', '\x7f', '\x85']) {
    test('Drop_RejectWholeControlBatch_001 ' + kind + ' ' + control.charCodeAt(0), () => {
      const h = controller(kind)
      assert.equal(h.drop(['/repo/safe.txt', '/repo/draft' + control + 'submit']), false)
      assert.deepEqual(h.writes, [])
    })
  }
  test('Drop_LiteralQuoteAndBackslash_002 ' + kind, () => {
    const h = controller(kind)
    const name = '目录/a"b\\c.txt'
    assert.equal(h.drop(['/repo/' + name]), true)
    assert.deepEqual(h.writes, [JSON.stringify(name)])
  })
  test('Drop_NormalPathsRemainUsable_003 ' + kind, () => {
    const h = controller(kind)
    assert.equal(h.drop(['/repo/src/a.ts', '/outside/my file.txt']), true)
    assert.deepEqual(h.writes, ['src/a.ts "/outside/my file.txt"'])
  })
}

test('Rename_ClosedHistoryMenuRetainsMetadataAction_004', () => {
  const row = { opened: false, archived: false, processState: 'stopped', resumable: true }
  assert.ok(selectSessionMenuActions(row).some(action => action.id === 'rename'))
  for (const blocked of [{ ...row, archived: true }, { ...row, preparationState: 'preparing' }]) {
    assert.ok(!selectSessionMenuActions(blocked).some(action => action.id === 'rename'))
  }
})

test('Rename_ClosedHistoryEditorDoesNotSelectTerminal_005', () => {
  const source = readFileSync('src/stores/unifiedSessions.ts', 'utf8')
  const body = source.match(/  function beginRename\(id: string\) \{([\s\S]*?)\n  \}/)?.[1]
  assert.ok(body)
  const sessions = { value: [{ id: 'history', opened: false, archived: false, renameState: 'idle' }] }
  const selection = { value: 'live-terminal' }
  const owners = new Map()
  const begin = Function('sessions', 'activeSessionId', 'pendingRenames', 'isPreparingSession',
    'renameOwners', 'ownsRename', 'captureRenameOwner', 'return function(id){' + body + '}')(
    sessions, selection, new Map(), () => false, owners, () => true,
    () => ({ controller: new AbortController() }))
  begin('history')
  assert.equal(sessions.value[0].renameState, 'editing')
  assert.equal(selection.value, 'live-terminal')
  assert.ok(owners.has('history'))
})

test('Rename_ClosedHistorySfcAdmitsAndRendersEditor_006', () => {
  const source = readFileSync('src/components/sessions/SessionItem.vue', 'utf8')
  const props = { selected: false, session: { opened: false, archived: false, renameState: 'editing' } }
  for (const name of ['canRename', 'isRenaming']) {
    const expression = source.match(new RegExp('const ' + name + ' = computed\\(\\(\\) => (.+)\\)'))?.[1]
    assert.ok(expression)
    assert.equal(Function('props', 'localRename', 'return ' + expression)(props, { value: false }), true, name)
  }
  const expression = source.match(/const isRenaming = computed\(\(\) => (.+)\)/)?.[1]
  const render = (selected, opened, state, local) => Function('props', 'localRename', 'return ' + expression)(
    { selected, session: { opened, renameState: state } }, { value: local })
  // 后台保存只禁用 Archive；已进入的编辑器和选中运行行保存仍保持编辑器。
  assert.equal(render(false, false, 'saving', false), false)
  assert.equal(render(false, false, 'saving', true), true)
  assert.equal(render(true, true, 'saving', false), true)
})
