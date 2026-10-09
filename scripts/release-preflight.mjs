import { appendFileSync, readFileSync } from 'node:fs'
import { mayPublish, validateArtifacts } from './release-policy.mjs'

const { GITHUB_REPOSITORY: repository, GITHUB_SHA: sha, GITHUB_REF: ref, GITHUB_EVENT_NAME: event, GITHUB_TOKEN: token } = process.env
if (!/^[-\w.]+\/[-\w.]+$/.test(repository ?? '') || !token) throw new Error('workflow repository/token required')
async function api(path, optional = false) {
  const response = await fetch(`https://api.github.com/repos/${repository}/${path}`, {
    headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' },
  })
  if (optional && response.status === 404) return null
  if (!response.ok) throw new Error(`release gate API failed: HTTP ${response.status}`)
  return response.json()
}
async function pages(path, key) {
  const items = []
  for (let page = 1; page <= 100; page++) {
    const result = await api(`${path}${path.includes('?') ? '&' : '?'}per_page=100&page=${page}`)
    const batch = key ? result[key] : result
    if (!Array.isArray(batch)) throw new Error('invalid paginated release gate response')
    items.push(...batch)
    if (batch.length < 100) return items
  }
  throw new Error('release gate pagination exceeded bound')
}
const packageVersion = JSON.parse(readFileSync('package.json', 'utf8')).version
const tauriVersion = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8')).version
const cargoPackage = readFileSync('src-tauri/Cargo.toml', 'utf8').match(/\[package\]([\s\S]*?)(?=\n\[|$)/)?.[1]
const cargoVersion = cargoPackage?.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
const tag = `v${packageVersion}`
const branch = await api('branches/main')
const runs = await pages(`actions/workflows/ci.yml/runs?head_sha=${sha}&branch=main&event=push`, 'workflow_runs')
const ci = runs.sort((a, b) => b.id - a.id)[0]
const jobs = ci ? await pages(`actions/runs/${ci.id}/jobs?filter=latest`, 'jobs') : []
const tagRef = await api(`git/ref/tags/${encodeURIComponent(tag)}`, true)
// List authenticated drafts too: the public tag endpoint can miss an existing draft.
const releases = await pages('releases', null)
const context = { event, ref, sha, main: { sha: branch.commit?.sha, protected: branch.protected },
  versions: [packageVersion, cargoVersion, tauriVersion], tag, tagExists: tagRef !== null,
  releaseExists: releases.some(release => release.tag_name === tag), ci, jobs }
if (!mayPublish(context)) throw new Error('release gate blocked: require current protected main, matching versions, full successful main CI and unused tag/release')
if (process.argv.includes('--artifacts')) {
  const runId = Number(process.env.GITHUB_RUN_ID)
  const run = await api(`actions/runs/${runId}`)
  if (run.head_sha !== sha || run.head_branch !== 'main' || run.path !== '.github/workflows/release.yml') throw new Error('release workflow source binding failed')
  validateArtifacts(await pages(`actions/runs/${runId}/artifacts`, 'artifacts'), sha, runId)
}
if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `version=${packageVersion}\ntag=${tag}\nci_run=${ci.id}\n`)
console.log(`Release preflight passed for ${sha}, CI run ${ci.id}, ${tag}`)
