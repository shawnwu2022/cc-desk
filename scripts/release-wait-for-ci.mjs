import { setTimeout as pause } from 'node:timers/promises'
import { pathToFileURL } from 'node:url'

export function ciWaitState({ sha, main, runs }) {
  if (!/^[a-f0-9]{40}$/.test(sha ?? '') || main?.sha !== sha || main.protected !== true) throw new Error('Release wait: protected main changed')
  if (!Array.isArray(runs)) throw new Error('Release wait: invalid CI response')
  const ci = runs.filter(run => run.head_sha === sha && run.head_branch === 'main' && run.event === 'push'
    && run.path === '.github/workflows/ci.yml').sort((a, b) => b.id - a.id)[0]
  if (!ci || ci.status !== 'completed') return 'wait'
  if (ci.conclusion !== 'success') throw new Error(`Release wait: current-source CI ${ci.id} failed (${ci.conclusion})`)
  return 'ready'
}
export async function waitForCurrentCi({ read, pause: wait = pause, now = Date.now, started = now(), timeout = 120 * 60 * 1000 }) {
  for (;;) {
    if (now() - started >= timeout) throw new Error('Release wait: same-source CI timed out')
    if (ciWaitState(await read()) === 'ready') return
    await wait(15000)
  }
}
async function main() {
  const { GITHUB_REPOSITORY: repository, GITHUB_TOKEN: token, GITHUB_SHA: sha } = process.env
  if (repository !== 'shawnwu2022/cc-desk' || !token || process.env.GITHUB_REF !== 'refs/heads/main') throw new Error('Release wait: existing main workflow authentication required')
  async function api(path) {
    const response = await fetch(`https://api.github.com/repos/${repository}/${path}`, {
      redirect: 'error', signal: AbortSignal.timeout(30000),
      headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' },
    })
    if (!response.ok) throw new Error(`Release wait: API HTTP ${response.status}`)
    return response.json()
  }
  console.log(`Wait for ordinary push CI on exact main source ${sha}; failures remain blocking`)
  await waitForCurrentCi({ read: async () => {
    const branch = await api('branches/main')
    const response = await api(`actions/workflows/ci.yml/runs?head_sha=${sha}&branch=main&event=push&per_page=100`)
    return { sha, main: { sha: branch.commit?.sha, protected: branch.protected }, runs: response.workflow_runs }
  } })
  console.log(`Ordinary current-main CI succeeded for ${sha}; full publication preflight follows`)
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(error => { console.error(error.message); process.exitCode = 1 })
}
