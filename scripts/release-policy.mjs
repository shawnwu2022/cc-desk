export function mayPublish(context) {
  const c = context
  if (!c || !['push', 'workflow_dispatch'].includes(c.event) || c.ref !== 'refs/heads/main') return false
  if (!/^[a-f0-9]{40}$/.test(c.sha ?? '') || c.main?.protected !== true || c.main.sha !== c.sha) return false
  if (!Array.isArray(c.versions) || c.versions.length !== 3 || !c.versions.every(v => v === c.versions[0])) return false
  if (!/^\d+\.\d+\.\d+$/.test(c.versions[0]) || c.tag !== `v${c.versions[0]}`) return false
  if (c.tagExists !== false || c.releaseExists !== false) return false
  const ci = c.ci
  if (!ci || ci.path !== '.github/workflows/ci.yml' || ci.head_sha !== c.sha || ci.head_branch !== 'main' || ci.event !== 'push') return false
  if (ci.status !== 'completed' || ci.conclusion !== 'success') return false
  if (!Array.isArray(c.jobs) || !c.jobs.length || !c.jobs.every(j => j.status === 'completed' && j.conclusion === 'success')) return false
  return ['Frontend checks', 'Rust checks'].every(name => c.jobs.filter(j => j.name === name).length === 1)
}

export function validateArtifacts(artifacts, sha, runId) {
  const names = ['windows', 'macos', 'linux'].map(platform => `cc-desk-candidate-${sha}-${platform}`)
  if (!Array.isArray(artifacts) || artifacts.length !== 3) throw new Error('expected exactly three platform artifacts')
  for (const name of names) {
    const matches = artifacts.filter(a => a.name === name)
    if (matches.length !== 1) throw new Error(`missing or duplicate artifact: ${name}`)
    const artifact = matches[0]
    if (artifact.expired !== false || artifact.workflow_run?.id !== runId || artifact.workflow_run?.head_sha !== sha || artifact.workflow_run?.head_branch !== 'main') {
      throw new Error(`artifact source/run binding failed: ${name}`)
    }
  }
}
