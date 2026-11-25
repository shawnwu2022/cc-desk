import type { InjectionKey } from 'vue'

export type InviteMinutes = 1 | 5 | 10
export interface PairingIdentity {
  readonly phoneId: string
  readonly phoneRelayPublicKeySec1: string
  /** Endpoint identity, separate from the Relay admission signer. */
  readonly phoneEndpointSpki: string
  readonly endpointBinding: string
}
export interface PendingPairing extends PairingIdentity {
  readonly inviteId: string
  readonly requestId: string
}
export interface ApprovedDevice extends PairingIdentity {
  readonly tunnel: string
}
export interface PairingSnapshot {
  readonly invite: {
    readonly inviteId: string
    /** Available only when the native bridge retained the exact creation receipt. */
    readonly code?: string
    readonly expiresUnixMs: number
    /** Locally generated QR matrix without quiet-zone padding. */
    readonly qrRows?: readonly string[]
  } | null
  readonly pending: readonly PendingPairing[]
  readonly devices: readonly ApprovedDevice[]
}

/** Only an authenticated native Host bridge may provide this handle. All
 * mutations recheck its authority, current membership and the exact target;
 * approve/revoke additionally require native local confirmation. */
export interface VerifiedPairing {
  readonly endpoint: string
  current(): Promise<PairingSnapshot>
  create(minutes: InviteMinutes): Promise<PairingSnapshot>
  cancel(inviteId: string): Promise<PairingSnapshot>
  decide(request: PendingPairing, approve: boolean): Promise<PairingSnapshot>
  revoke(device: ApprovedDevice): Promise<PairingSnapshot>
}
export interface DesktopPairingPort {
  verify(endpoint: string): Promise<VerifiedPairing>
}
export const DESKTOP_PAIRING_PORT: InjectionKey<DesktopPairingPort> = Symbol('verified native pairing')
