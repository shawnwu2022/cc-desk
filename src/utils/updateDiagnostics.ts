import { isReleaseChannel } from './updatePolicy'
import { updateFailure } from './updateErrors'

const version = (value: unknown) => typeof value === 'string' && value.length <= 64 && /^\d+\.\d+\.\d+(?:-(?:beta|rc|preview)(?:\.\d+)?)?$/.test(value) ? value : 'unknown'
const states = ['idle', 'checking', 'downloading', 'verifying', 'installing', 'restarting', 'cancelled', 'error']

export function safeUpdateDiagnostics(input: { channel: unknown; state: unknown; candidate: unknown; failure: unknown }) {
  const failure = input.failure ? updateFailure(input.failure) : null
  return {
    selectedChannel: isReleaseChannel(input.channel) ? input.channel : 'unknown',
    phase: typeof input.state === 'string' && states.includes(input.state) ? input.state : 'unknown',
    candidateVersion: version(input.candidate),
    failure: failure ? { code: failure.code, stage: failure.stage } : null,
    cliVersions: { claude: 'not-collected', codex: 'not-collected', opencode: 'not-collected' },
  }
}
