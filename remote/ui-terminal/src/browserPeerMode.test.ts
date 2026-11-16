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
