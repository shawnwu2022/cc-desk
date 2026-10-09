import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createHash } from 'node:crypto'

const fixtureRoot = new URL('../fixtures/version-history-payload/', import.meta.url)
const read = name => readFileSync(new URL(name, fixtureRoot))
const digest = bytes => createHash('sha256').update(bytes).digest('hex')
const measurements = JSON.parse(read('reviewed-measurements.json'))
const catalog = JSON.parse(read('catalog.json'))
const versions = ['0.14.0', '0.15.0', '0.16.0', '0.17.0', '0.17.1', '0.17.2', '0.17.5', '0.17.6', '0.17.7']

test('reviewed payload policy checks run in ordinary CI and before native evidence collection', () => {
  const ciSource = readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8')
  const evidenceSource = readFileSync(new URL('../../.github/workflows/history-payload-evidence.yml', import.meta.url), 'utf8')
  for (const ending of ['\n', '\r\n']) {
    const ci = ciSource.replace(/\r?\n/g, ending)
    const evidence = evidenceSource.replace(/\r?\n/g, ending)
    assert.match(ci, /name: Run Node policy tests\s+run: [^\n]*tests\/scripts\/historyPayloadPolicy\.node\.mjs/,
      'ordinary CI must execute reviewed payload policy checks')
    assert.match(evidence, /name: Verify pinned fixture provenance\s+run: [^\n]*tests\/scripts\/historyPayloadPolicy\.node\.mjs/)
    assert.match(evidence, /^      - tests\/scripts\/historyPayloadPolicy\.node\.mjs\r?$/m)
    assert.match(evidence, /^      - src-tauri\/src\/version_history\/payload_policy\.rs\r?$/m)
    assert.match(evidence, /^      - src-tauri\/src\/tests\/version_history_payload_policy\.rs\r?$/m)
    const policyCommand = 'cargo test --locked --lib version_history::payload_policy::tests:: -- --test-threads=1'
    assert.ok(evidence.includes(policyCommand), 'native policy contracts must run before installer observation')
    assert.ok(evidence.indexOf(policyCommand) < evidence.indexOf('Run isolated capability gate and payload capture'))
  }
})

// 检查九个静态条目逐字段匹配已审查证据；此源码一致性检查不执行 Rust 准入逻辑。
test('reviewed static policy contains exactly the nine measured installer and inventory tuples', () => {
  const source = readFileSync(new URL('../../src-tauri/src/version_history/payload_policy.rs', import.meta.url), 'utf8')
  const table = source.split('const REVIEWED: &[MeasuredPayload] = ')[1].split('// Enabled only')[0]
  const actual = [...table.matchAll(/MeasuredPayload \{\s*version: "([^"]+)",\s*installer_digest: "([a-f0-9]+)",\s*installer_size: ([\d_]+),\s*installed_inventory: &\[([\s\S]*?)\],\s*installed_bytes: ([\d_]+),\s*\}/g)].map(match => ({
    version: match[1],
    installer_digest: match[2],
    installer_size: Number(match[3].replaceAll('_', '')),
    installed_inventory: [...match[4].matchAll(/MeasuredFile \{\s*path: "([^"]+)",\s*size: ([\d_]+),\s*sha256: "([a-f0-9]+)",\s*attributes: (\d+),\s*\}/g)].map(file => ({
      path: file[1], size: Number(file[2].replaceAll('_', '')), sha256: file[3], attributes: Number(file[4]),
    })),
    installed_bytes: Number(match[5].replaceAll('_', '')),
  })).sort((a, b) => a.version.localeCompare(b.version))
  const expected = measurements.payloads.map(({ inventory_digest, provenance, ...payload }) => payload)
  assert.deepEqual(actual.map(payload => payload.version), versions, 'missing, duplicate or unreviewed policy version')
  assert.deepEqual(actual, expected, 'static policy must match the independently reviewed public measurements exactly')
})

// 检查全部已批准输出与官方选择绑定，来源附属文件不能加入目标字节或清单摘要。
test('measurement inventories bind the exact official package and exclude source-only companions', () => {
  assert.equal(measurements.schema, 1)
  assert.deepEqual(measurements.payloads.map(payload => payload.version), versions)
  for (const payload of measurements.payloads) {
    const pinned = catalog.fixtures.find(fixture => fixture.version === payload.version)
    const installer = pinned.selection.assets[0]
    assert.equal(payload.installer_size, installer.size)
    assert.equal(`sha256:${payload.installer_digest}`, installer.digest)
    const expectedPaths = payload.version === '0.17.7'
      ? ['LICENSE-Microsoft-ConPTY.txt', 'OpenConsole.exe', 'cc-desk.exe', 'conpty.dll', 'uninstall.exe']
      : ['cc-desk.exe', 'uninstall.exe']
    assert.deepEqual(payload.installed_inventory.map(file => file.path), expectedPaths)
    for (const file of payload.installed_inventory) {
      assert.deepEqual(Object.keys(file), ['path', 'size', 'sha256', 'attributes'])
      assert.ok(Number.isSafeInteger(file.size) && file.size > 0)
      assert.match(file.sha256, /^[0-9a-f]{64}$/)
      assert.equal(file.attributes, 32)
    }
    assert.equal(payload.installed_bytes, payload.installed_inventory.reduce((sum, file) => sum + file.size, 0))
    assert.equal(payload.inventory_digest, digest(JSON.stringify([
      'cc-desk-measured-payload-v1', payload.version, payload.installer_digest,
      payload.installer_size, payload.installed_inventory,
    ])))
    assert.equal(payload.provenance.releaseSourceCommit, pinned.provenance.sourceCommit)
    assert.equal(payload.provenance.releaseSourceURL, `https://github.com/shawnwu2022/cc-desk/tree/${pinned.provenance.sourceCommit}`)
  }
})

// 检查新矩阵中的 0.17.7 与原始已批准包及五个输出保持字节级等价。
test('v0.17.7 measurement preserves the previously reviewed payload bytes and inventory digest', () => {
  const previous = JSON.parse(read('v0.17.7-measured.json'))
  const current = measurements.payloads.find(payload => payload.version === '0.17.7')
  assert.equal(current.installer_digest, previous.installerSha256)
  assert.equal(current.installer_size, previous.installerSize)
  assert.deepEqual(current.installed_inventory, previous.installed
    .filter(entry => entry.metadata.kind === 'File' && entry.metadata.path !== 'source-only-leftover.txt')
    .map(entry => ({ path: entry.metadata.path, size: entry.metadata.size, sha256: entry.sha256, attributes: entry.metadata.permissions.Windows.attributes })))
  assert.equal(current.installed_bytes, 18599575)
})

// 保留三份限流失败和后续精确重试来源，禁止把 18 个成功产物说成 21 次成功。
test('public provenance retains all successful pairs and original rate-limit failures without VM identifiers', () => {
  const evidence = measurements.evidence
  const sourceCommit = 'b708427aaae52c81d6d8c821ca08c097e4258108'
  const runURL = 'https://github.com/shawnwu2022/cc-desk/actions/runs/37199938820'
  assert.equal(evidence.fixtureSourceCommit, sourceCommit)
  assert.equal(evidence.fixtureSourceURL, `https://github.com/shawnwu2022/cc-desk/tree/${sourceCommit}`)
  assert.deepEqual(evidence.attempts, [1, 2].map(attempt => ({ attempt, url: `${runURL}/attempts/${attempt}` })))
  assert.deepEqual(evidence.inputSha256, {
    'measured-summary.json': '5624e91c9a3a9c36dc5a96dce980302ecee4faf437940bdbe397315dc6950676',
    'static-payload-fields-for-review.json': 'bd0fcee28c7f34f681d8fc19029147cf62a57887c23f8e8709201c75b0601cbf',
  })
  assert.equal(evidence.fixtureCatalogSha256, digest(read('catalog.json')))
  assert.equal(evidence.releaseCatalogSha256, catalog.releaseCatalogSha256)
  assert.equal(evidence.completePairs, 9)
  assert.equal(evidence.allObservationCheckCount, 1847)
  assert.equal(evidence.checkFailures, 0)
  const observations = measurements.payloads.flatMap(payload => {
    assert.match(payload.provenance.fixtureFingerprint, /^[0-9a-f]{64}$/)
    assert.deepEqual(Object.keys(payload.provenance.cases), ['clean', 'seeded-existing'])
    return Object.entries(payload.provenance.cases).map(([name, observation]) => {
      assert.equal(observation.runId, evidence.runId)
      assert.equal(observation.fixtureSourceCommit, sourceCommit)
      assert.equal(observation.compiledSourceCommit, sourceCommit)
      assert.equal(observation.jobURL, `${runURL}/job/${observation.jobId}`)
      assert.equal(observation.artifactURL, `https://api.github.com/repos/shawnwu2022/cc-desk/actions/artifacts/${observation.artifactId}`)
      assert.match(observation.artifactSha256, /^[0-9a-f]{64}$/)
      assert.equal(observation.checksPassed, observation.checkCount)
      assert.equal(observation.fileCount, payload.installed_inventory.length + (name === 'clean' ? 0 : 1))
      assert.equal(observation.entryCount, observation.fileCount + 1)
      assert.equal(observation.payloadTotalBytes, payload.installed_bytes + (name === 'clean' ? 0 : 56))
      return observation
    })
  })
  assert.equal(observations.length, evidence.actualSuccessArtifactCount)
  assert.equal(observations.filter(record => record.runAttempt === 1).length, 15)
  assert.equal(observations.filter(record => record.runAttempt === 2).length, 3)
  assert.equal(evidence.attempt1SuccessArtifacts, 15)
  assert.equal(evidence.attempt2SuccessArtifacts, 3)
  assert.equal(evidence.priorBlockedEvidence.length, evidence.retainedBlockedArtifactCount)
  assert.equal(evidence.retainedBlockedArtifactCount, 3)
  assert.equal(observations.length + evidence.priorBlockedEvidence.length, evidence.totalActualArtifactsReviewed)
  assert.equal(evidence.totalActualArtifactsReviewed, 21)
  assert.equal(new Set([...observations, ...evidence.priorBlockedEvidence].map(record => record.artifactId)).size, 21)
  for (const failure of evidence.priorBlockedEvidence) {
    assert.equal(failure.runAttempt, 1)
    assert.equal(failure.error, 'HISTORY_RATE_LIMITED')
    assert.equal(failure.installerStarted, false)
    assert.match(failure.artifactSha256, /^[0-9a-f]{64}$/)
    assert.equal(failure.jobURL, `${runURL}/job/${failure.jobId}`)
    const retry = measurements.payloads.find(payload => payload.version === failure.version).provenance.cases[failure.case]
    assert.equal(retry.runAttempt, 2)
    assert.notEqual(retry.artifactId, failure.artifactId)
  }
  assert.doesNotMatch(read('reviewed-measurements.json').toString(), /S-1-5-|[A-Za-z]:\\|\/workspace\/|object_identity|security_descriptor/)
})
