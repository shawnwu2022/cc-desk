import {createProductBrowserProfileReader} from './browserProductStartup.ts';

// 固定产品入口只暴露读取，不授予安装、撤销或凭据配置能力。
export const readApprovedBrowserInputs=createProductBrowserProfileReader();
