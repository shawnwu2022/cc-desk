import {test} from 'node:test';import assert from 'node:assert/strict';
import {validBrowserPeerInstall} from './browserPeerMode.ts';
test('BrowserControlLease_ExplicitStrictMode_001',()=>{
 for(const role of ['Controller','Observer'])assert.equal(validBrowserPeerInstall({budgetMs:1000},role),true);
 assert.equal(validBrowserPeerInstall({budgetMs:1000,controlLease:'manual-v1'},'Controller'),true);
 for(const mode of [undefined,null,true,'auto-v1',{'manual-v1':null}])assert.equal(validBrowserPeerInstall({budgetMs:1000,controlLease:mode},'Controller'),false);
 assert.equal(validBrowserPeerInstall({budgetMs:1000,controlLease:'manual-v1'},'Observer'),false);
 assert.equal(validBrowserPeerInstall({budgetMs:1000,unattended:true},'Controller'),false);
 assert.equal(validBrowserPeerInstall(Object.assign(Object.create({controlLease:'manual-v1'}),{budgetMs:1000}),'Controller'),false);
});

test('BrowserLiveControl_ExplicitModeNotManual_002',()=>{
 assert.equal(validBrowserPeerInstall({budgetMs:1000,controlLease:'live-v1'},'Controller'),true);
 assert.equal(validBrowserPeerInstall({budgetMs:1000,controlLease:'live-v1'},'Observer'),false);
 for(const value of ['live-v2',{'live-v1':null}])assert.equal(validBrowserPeerInstall({budgetMs:1000,controlLease:value},'Controller'),false);
});

test('BrowserSnapshotPull_ExplicitLiveOnlyMode_003',()=>{
 const enabled={budgetMs:1000,controlLease:'live-v1',snapshotPull:'snapshot-pull-v1'};
 assert.equal(validBrowserPeerInstall(enabled,'Controller'),true);
 for(const value of [undefined,null,true,'snapshot-pull-v2',{'snapshot-pull-v1':null}])
  assert.equal(validBrowserPeerInstall({...enabled,snapshotPull:value},'Controller'),false);
 for(const controlLease of [undefined,'manual-v1'])
  assert.equal(validBrowserPeerInstall({...enabled,controlLease},'Controller'),false);
 assert.equal(validBrowserPeerInstall(enabled,'Observer'),false);
 assert.equal(validBrowserPeerInstall({...enabled,other:true},'Controller'),false);
 assert.equal(validBrowserPeerInstall(Object.assign(Object.create({snapshotPull:'snapshot-pull-v1'}),{budgetMs:1000,controlLease:'live-v1'}),'Controller'),false);
});