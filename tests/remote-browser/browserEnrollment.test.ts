import {beforeAll,expect,test,vi} from 'vitest'
import {createECDH} from 'node:crypto'
import {createBrowserEnrollmentAuthority} from '../../remote/ui-terminal/src/browserEnrollment'
import {createApprovedBrowserProfileReader,type BrowserProfileHandles} from '../../remote/ui-terminal/src/browserPairingProfile'
import {approvedBrowserInputs} from '../../remote/ui-terminal/src/browserEntry'
import {browserConnectionSource} from '../../remote/ui-terminal/src/browserSource'

const configuration={version:1,profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:'Bw'},localSpki:Buffer.alloc(91,1).toString('base64url'),hostSpki:Buffer.alloc(91,2).toString('base64url'),trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},grant:{issuerSpki:Buffer.alloc(91,3).toString('base64url'),principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000},profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:'Bw',credentialSpki:Buffer.alloc(91,4).toString('base64url'),backupEligible:false,allowBackedUp:false}},relay:{endpoint:'wss://relay.invalid/route',tunnel:Buffer.alloc(16,9).toString('base64url'),keyId:Buffer.alloc(16,8).toString('base64url')},artifacts:{wasm:'https://desk.invalid/client.wasm',glue:'https://desk.invalid/client.js'}}
const bytes=()=>Uint8Array.from(Buffer.from(JSON.stringify(configuration)))
let handles:BrowserProfileHandles
beforeAll(async()=>{
 const keys:CryptoKey[]=[]
 for(const scalar of [1,2]){const d=Buffer.alloc(32);d[31]=scalar;const ec=createECDH('prime256v1');ec.setPrivateKey(d);const point=ec.getPublicKey();keys.push(await crypto.subtle.importKey('jwk',{kty:'EC',crv:'P-256',d:d.toString('base64url'),x:point.subarray(1,33).toString('base64url'),y:point.subarray(33).toString('base64url')},{name:'ECDSA',namedCurve:'P-256'},false,['sign']))}
 handles={tlsKey:keys[0],grantKey:keys[1],relaySign:vi.fn(async()=>new Uint8Array(64)),present:vi.fn(async()=>{})}
})
const material=()=>({configuration:bytes(),handles})

test('Enrollment_MissingBeforeApprovalIsPermanentlyLocked_001',async()=>{
 const owner=createBrowserEnrollmentAuthority(),reader=createApprovedBrowserProfileReader(bytes(),owner.provider)
 expect(await reader(new AbortController().signal)).toBeUndefined()
 await expect(owner.approve(async()=>material())).rejects.toThrow('browser enrollment unavailable')
 expect(await reader(new AbortController().signal)).toBeUndefined()
 owner.revoke();await owner.waitClosed()
})

test('Enrollment_IndependentOriginalSnapshotAndHandles_002',async()=>{
 const owner=createBrowserEnrollmentAuthority(),original=material(),load=vi.fn(async()=>original)
 await owner.approve(load)
 original.configuration.fill(0)
 const reader=createApprovedBrowserProfileReader(bytes(),owner.provider),input=await reader(new AbortController().signal)
 expect(load).toHaveBeenCalledTimes(1);expect(load.mock.calls[0]).toHaveLength(1)
 expect(input?.profile.tlsKey).toBe(handles.tlsKey);expect(input?.profile.grantKey).toBe(handles.grantKey)
 expect(input?.relay.identity.sign).toBe(handles.relaySign);expect(input?.present).toBe(handles.present)
 expect(input?.enrollment).toBe(owner.provider.enrollment)
 expect(approvedBrowserInputs(input!).enrollment).toBe(input?.enrollment)
 input!.hostSpki.fill(0)
 expect((await reader(new AbortController().signal))?.hostSpki[0]).toBe(2)
 expect(handles.relaySign).not.toHaveBeenCalled();expect(handles.present).not.toHaveBeenCalled()
 owner.revoke();await owner.waitClosed()
})

test('Enrollment_RevokeRejectsCachedReaderAndSource_003',async()=>{
 const owner=createBrowserEnrollmentAuthority();await owner.approve(async()=>material())
 const reader=createApprovedBrowserProfileReader(bytes(),owner.provider),signal=new AbortController().signal,input=await reader(signal)
 const source=browserConnectionSource(input)
 owner.revoke()
 await expect(reader(signal)).rejects.toThrow('browser pairing profile unavailable')
 await expect(reader(new AbortController().signal)).rejects.toThrow('browser pairing profile unavailable')
 await expect(source.open(new AbortController().signal)).rejects.toThrow('browser entry unavailable')
 expect(()=>approvedBrowserInputs(input!)).toThrow()
 await expect(owner.approve(async()=>material())).rejects.toThrow('browser enrollment unavailable')
 await owner.waitClosed()
})

test('Enrollment_RevokeRetainsOriginalLateApproval_004',async()=>{
 const owner=createBrowserEnrollmentAuthority();let release!:(v:ReturnType<typeof material>)=>void,signal:AbortSignal|undefined
 const approval=owner.approve(async s=>{signal=s;return new Promise(r=>{release=r})})
 const reader=createApprovedBrowserProfileReader(bytes(),owner.provider),read=reader(new AbortController().signal)
 const outcomes=[approval.then(()=>false,()=>true),read.then(()=>false,()=>true)]
 await Promise.resolve();await Promise.resolve();owner.revoke()
 expect(signal?.aborted).toBe(true)
 let settled=false;void owner.waitClosed().then(()=>{settled=true},()=>{settled=true})
 await Promise.resolve();await Promise.resolve();expect(settled).toBe(false)
 release(material());expect(await Promise.all(outcomes)).toEqual([true,true])
 await owner.waitClosed()
 await expect(reader(new AbortController().signal)).rejects.toThrow('browser pairing profile unavailable')
})

test('Enrollment_OriginalReadAbortSealsRecord_005',async()=>{
 const owner=createBrowserEnrollmentAuthority();let release!:(v:ReturnType<typeof material>)=>void
 const approval=owner.approve(async()=>new Promise(r=>{release=r}));void approval.catch(()=>{})
 const controller=new AbortController(),reader=createApprovedBrowserProfileReader(bytes(),owner.provider),read=reader(controller.signal)
 const rejected=read.then(()=>false,()=>true)
 await Promise.resolve();await Promise.resolve();controller.abort();release(material())
 expect(await rejected).toBe(true)
 await expect(reader(new AbortController().signal)).rejects.toThrow('browser pairing profile unavailable')
 expect(owner.provider.enrollment.signal.aborted).toBe(true)
 await owner.waitClosed()
})

test('Enrollment_WaitsForOriginalOwnersAndRetainsUncertainty_006',async()=>{
 const owner=createBrowserEnrollmentAuthority();await owner.approve(async()=>material())
 let resolve!:()=>void,reject!:(e:Error)=>void
 owner.provider.enrollment.retain(new Promise<void>(r=>{resolve=r}))
 owner.provider.enrollment.retain(new Promise<void>((_,j)=>{reject=j}))
 let ended=false;void owner.waitClosed().then(()=>{ended=true},()=>{ended=true})
 await Promise.resolve();expect(ended).toBe(false)
 owner.revoke();resolve();await Promise.resolve();expect(ended).toBe(false)
 expect(()=>owner.provider.enrollment.retain(Promise.resolve())).toThrow()
 reject(Error('PRIVATE_RETIREMENT_FAILURE'))
 await expect(owner.waitClosed()).rejects.toThrow(/^browser enrollment retirement uncertain$/)
})

test('Enrollment_DuplicateApprovalAndLoadFailureAreClosed_007',async()=>{
 const owner=createBrowserEnrollmentAuthority(),load=vi.fn(async()=>{throw Error('PRIVATE_LOAD')})
 const approval=owner.approve(load)
 await expect(owner.approve(async()=>material())).rejects.toThrow('browser enrollment unavailable')
 await expect(approval).rejects.toThrow(/^browser enrollment unavailable$/)
 const reader=createApprovedBrowserProfileReader(bytes(),owner.provider)
 await expect(reader(new AbortController().signal)).rejects.toThrow()
 owner.revoke();await expect(owner.waitClosed()).rejects.toThrow(/^browser enrollment retirement uncertain$/)
 expect(load).toHaveBeenCalledTimes(1)
})

test.each([{}, {signal:{aborted:false},retain:()=>{}}, {signal:new AbortController().signal,retain:true}])('Enrollment_JsonCannotForgeLifecycle_008_%#',lease=>{
 expect(()=>createApprovedBrowserProfileReader(bytes(),{readApproved:async()=>material(),enrollment:lease} as any)).toThrow('browser pairing profile unavailable')
})

test('Enrollment_MismatchedCandidateDoesNotChangeAuthority_009',async()=>{
 const owner=createBrowserEnrollmentAuthority();await owner.approve(async()=>material())
 const changed=structuredClone(configuration);changed.grant.scope.run='OTHER_PUBLIC_TEST_ONLY'
 const reader=createApprovedBrowserProfileReader(Uint8Array.from(Buffer.from(JSON.stringify(changed))),owner.provider)
 await expect(reader(new AbortController().signal)).rejects.toThrow('browser pairing profile unavailable')
 expect(handles.relaySign).not.toHaveBeenCalled();expect(handles.present).not.toHaveBeenCalled()
 owner.revoke();await owner.waitClosed()
})

test('Enrollment_PreAbortedDirectReadRetiresBeforeClosed_010',async()=>{
 const owner=createBrowserEnrollmentAuthority();await owner.approve(async()=>material())
 const controller=new AbortController();controller.abort()
 let readSettled=false
 const read=owner.provider.readApproved(controller.signal)
 const observed=read.then(()=>{readSettled=true},()=>{readSettled=true})
 const closed=owner.waitClosed().then(()=>{expect(readSettled).toBe(true)})
 await observed;await closed
})

test('Enrollment_DirectMissingReadClosesApprovalSynchronously_011',async()=>{
 const owner=createBrowserEnrollmentAuthority(),read=owner.provider.readApproved(new AbortController().signal)
 await expect(owner.approve(async()=>material())).rejects.toThrow('browser enrollment unavailable')
 expect(await read).toBeUndefined();owner.revoke();await owner.waitClosed()
})

test('Enrollment_RevokeBeforeLoadSkipsOriginalLoader_012',async()=>{
 const owner=createBrowserEnrollmentAuthority(),load=vi.fn(async()=>material()),approval=owner.approve(load)
 owner.revoke();await expect(approval).rejects.toThrow('browser enrollment unavailable')
 await owner.waitClosed();expect(load).not.toHaveBeenCalled()
})

test('Enrollment_InvalidDirectSignalCannotStrandRetirement_013',async()=>{
 const owner=createBrowserEnrollmentAuthority();await owner.approve(async()=>material())
 await expect(owner.provider.readApproved({} as AbortSignal)).rejects.toThrow('browser enrollment unavailable')
 owner.revoke();await owner.waitClosed()
})

test('Enrollment_LaterCallerCancellationDoesNotRevokeOriginal_014',async()=>{
 const owner=createBrowserEnrollmentAuthority();await owner.approve(async()=>material())
 expect(await owner.provider.readApproved(new AbortController().signal)).toBeDefined()
 const later=new AbortController();later.abort()
 await expect(owner.provider.readApproved(later.signal)).rejects.toThrow('browser enrollment unavailable')
 expect(owner.provider.enrollment.signal.aborted).toBe(false)
 expect(await owner.provider.readApproved(new AbortController().signal)).toBeDefined()
 owner.revoke();await owner.waitClosed()
})

test('Enrollment_MutableProviderLeaseCannotReplaceCapturedOwner_015',async()=>{
 const owner=createBrowserEnrollmentAuthority();await owner.approve(async()=>material())
 const enrollment={signal:owner.provider.enrollment.signal,retain:owner.provider.enrollment.retain}
 const reader=createApprovedBrowserProfileReader(bytes(),{readApproved:owner.provider.readApproved,enrollment})
 enrollment.signal=new AbortController().signal;enrollment.retain=()=>{}
 const input=await reader(new AbortController().signal)
 expect(input?.enrollment?.signal).toBe(owner.provider.enrollment.signal)
 expect(input?.enrollment?.retain).toBe(owner.provider.enrollment.retain)
 owner.revoke();await expect(reader(new AbortController().signal)).rejects.toThrow('browser pairing profile unavailable')
 await owner.waitClosed()
})

test('Enrollment_RecordAccessorsCannotRunOrCreateApproval_016',async()=>{
 const owner=createBrowserEnrollmentAuthority(),getter=vi.fn(()=>bytes())
 const record={handles};Object.defineProperty(record,'configuration',{enumerable:true,get:getter})
 await expect(owner.approve(async()=>record as any)).rejects.toThrow('browser enrollment unavailable')
 expect(getter).not.toHaveBeenCalled();expect(owner.provider.enrollment.signal.aborted).toBe(true)
 await owner.waitClosed()
})
