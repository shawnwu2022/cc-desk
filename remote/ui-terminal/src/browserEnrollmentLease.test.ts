import {test} from 'node:test';
import assert from 'node:assert/strict';
import {checkedBrowserEnrollmentLease,createBrowserEnrollmentLease} from './browserEnrollmentLease.ts';

test('EnrollmentLease_OptionalCloseOnlyContractPreservesOriginalIdentity_001',async()=>{
 const old=createBrowserEnrollmentLease();assert.equal(checkedBrowserEnrollmentLease(old.lease),old.lease);assert.equal('retire'in old.lease,false);old.revoke();await old.waitClosed();
 let calls=0;const owner=createBrowserEnrollmentLease(()=>{calls++;owner.revoke();return owner.waitClosed();});
 assert.equal(checkedBrowserEnrollmentLease(owner.lease),owner.lease);
 let release!:()=>void;owner.lease.retain(new Promise<void>(r=>{release=r}));let retired=false;const done=owner.lease.retire!().then(()=>{retired=true});await Promise.resolve();assert.equal(retired,false);assert.equal(owner.lease.signal.aborted,true);release();await done;assert.equal(calls,1);
});

test('EnrollmentLease_RejectsAccessorInheritedUnknownOrInvalidRetire_002',()=>{
 const base={signal:new AbortController().signal,retain:()=>{}},invalid=[{...base,retire:undefined},{...base,retire:'yes'},{...base,retire:()=>Promise.resolve(),extra:true},Object.assign(Object.create({retire:()=>Promise.resolve()}),base)];
 let getters=0;invalid.push(Object.defineProperty({...base},'retire',{enumerable:true,get(){getters++;return()=>Promise.resolve()}}) as any);
 for(const value of invalid)assert.throws(()=>checkedBrowserEnrollmentLease(value));assert.equal(getters,0);
});
