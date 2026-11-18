import {createApprovedBrowserProfileReader,type BrowserProfileProvider} from '../src/browserPairingProfile.ts';

// 仅用于隔离构建 smoke 的公开合成候选，不是可登记或部署的真实配对记录。
const publicBytes=(length:number,value:number)=>btoa(String.fromCharCode(...new Uint8Array(length).fill(value))).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
const candidate=new TextEncoder().encode(JSON.stringify({
 version:1,
 profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:'Bw'},
 localSpki:publicBytes(91,1),hostSpki:publicBytes(91,2),
 trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},
 grant:{issuerSpki:publicBytes(91,3),principal:'04'.repeat(32),
  scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'},role:'Observer',budgetMs:10000,peerInstall:{budgetMs:1000},
  profile:{origin:'https://desk.invalid',rpId:'desk.invalid',credentialId:'Bw',credentialSpki:publicBytes(91,4),backupEligible:false,allowBackedUp:false}},
 relay:{mode:'rendezvous-v1',endpoint:'wss://relay.invalid/route',tunnel:publicBytes(16,9),keyId:publicBytes(16,8)},
 artifacts:{wasm:'https://desk.invalid/assets/client.wasm',glue:'https://desk.invalid/assets/client.js'},
}));

// 示例永远锁定。实际站点只能替换为其已存在、独立审核的 provider 实现。
// provider 自己读取完整原批准配置和原 handles，不接收 candidate 或 key 名。
// 绝不能用 candidate 作为 provider.configuration，不得在此自动 enrollment。
const provider:BrowserProfileProvider=Object.freeze({async readApproved(_signal:AbortSignal){return undefined;}});
export const readApprovedBrowserInputs=createApprovedBrowserProfileReader(candidate,provider);
