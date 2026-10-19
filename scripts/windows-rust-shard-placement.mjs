import fs from 'node:fs'
import { pathToFileURL } from 'node:url'
import { RUST_SHARD_COUNT, estimatedWeight, partitionNames, validatePartition } from './windows-rust-shards.mjs'

const TAIL_NAME = 'version_history::windows::context::bundle_restore::tests::BundleReturn_RestorePresentImage_002'
function requireThat(condition, message) {
  if (!condition) throw new Error(`Rust placement comparison: ${message}`)
}
// 只生成候选和启发式负载对比；不接入 producer、执行器或 coverage 准入。
export function compareTailCandidate(observation) {
  requireThat(/^[a-f0-9]{40}$/.test(observation?.sourceSha ?? '') && /^[a-f0-9]{64}$/.test(observation?.planHash ?? '')
    && /^[1-9]\d*$/.test(observation?.runId ?? '') && Number.isSafeInteger(observation?.runAttempt) && observation.runAttempt > 0, 'observation identity required')
  const shards = observation.shards
  requireThat(Array.isArray(shards) && shards.length === RUST_SHARD_COUNT
    && new Set(shards.map(shard => shard.index)).size === RUST_SHARD_COUNT
    && shards.every(shard => Number.isInteger(shard.index) && shard.index >= 0 && shard.index < RUST_SHARD_COUNT && Array.isArray(shard.names)), 'complete baseline required')
  const names = shards.flatMap(shard => shard.names), original = partitionNames(names, RUST_SHARD_COUNT)
  requireThat(shards.every(shard => JSON.stringify(original[shard.index]) === JSON.stringify(shard.names)), 'current algorithm differs from recorded baseline')
  requireThat(names.includes(TAIL_NAME), 'observed tail name missing')
  const candidate = [[TAIL_NAME], ...partitionNames(names.filter(name => name !== TAIL_NAME), RUST_SHARD_COUNT - 1)]
  validatePartition(names, candidate)
  const loads = partitions => partitions.map(partition => partition.reduce((sum, name) => sum + estimatedWeight(name), 0))
  return { candidate, summary: { advisoryOnly: true, sourceSha: observation.sourceSha, runId: observation.runId, runAttempt: observation.runAttempt,
    planHash: observation.planHash, selectedCount: names.length, shardCount: RUST_SHARD_COUNT, isolatedName: TAIL_NAME,
    originalMaxHeuristicLoad: Math.max(...loads(original)), candidateMaxHeuristicLoad: Math.max(...loads(candidate)),
    durationPrediction: null, hostedNetGainVerified: false } }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  requireThat(process.argv.length === 3, 'one observation JSON file required')
  requireThat(fs.statSync(process.argv[2]).size <= 2 * 1024 * 1024, 'observation bound exceeded')
  const { summary } = compareTailCandidate(JSON.parse(fs.readFileSync(process.argv[2], 'utf8')))
  console.log(JSON.stringify(summary, null, 2))
}
