import type { InjectionKey } from 'vue'

/** Capture the current runtime attempt without giving presentation components lifecycle access. */
export const SESSION_INTERACTION_OWNER: InjectionKey<(id: string) => () => boolean> = Symbol('session-interaction-owner')
