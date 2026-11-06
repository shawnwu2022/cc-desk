import { isReleaseChannel } from './updatePolicy'
import { updateFailure } from './updateErrors'
import { isDiagnosticVersion, isVersionProbeReason } from '@/api/cliVersionDiagnostics'

const version = (value: unknown) => typeof value === 'string' && value.length <= 64 && /^\d+\.\d+\.\d+(?:-(?:beta|rc|preview)(?:\.\d+)?)?$/.test(value) ? value : 'unknown'
const states = ['idle', 'checking', 'downloading', 'verifying', 'installing', 'restarting', 'cancelled', 'error']

function safeCliVersion(input: unknown) {
  const row = input && typeof input === 'object' && !Array.isArray(input) ? input as Record<string, unknown> : {}
  const source = typeof row.source === 'string' && /^selected-profile-[1-9]\d{0,5}$/.test(row.source) ? row.source : null
  const state = typeof row.state === 'string' && ['not-collected', 'collecting', 'cancelling', 'known', 'unknown', 'cancelled'].includes(row.state) ? row.state : 'unknown'
  if (state === 'known' && isDiagnosticVersion(row.version)) return { source, state, version: row.version, reason: null }
  return { source, state: state === 'known' ? 'unknown' : state, version: null,
    reason: state === 'unknown' || state === 'known' ? isVersionProbeReason(row.reason) ? row.reason : 'unavailable' : null }
}

export function safeUpdateDiagnostics(input: { channel: unknown; state: unknown; candidate: unknown; failure: unknown; cliVersions?: unknown }) {
  const failure = input.failure ? updateFailure(input.failure) : null
  const versions = input.cliVersions && typeof input.cliVersions === 'object' ? input.cliVersions as Record<string, unknown> : null
  return {
    selectedChannel: isReleaseChannel(input.channel) ? input.channel : 'unknown',
    phase: typeof input.state === 'string' && states.includes(input.state) ? input.state : 'unknown',
    candidateVersion: version(input.candidate),
    failure: failure ? { code: failure.code, stage: failure.stage } : null,
    cliVersions: versions ? { claude: safeCliVersion(versions.claude), codex: safeCliVersion(versions.codex), opencode: safeCliVersion(versions.opencode) }
      : { claude: 'not-collected', codex: 'not-collected', opencode: 'not-collected' },
  }
}
