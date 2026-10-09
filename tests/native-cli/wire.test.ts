import { describe, expect, it } from 'vitest'
import goldens from '../fixtures/native-cli/wire-goldens.json'
import type { LaunchRequest, NativeSessionRef } from '@/types/cli'
import {
  nativeSessionKey,
  parseU64,
  validateLaunchRequest,
  validateWireBytes,
} from '@/utils/nativeIdentity'

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

describe('native CLI wire contract', () => {
  it('D05_Wire_U64Boundaries_01', () => {
    for (const value of goldens.validU64) {
      expect(parseU64(value).toString()).toBe(value)
    }
    expect(parseU64('9007199254740993')).toBe(9007199254740993n)
    for (const value of goldens.invalidU64) {
      expect(() => parseU64(value), value).toThrow('INVALID_REQUEST')
    }
  })

  it('D05_Wire_ValidLaunchGoldens_02', () => {
    for (const golden of goldens.validLaunchRequests) {
      expect(validateLaunchRequest(clone(golden.value)), golden.name).toEqual(golden.value)
    }
  })

  it('D05_Wire_RejectsUnknownCliAndKind_03', () => {
    const request = clone(goldens.validLaunchRequests[0].value) as Record<string, unknown>
    request.cli = 'Claude'
    expect(() => validateLaunchRequest(request)).toThrow('INVALID_REQUEST:cli')

    const invalidAction = clone(goldens.validLaunchRequests[0].value) as Record<string, unknown>
    invalidAction.action = { kind: 'continue-last' }
    expect(() => validateLaunchRequest(invalidAction)).toThrow('INVALID_REQUEST:action.kind')
  })

  it('D05_Wire_RejectsInvalidGenerationAndDimensions_04', () => {
    for (const generation of [-1, 1.5, 4294967296]) {
      const request = clone(goldens.validLaunchRequests[0].value) as Record<string, unknown>
      request.generation = generation
      expect(() => validateLaunchRequest(request), String(generation)).toThrow(
        'INVALID_REQUEST:generation',
      )
    }

    for (const [field, value] of [
      ['cols', 0],
      ['cols', 65536],
      ['rows', 1.5],
    ] as const) {
      const request = clone(goldens.validLaunchRequests[0].value) as Record<string, unknown>
      request[field] = value
      expect(() => validateLaunchRequest(request), `${field}=${value}`).toThrow(
        `INVALID_REQUEST:${field}`,
      )
    }
  })

  it('D05_Wire_RejectsNulWithoutEchoingContent_05', () => {
    const request = clone(goldens.validLaunchRequests[0].value) as Record<string, unknown>
    request.launchCwd = '/repo/secret\0payload'
    expect(() => validateLaunchRequest(request)).toThrow('INVALID_REQUEST:launchCwd')

    const argRequest = clone(goldens.validLaunchRequests[0].value) as Record<string, unknown>
    argRequest.extraArgs = ['fixture-token\0must-not-appear']
    try {
      validateLaunchRequest(argRequest)
      throw new Error('expected validation failure')
    } catch (error) {
      expect(String(error)).toContain('INVALID_REQUEST:extraArgs[0]')
      expect(String(error)).not.toContain('fixture-token')
    }
  })

  it('D05_Wire_RawCannotAlsoUseExtraArgs_06', () => {
    const request = clone(goldens.validLaunchRequests[3].value) as Record<string, unknown>
    request.extraArgs = ['--model', 'fixture']
    expect(() => validateLaunchRequest(request)).toThrow('INVALID_REQUEST:extraArgs')
  })

  it('D05_Identity_CliAndRootAreNamespaced_07', () => {
    const refs = goldens.nativeSessionRefs as NativeSessionRef[]
    const keys = refs.map(nativeSessionKey)
    expect(new Set(keys).size).toBe(refs.length)
    expect(keys[0]).toBe(JSON.stringify([
      refs[0].hostId,
      refs[0].cli,
      refs[0].sourceRootKey,
      refs[0].nativeSessionId,
    ]))
  })

  it('D05_Wire_BytesAreStrictOctets_08', () => {
    expect(validateWireBytes([0, 1, 127, 128, 255])).toEqual(
      new Uint8Array([0, 1, 127, 128, 255]),
    )
    for (const invalid of [[-1], [256], [1.5], ['1'], [Number.NaN]]) {
      expect(() => validateWireBytes(invalid)).toThrow('INVALID_REQUEST:bytes[0]')
    }
  })

  it('D05_Wire_TypeRoundTripKeepsLargeRevisionAsString_09', () => {
    const request = validateLaunchRequest(
      clone(goldens.validLaunchRequests[0].value),
    ) as LaunchRequest
    expect(request.expectedProfileRevision).toBe('9007199254740993')
    expect(JSON.parse(JSON.stringify(request)).expectedProfileRevision).toBe(
      '9007199254740993',
    )
  })
})
