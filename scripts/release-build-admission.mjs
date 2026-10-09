import { execFileSync } from 'node:child_process'
import { appendFileSync, readFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

// Candidate build admission only. This never establishes CI coverage, verifies
// release assets, changes a tag/draft, or grants publication authority.
export function assertBuildAdmission(context) {
  const c = context
  if (!c || !['push', 'workflow_dispatch'].includes(c.event) || c.ref !== 'refs/heads/main'
    || !/^[a-f0-9]{40}$/.test(c.sha ?? '') || c.checkoutSha !== c.sha
    || c.main?.protected !== true || c.main.sha !== c.sha) {
    throw new Error('build admission blocked: exact checkout of current protected main required')
  }
  if (!Array.isArray(c.versions) || c.versions.length !== 3
    || !c.versions.every(version => typeof version === 'string' && version === c.versions[0] && version === version.trim())
    || !/^\d+\.\d+\.\d+$/.test(c.versions[0] ?? '')) {
    throw new Error('build admission blocked: matching package/Cargo/Tauri versions required')
  }
}

export async function runBuildAdmission() {
  const { GITHUB_REPOSITORY: repository, GITHUB_SHA: sha, GITHUB_REF: ref,
    GITHUB_EVENT_NAME: event, GITHUB_TOKEN: token } = process.env
  if (!/^[-\w.]+\/[-\w.]+$/.test(repository ?? '') || !token) {
    throw new Error('build admission blocked: workflow repository/token required')
  }
  if (process.argv.length !== 2) throw new Error('build admission blocked: arguments are not supported')
  const checkoutSha = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()
  const packageVersion = JSON.parse(readFileSync('package.json', 'utf8')).version
  const tauriVersion = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8')).version
  const cargoPackage = readFileSync('src-tauri/Cargo.toml', 'utf8').match(/\[package\]([\s\S]*?)(?=\n\[|$)/)?.[1]
  const cargoVersion = cargoPackage?.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
  const response = await fetch(`https://api.github.com/repos/${repository}/branches/main`, {
    headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' },
    signal: AbortSignal.timeout(30000),
  })
  if (!response.ok) throw new Error(`build admission API failed: HTTP ${response.status}`)
  const branch = await response.json()
  assertBuildAdmission({ event, ref, sha, checkoutSha,
    main: { sha: branch.commit?.sha, protected: branch.protected }, versions: [packageVersion, cargoVersion, tauriVersion] })
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `version=${packageVersion}\ntag=v${packageVersion}\n`)
  console.log(`Build source admitted for ${sha}; publication still requires exact CI coverage and signatures`)
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await runBuildAdmission()
