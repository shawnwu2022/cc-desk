export function mayPublish(context) {
  return Boolean(
    context
    && context.event === 'workflow_dispatch'
    && context.operation === 'promote'
    && context.gatePassed === true
    && context.manifestVerified === true
    && context.sameCandidate === true
    && context.explicitApproval === true
  )
}
