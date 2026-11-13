import { beforeAll, expect, test, vi } from 'vitest'
import { createECDH } from 'node:crypto'
import { createApprovedBrowserProfileReader, parseBrowserPairingConfiguration, type BrowserProfileHandles } from '../../remote/ui-terminal/src/browserPairingProfile.ts'

const configuration = {
  version: 1,
  profile: { origin: 'https://desk.invalid', rpId: 'desk.invalid', credentialId: 'Bw' },
  localSpki: Buffer.alloc(91, 1).toString('base64url'), hostSpki: Buffer.alloc(91, 2).toString('base64url'),
  trust: { hostKeyEpoch: '1', clientKeyEpoch: '2', origin: 'wss://relay.invalid/route' },
  grant: {
    issuerSpki: Buffer.alloc(91, 3).toString('base64url'), principal: '04'.repeat(32),
    scope: { session: '05'.repeat(16), run: 'PUBLIC_TEST_ONLY', generation: '7' },
    role: 'Controller', budgetMs: 10000, peerInstall: { budgetMs: 1000 },
    profile: { origin: 'https://desk.invalid', rpId: 'desk.invalid', credentialId: 'Bw', credentialSpki: Buffer.alloc(91, 4).toString('base64url'), backupEligible: false, allowBackedUp: false },
  },
  relay: { mode: 'rendezvous-v1', endpoint: 'wss://relay.invalid/route', tunnel: Buffer.alloc(16, 9).toString('base64url'), keyId: Buffer.alloc(16, 8).toString('base64url') },
  artifacts: { wasm: 'https://desk.invalid/assets/client.wasm', glue: 'https://desk.invalid/assets/client.js' },
}
let tlsKey: CryptoKey, grantKey: CryptoKey, extractableKey: CryptoKey
beforeAll(async () => {
  // 复用现有公开 TEST_ONLY scalar1/2，不生成或登记真实账户密钥。
  const keys: CryptoKey[] = []
  for (const scalar of [1, 2, 1]) {
    const d = Buffer.alloc(32); d[31] = scalar
    const ec = createECDH('prime256v1'); ec.setPrivateKey(d); const point = ec.getPublicKey()
    keys.push(await crypto.subtle.importKey('jwk', { kty: 'EC', crv: 'P-256', d: d.toString('base64url'), x: point.subarray(1, 33).toString('base64url'), y: point.subarray(33).toString('base64url') }, { name: 'ECDSA', namedCurve: 'P-256' }, keys.length === 2, ['sign']))
  }
  ;[tlsKey, grantKey, extractableKey] = keys
})

// 解析仅返回冻结公开值，原始字节变化不能改变快照。
test('Pairing_PublicSnapshot_001', () => {
  const bytes = Buffer.from(JSON.stringify(configuration)), parsed = parseBrowserPairingConfiguration(bytes)
  bytes.fill(0)
  expect(parsed).toEqual(configuration)
  expect(Object.isFrozen(parsed.grant.scope)).toBe(true)
  expect(Object.isFrozen(parsed.grant.profile)).toBe(true)
  expect('tlsKey' in parsed.profile).toBe(false)
})

// 任一层重复字段、未知字段、非法 UTF-8 和深嵌套均被拒绝。
test.each([
  '{"version":1,"version":1}', '{"version":1,"versi\\u006fn":1}', '{"grant":{"scope":{"run":"a","run":"b"}}}',
  JSON.stringify({ ...configuration, privateKey: 'PUBLIC_TEST_ONLY' }),
  JSON.stringify({ ...configuration, profile: { ...configuration.profile, tlsKey: {} } }),
  JSON.stringify({ ...configuration, relay: { ...configuration.relay, sign: 'function' } }),
  JSON.stringify({ ...configuration, grant: { ...configuration.grant, approved: true } }),
  JSON.stringify({ ...configuration, version: 2 }), JSON.stringify(configuration) + ' trailing',
  '['.repeat(17) + '0' + ']'.repeat(17), ' '.repeat(16385),
])('Pairing_RejectJson_002_%#', text => {
  expect(() => parseBrowserPairingConfiguration(Buffer.from(text))).toThrow('browser pairing configuration')
})

// 编码、身份、URL 和预算边界失败时不回显输入。
test.each([
  ['profile.credentialId', 'Bw=='], ['profile.credentialId', 'Bx'], ['profile.credentialId', ''],
  ['localSpki', 'AA'], ['hostSpki', 'A'.repeat(121)], ['grant.issuerSpki', 'A'.repeat(121)],
  ['relay.tunnel', Buffer.alloc(16).toString('base64url')], ['relay.keyId', 'AA'],
  ['profile.origin', 'http://desk.invalid'], ['profile.rpId', 'other.invalid'],
  ['trust.hostKeyEpoch', '0'], ['trust.clientKeyEpoch', '18446744073709551616'],
  ['grant.scope.generation', '07'], ['grant.scope.run', ''], ['grant.scope.session', 'AB'.repeat(16)],
  ['grant.principal', '0'], ['grant.role', 'Admin'], ['grant.budgetMs', 30001], ['grant.peerInstall.budgetMs', 0],
  ['grant.profile.credentialId', 'CA'], ['grant.profile.backupEligible', 'false'], ['relay.mode', 'automatic'],
  ['relay.endpoint', 'wss://user:pass@relay.invalid/route'], ['relay.endpoint', 'wss://relay.invalid/route?token=PUBLIC_TEST_ONLY'],
  ['artifacts.glue', 'https://other.invalid/client.js'],
])('Pairing_RejectValue_003_%#', (path, value) => {
  const changed = structuredClone(configuration) as any, names = (path as string).split('.'); let owner = changed
  for (const name of names.slice(0, -1)) owner = owner[name]
  owner[names[names.length - 1]] = value
  expect(() => parseBrowserPairingConfiguration(Buffer.from(JSON.stringify(changed)))).toThrow('browser pairing configuration')
})

// 非法 UTF-8 不是可接受的替换字符。
test('Pairing_RejectUtf8_004', () => {
  expect(() => parseBrowserPairingConfiguration(new Uint8Array([0xff]))).toThrow('browser pairing configuration')
})

// 成功输出保留可信 provider 的原 handles，不签名、不展示、不重读。
test('Pairing_TrustedHandles_005', async () => {
  const source = Buffer.from(JSON.stringify(configuration)), trusted = Buffer.from(source)
  const relaySign = vi.fn(async () => new Uint8Array(64)), present = vi.fn(async () => {})
  const readApproved = vi.fn(async () => ({ configuration: trusted, handles: { tlsKey, grantKey, relaySign, present } }))
  const reader = createApprovedBrowserProfileReader(source, { readApproved }); source.fill(0)
  const signal = new AbortController().signal, inputs = await reader(signal)
  expect(inputs?.profile.tlsKey).toBe(tlsKey); expect(inputs?.profile.grantKey).toBe(grantKey)
  expect(inputs?.relay.identity.sign).toBe(relaySign); expect(inputs?.present).toBe(present)
  expect(inputs?.grant.scope).toEqual(configuration.grant.scope)
  expect(readApproved).toHaveBeenCalledExactlyOnceWith(signal)
  expect(relaySign).not.toHaveBeenCalled(); expect(present).not.toHaveBeenCalled()
  trusted.fill(0); inputs!.hostSpki.fill(0)
  expect((await reader(new AbortController().signal))?.hostSpki[0]).toBe(2)
  expect(readApproved).toHaveBeenCalledTimes(1)
})

// 全字段绑定不能退化为仅检查 scope 或 pin。
test.each([
  ['localSpki', Buffer.alloc(91, 8).toString('base64url')], ['hostSpki', Buffer.alloc(91, 8).toString('base64url')],
  ['trust.hostKeyEpoch', '3'], ['trust.clientKeyEpoch', '3'], ['grant.issuerSpki', Buffer.alloc(91, 8).toString('base64url')],
  ['grant.principal', '06'.repeat(32)], ['grant.scope.session', '06'.repeat(16)], ['grant.scope.run', 'OTHER_TEST_ONLY'],
  ['grant.scope.generation', '8'], ['grant.role', 'Observer'], ['grant.budgetMs', 9999], ['grant.peerInstall.budgetMs', 999],
  ['grant.profile.credentialSpki', Buffer.alloc(91, 8).toString('base64url')], ['grant.profile.backupEligible', true], ['grant.profile.allowBackedUp', true],
  ['relay.mode', 'legacy'], ['relay.tunnel', Buffer.alloc(16, 7).toString('base64url')], ['relay.keyId', Buffer.alloc(16, 7).toString('base64url')],
  ['artifacts.wasm', 'https://desk.invalid/assets/other.wasm'], ['artifacts.glue', 'https://desk.invalid/assets/other.js'],
])('Pairing_RejectMismatch_006_%#', async (path, value) => {
  const changed = structuredClone(configuration) as any, names = (path as string).split('.'); let owner = changed
  for (const name of names.slice(0, -1)) owner = owner[name]
  owner[names[names.length - 1]] = value
  const relaySign = vi.fn(async () => new Uint8Array(64)), present = vi.fn(async () => {})
  const reader = createApprovedBrowserProfileReader(Buffer.from(JSON.stringify(changed)), { readApproved: async () => ({ configuration: Buffer.from(JSON.stringify(configuration)), handles: { tlsKey, grantKey, relaySign, present } }) })
  await expect(reader(new AbortController().signal)).rejects.toThrow('browser pairing profile unavailable')
  expect(relaySign).not.toHaveBeenCalled(); expect(present).not.toHaveBeenCalled()
})

// 缺少 provider 或批准记录时保持锁定，空结果同样只能读一次。
test('Pairing_MissingProvider_007', async () => {
  const bytes = Buffer.from(JSON.stringify(configuration))
  await expect(createApprovedBrowserProfileReader(bytes)(new AbortController().signal)).resolves.toBeUndefined()
  const readApproved = vi.fn(async () => undefined), reader = createApprovedBrowserProfileReader(bytes, { readApproved })
  await expect(reader(new AbortController().signal)).resolves.toBeUndefined()
  await expect(reader(new AbortController().signal)).resolves.toBeUndefined()
  expect(readApproved).toHaveBeenCalledTimes(1)
})

// 首次读取取消后保留原 pending，不能借新 signal 读取或发布迟到成功。
test('Pairing_CancelPending_008', async () => {
  const bytes = Buffer.from(JSON.stringify(configuration)), controller = new AbortController()
  const handles: BrowserProfileHandles = { tlsKey, grantKey, relaySign: async () => new Uint8Array(64), present: async () => {} }
  let release!: (value: { configuration: Uint8Array; handles: BrowserProfileHandles }) => void
  const readApproved = vi.fn(() => new Promise<{ configuration: Uint8Array; handles: BrowserProfileHandles }>(resolve => { release = resolve }))
  const reader = createApprovedBrowserProfileReader(bytes, { readApproved })
  const first = reader(controller.signal); await Promise.resolve()
  const second = reader(new AbortController().signal)
  let settled = 0
  void first.then(() => { settled++ }, () => { settled++ })
  void second.then(() => { settled++ }, () => { settled++ })
  controller.abort(); await Promise.resolve(); await Promise.resolve()
  expect(settled).toBe(0); expect(readApproved).toHaveBeenCalledTimes(1)
  release({ configuration: bytes, handles })
  await expect(first).rejects.toThrow('browser pairing profile unavailable')
  await expect(second).rejects.toThrow('browser pairing profile unavailable')
  await expect(reader(new AbortController().signal)).rejects.toThrow('browser pairing profile unavailable')
  expect(readApproved).toHaveBeenCalledTimes(1)
})

// 原有句柄验证不能被公开 JSON 的解析成功绕过。
test.each(['extractable', 'same', 'missing'] as const)('Pairing_RejectHandles_009_%s', async kind => {
  const bytes = Buffer.from(JSON.stringify(configuration))
  const handles = { tlsKey: kind === 'extractable' ? extractableKey : tlsKey, grantKey: kind === 'same' ? tlsKey : grantKey, relaySign: kind === 'missing' ? undefined : async () => new Uint8Array(64), present: async () => {} }
  const reader = createApprovedBrowserProfileReader(bytes, { readApproved: async () => ({ configuration: bytes, handles: handles as BrowserProfileHandles }) })
  await expect(reader(new AbortController().signal)).rejects.toThrow('browser pairing profile unavailable')
})

// 错误固定且不重读 provider，不泄漏其内部异常载荷。
test('Pairing_FailureRetained_010', async () => {
  const readApproved = vi.fn(async () => { throw Error('PRIVATE_PROVIDER_DETAIL') })
  const reader = createApprovedBrowserProfileReader(Buffer.from(JSON.stringify(configuration)), { readApproved })
  await expect(reader(new AbortController().signal)).rejects.toThrow(/^browser pairing profile unavailable$/)
  await expect(reader(new AbortController().signal)).rejects.toThrow(/^browser pairing profile unavailable$/)
  expect(readApproved).toHaveBeenCalledTimes(1)
})

// 合法成组改动也必须与独立可信快照逐字段匹配，而不是只通过 schema。
test.each(['origin', 'credential', 'relay'] as const)('Pairing_GroupMismatch_011_%s', async kind => {
  const changed = structuredClone(configuration)
  if (kind === 'origin') {
    changed.profile.origin = changed.grant.profile.origin = 'https://other.invalid'
    changed.profile.rpId = changed.grant.profile.rpId = 'other.invalid'
    changed.artifacts = { wasm: 'https://other.invalid/client.wasm', glue: 'https://other.invalid/client.js' }
  } else if (kind === 'credential') changed.profile.credentialId = changed.grant.profile.credentialId = 'CA'
  else changed.relay.endpoint = changed.trust.origin = 'wss://other.invalid/route'
  const candidate = Buffer.from(JSON.stringify(changed))
  expect(parseBrowserPairingConfiguration(candidate)).toEqual(changed)
  const reader = createApprovedBrowserProfileReader(candidate, { readApproved: async () => ({ configuration: Buffer.from(JSON.stringify(configuration)), handles: { tlsKey, grantKey, relaySign: async () => new Uint8Array(64), present: async () => {} } }) })
  await expect(reader(new AbortController().signal)).rejects.toThrow('browser pairing profile unavailable')
})

// 省略 relay mode 不能与显式 legacy 混同，避免隐式协议降级。
test('Pairing_AbsentMode_012', async () => {
  const candidate = structuredClone(configuration) as any; delete candidate.relay.mode
  const expected = structuredClone(configuration); expected.relay.mode = 'legacy'
  const reader = createApprovedBrowserProfileReader(Buffer.from(JSON.stringify(candidate)), { readApproved: async () => ({ configuration: Buffer.from(JSON.stringify(expected)), handles: { tlsKey, grantKey, relaySign: async () => new Uint8Array(64), present: async () => {} } }) })
  await expect(reader(new AbortController().signal)).rejects.toThrow('browser pairing profile unavailable')
})

// 字段顺序与空白不属于语义，键值完全相同才能读取原批准。
test('Pairing_FieldOrder_013', async () => {
  const candidate = { ...Object.fromEntries(Object.entries(configuration).reverse()), profile: Object.fromEntries(Object.entries(configuration.profile).reverse()) }
  const reader = createApprovedBrowserProfileReader(Buffer.from(JSON.stringify(candidate, null, 2)), { readApproved: async () => ({ configuration: Buffer.from(JSON.stringify(configuration)), handles: { tlsKey, grantKey, relaySign: async () => new Uint8Array(64), present: async () => {} } }) })
  expect((await reader(new AbortController().signal))?.grant.scope.run).toBe('PUBLIC_TEST_ONLY')
})

// 预先取消的首读不调用 provider，也不能换 signal 重启。
test('Pairing_PreAborted_014', async () => {
  const controller = new AbortController(); controller.abort()
  const readApproved = vi.fn(async () => undefined)
  const reader = createApprovedBrowserProfileReader(Buffer.from(JSON.stringify(configuration)), { readApproved })
  await expect(reader(controller.signal)).rejects.toThrow('browser pairing profile unavailable')
  await expect(reader(new AbortController().signal)).rejects.toThrow('browser pairing profile unavailable')
  expect(readApproved).not.toHaveBeenCalled()
})

// 缓存成功不能让后来已取消的调用获得批准 inputs。
test('Pairing_AbortedCachedRead_015', async () => {
  const readApproved = vi.fn(async () => ({ configuration: Buffer.from(JSON.stringify(configuration)), handles: { tlsKey, grantKey, relaySign: async () => new Uint8Array(64), present: async () => {} } }))
  const reader = createApprovedBrowserProfileReader(Buffer.from(JSON.stringify(configuration)), { readApproved })
  expect(await reader(new AbortController().signal)).toBeDefined()
  const controller = new AbortController(); controller.abort()
  await expect(reader(controller.signal)).rejects.toThrow('browser pairing profile unavailable')
  expect(readApproved).toHaveBeenCalledTimes(1)
})

// 完整有效文档中的重复字段仍拒绝，不能靠其余必填字段缺失让测试通过。
test.each(['"version":1,', '"versi\\u006fn":1,'])('Pairing_ValidDuplicate_016_%#', field => {
  const duplicate = '{' + field + JSON.stringify(configuration).slice(1)
  expect(() => parseBrowserPairingConfiguration(Buffer.from(duplicate))).toThrow('browser pairing configuration')
})

// provider 只能交回受控数据属性，getter 不得在绑定过程中换值。
test('Pairing_RejectAccessor_017', async () => {
  const bytes = Buffer.from(JSON.stringify(configuration)), getter = vi.fn(() => bytes)
  const supplied = { handles: { tlsKey, grantKey, relaySign: async () => new Uint8Array(64), present: async () => {} } } as any
  Object.defineProperty(supplied, 'configuration', { enumerable: true, get: getter })
  const reader = createApprovedBrowserProfileReader(bytes, { readApproved: async () => supplied })
  await expect(reader(new AbortController().signal)).rejects.toThrow('browser pairing profile unavailable')
  expect(getter).not.toHaveBeenCalled()
})

// 完整有效 scope 中同值 run 重复仍拒绝，不依赖其他缺失字段。
test('Pairing_ScopeDuplicate_018', () => {
  const text = JSON.stringify(configuration).replace('"run":"PUBLIC_TEST_ONLY"', '"run":"PUBLIC_TEST_ONLY","run":"PUBLIC_TEST_ONLY"')
  expect(() => parseBrowserPairingConfiguration(Buffer.from(text))).toThrow('browser pairing configuration')
})
