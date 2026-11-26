import { spawnSync } from 'node:child_process'
import { mkdtempSync, realpathSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'

export function withIsolatedPwshCache(options, run, platform = process.platform) {
  if (platform !== 'linux') return run(options)
  // Parallel PowerShell starts can corrupt the shared .NET startup profile.
  // https://github.com/PowerShell/PowerShell/issues/26528
  const parent = realpathSync(tmpdir())
  const cache = mkdtempSync(join(parent, 'ccdesk-pwsh-cache-'))
  try {
    return run({ ...options, env: { ...(options.env ?? process.env), XDG_CACHE_HOME: cache } })
  } finally {
    if (dirname(cache) !== parent) throw new Error('PowerShell cache cleanup escaped its temporary parent')
    rmSync(cache, { recursive: true, force: true })
  }
}

export function spawnTestPwsh(args, options) {
  return withIsolatedPwshCache(options, isolated => spawnSync('pwsh', args, isolated))
}
