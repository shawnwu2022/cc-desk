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
  /** Only the original live owner may report an active share. */
  readonly sharing: 'none' | 'active'
}
export interface PairingScopeChoice {
  /** Opaque and current only; native owner revalidates the exact original run. */
  readonly id: string
  readonly label: string
}
export interface PairingSnapshot {
  readonly invite: {
    readonly inviteId: string
    /** Available only when the native bridge retained the exact creation receipt. */
    readonly manualCode?: string
    readonly expiresUnixMs: number
    /** Locally generated QR matrix without quiet-zone padding. */
    readonly qrRows?: readonly string[]
  } | null
  readonly pending: readonly PendingPairing[]
  readonly scopeChoices: readonly PairingScopeChoice[]
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
  approve(request: PendingPairing, scopeChoiceId: string | null): Promise<PairingSnapshot>
  reject(request: PendingPairing): Promise<PairingSnapshot>
  revoke(device: ApprovedDevice): Promise<PairingSnapshot>
}
export type SetupResult =
  | { readonly kind: 'verified'; readonly handle: VerifiedPairing }
  | { readonly kind: 'profileRequired' | 'setupVerified' | 'declined' }
  | { readonly kind: 'relayAuthorizationRequired'; readonly approvePublicRegistration: () => Promise<string | null> }

export interface DesktopPairingPort {
  verify(endpoint: string): Promise<SetupResult>
}
export const DESKTOP_PAIRING_PORT: InjectionKey<DesktopPairingPort> = Symbol('verified native pairing')
