const SHA256 = /^[0-9a-f]{64}$/

export function mayPublish(context) {
  if (!context || context.event !== 'workflow_dispatch' || context.operation !== 'promote') {
    return false
  }
  if (context.gatePassed !== true
    || context.manifestVerified !== true
    || context.sameCandidate !== true
    || context.rebuilt !== false
    || context.tagMatchesVersion !== true
    || !SHA256.test(String(context.candidateId ?? ''))) {
    return false
  }
  return context.approval === `PROMOTE:${context.candidateId}`
}
