export function promotionReady(context) {
  return Boolean(
    context
    && context.event === 'workflow_dispatch'
    && context.operation === 'promote'
    && context.gatePassed === true
    && context.manifestVerified === true
    && context.sameCandidate === true
    && context.rebuildPerformed === false
  )
}

export function mayPublish(context) {
  return Boolean(
    promotionReady(context)
    && context.explicitApproval === true
  )
}

export function promotionComplete(context) {
  return Boolean(
    mayPublish(context)
    && context.publishedBytesVerified === true
    && context.updaterVerified === true
  )
}
