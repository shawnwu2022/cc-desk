import { beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useSessionIndexMaintenanceStore } from '@/stores/sessionIndexMaintenance'
const io=vi.hoisted(()=>({maintenance:vi.fn()}))
vi.mock('@/api/tauri',()=>({sessionIndexMaintenance:io.maintenance}))
beforeEach(()=>{setActivePinia(createPinia());vi.clearAllMocks();io.maintenance.mockImplementation(async(id,action)=>({id,state:action==='begin'?'staging':action==='commit'?'committed':'cancelled'}))})

it('IndexRebuild_CommitsOnlyAfterCompleteScan_001', async()=>{
  const store=useSessionIndexMaintenanceStore(), published=vi.fn()
  let release!:()=>void
  store.configure(async control=>{control.progress(0,2);await new Promise<void>(resolve=>{release=resolve});control.progress(2,2)},published)
  const work=store.run();await Promise.resolve();await Promise.resolve()
  expect(store.busy).toBe(true);expect(io.maintenance).toHaveBeenCalledTimes(1);expect(published).not.toHaveBeenCalled()
  release();await work
  expect(io.maintenance.mock.calls.map(call=>call[1])).toEqual(['begin','commit'])
  expect(store.state).toBe('completed');expect(published).toHaveBeenCalledOnce()
})
it('IndexRebuild_CancelWaitsForInFlightBeforeDiscarding_002',async()=>{
  const store=useSessionIndexMaintenanceStore(),published=vi.fn();let release!:()=>void
  store.configure(async()=>{await new Promise<void>(resolve=>{release=resolve})},published)
  const work=store.run();await Promise.resolve();await Promise.resolve();store.cancel()
  expect(io.maintenance).toHaveBeenCalledTimes(1)
  release();await work
  expect(io.maintenance.mock.calls.map(call=>call[1])).toEqual(['begin','cancel'])
  expect(store.state).toBe('cancelled');expect(published).not.toHaveBeenCalled()
})
it('IndexRebuild_SourceFailurePreservesOldPublication_003',async()=>{
  const store=useSessionIndexMaintenanceStore(),published=vi.fn()
  store.configure(async()=>{throw new Error('private path/env')},published)
  await store.run()
  expect(io.maintenance.mock.calls.map(call=>call[1])).toEqual(['begin','cancel'])
  expect(store.state).toBe('failed');expect(store.error).toBe('indexRebuildFailed');expect(published).not.toHaveBeenCalled()
})

it('IndexRebuild_CommitFailureCancelsOnlyConfirmedStage_004',async()=>{
  const store=useSessionIndexMaintenanceStore(),published=vi.fn()
  store.configure(async()=>{},published)
  io.maintenance.mockImplementation(async(id,action)=>{
    if(action==='commit') throw new Error('MAINTENANCE_BUSY')
    return {id,state:action==='cancel'?'cancelled':'staging'}
  })
  await store.run()
  expect(io.maintenance.mock.calls.map(call=>call[1])).toEqual(['begin','commit','status','cancel'])
  expect(store.state).toBe('failed');expect(published).not.toHaveBeenCalled()
  expect(store.needsReconciliation).toBe(false)
})

it('IndexRebuild_UncertainCommitRechecksSameTransaction_005',async()=>{
  const store=useSessionIndexMaintenanceStore(),published=vi.fn(),scan=vi.fn(async()=>{})
  store.configure(scan,published)
  io.maintenance.mockImplementation(async(id,action)=>{
    if(action!=='begin') throw new Error('TRANSPORT_UNAVAILABLE')
    return {id,state:'staging'}
  })
  await store.run()
  expect(store.needsReconciliation).toBe(true)
  io.maintenance.mockImplementation(async(id)=>({id,state:'committed'}))
  await store.run()
  expect(io.maintenance.mock.calls.map(call=>call[1])).toEqual(['begin','commit','status','status'])
  expect(new Set(io.maintenance.mock.calls.map(call=>call[0])).size).toBe(1)
  expect(scan).toHaveBeenCalledOnce();expect(published).toHaveBeenCalledOnce()
  expect(store.state).toBe('completed');expect(store.needsReconciliation).toBe(false)
})

it('IndexRebuild_RejectedBeginDoesNotStrandPendingIdentity_006',async()=>{
  const store=useSessionIndexMaintenanceStore(),scan=vi.fn(async()=>{})
  store.configure(scan,vi.fn())
  io.maintenance.mockImplementation(async(id,action)=>{
    if(action==='begin') throw new Error('MAINTENANCE_BUSY')
    return {id,state:'absent'}
  })
  await store.run()
  expect(io.maintenance.mock.calls.map(call=>call[1])).toEqual(['begin','status','cancel'])
  expect(store.state).toBe('failed');expect(store.needsReconciliation).toBe(false)
  expect(scan).not.toHaveBeenCalled()
})

it('IndexRebuild_PublishedRefreshFailureRetriesOnlyRefresh_007',async()=>{
  const store=useSessionIndexMaintenanceStore(),scan=vi.fn(async()=>{})
  store.configure(scan,async()=>{throw new Error('refresh failed')})
  await store.run()
  expect(store.state).toBe('completed');expect(store.error).toBe('indexRebuildRefreshFailed')
  expect(store.needsRefresh).toBe(true)
  const refresh=vi.fn();store.configure(scan,refresh);await store.run()
  expect(io.maintenance.mock.calls.map(call=>call[1])).toEqual(['begin','commit'])
  expect(scan).toHaveBeenCalledOnce();expect(refresh).toHaveBeenCalledOnce()
  expect(store.needsRefresh).toBe(false);expect(store.error).toBeNull()
})

it('IndexRebuild_PublicationRefreshUsesCurrentRuntime_008',async()=>{
  const store=useSessionIndexMaintenanceStore(),oldRefresh=vi.fn(),newRefresh=vi.fn()
  let committed!:(value:unknown)=>void
  io.maintenance.mockImplementation(async(id,action)=>action==='commit'?new Promise(resolve=>{committed=resolve}):({id,state:'staging'}))
  const dispose=store.configure(async()=>{},oldRefresh)
  const work=store.run();await Promise.resolve();await Promise.resolve();await Promise.resolve()
  dispose();store.configure(async()=>{},newRefresh)
  committed({state:'committed'});await work
  expect(oldRefresh).not.toHaveBeenCalled();expect(newRefresh).toHaveBeenCalledOnce()
})
