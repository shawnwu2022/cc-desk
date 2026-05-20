/**
 * Provider 预设配置模板（复用 cc-switch）
 */
import type { ProviderPreset, ProviderCategory } from '@/types/provider'

export const providerPresets: ProviderPreset[] = [
  {
    name: 'Claude Official',
    websiteUrl: 'https://www.anthropic.com/claude-code',
    settingsConfig: {
      env: {},
    },
    isOfficial: true,
    category: 'official',
    icon: 'anthropic',
    iconColor: '#D4915D',
  },
  {
    name: 'DeepSeek',
    websiteUrl: 'https://platform.deepseek.com',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.deepseek.com/anthropic',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'deepseek-v4-pro',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'deepseek-v4-flash',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'deepseek-v4-pro',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'deepseek-v4-pro',
      },
    },
    category: 'cn_official',
    modelsUrl: 'https://api.deepseek.com/models',
    icon: 'deepseek',
    iconColor: '#1E88E5',
  },
  {
    name: 'Zhipu GLM',
    websiteUrl: 'https://open.bigmodel.cn',
    apiKeyUrl: 'https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://open.bigmodel.cn/api/anthropic',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'GLM-5.1',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'GLM-5.1',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'GLM-5.1',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'GLM-5.1',
      },
    },
    category: 'cn_official',
    icon: 'zhipu',
    iconColor: '#0F62FE',
  },
  {
    name: 'Zhipu GLM en',
    websiteUrl: 'https://z.ai',
    apiKeyUrl: 'https://z.ai/subscribe?ic=8JVLJQFSKB',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.z.ai/api/anthropic',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'GLM-5.1',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'GLM-5.1',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'GLM-5.1',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'GLM-5.1',
      },
    },
    category: 'cn_official',
    icon: 'zhipu',
    iconColor: '#0F62FE',
  },
  {
    name: 'Baidu Qianfan Coding Plan',
    websiteUrl: 'https://cloud.baidu.com/product/qianfan_modelbuilder',
    apiKeyUrl: 'https://console.bce.baidu.com/qianfan/ais/console/applicationConsole/application',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://qianfan.baidubce.com/anthropic/coding',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'qianfan-code-latest',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'qianfan-code-latest',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'qianfan-code-latest',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'qianfan-code-latest',
      },
    },
    category: 'cn_official',
    endpointCandidates: ['https://qianfan.baidubce.com/anthropic/coding'],
    icon: 'baidu',
    iconColor: '#2932E1',
  },
  {
    name: 'Bailian',
    websiteUrl: 'https://bailian.console.aliyun.com',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://dashscope.aliyuncs.com/apps/anthropic',
        ANTHROPIC_AUTH_TOKEN: '',
      },
    },
    category: 'cn_official',
    icon: 'bailian',
    iconColor: '#624AFF',
  },
  {
    name: 'Bailian For Coding',
    websiteUrl: 'https://bailian.console.aliyun.com',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://coding.dashscope.aliyuncs.com/apps/anthropic',
        ANTHROPIC_AUTH_TOKEN: '',
      },
    },
    category: 'cn_official',
    icon: 'bailian',
    iconColor: '#624AFF',
  },
  {
    name: 'Kimi',
    websiteUrl: 'https://platform.moonshot.cn/console',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.moonshot.cn/anthropic',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'kimi-k2.6',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'kimi-k2.6',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'kimi-k2.6',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'kimi-k2.6',
      },
    },
    category: 'cn_official',
    icon: 'kimi',
    iconColor: '#6366F1',
  },
  {
    name: 'Kimi For Coding',
    websiteUrl: 'https://www.kimi.com/code/docs/',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.kimi.com/coding/',
        ANTHROPIC_AUTH_TOKEN: '',
      },
    },
    category: 'cn_official',
    icon: 'kimi',
    iconColor: '#6366F1',
  },
  {
    name: 'StepFun',
    websiteUrl: 'https://platform.stepfun.com/step-plan',
    apiKeyUrl: 'https://platform.stepfun.com/interface-key',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.stepfun.com/step_plan',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'step-3.5-flash-2603',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'step-3.5-flash-2603',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'step-3.5-flash-2603',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'step-3.5-flash-2603',
      },
    },
    category: 'cn_official',
    endpointCandidates: ['https://api.stepfun.com/step_plan'],
    icon: 'stepfun',
    iconColor: '#16D6D2',
  },
  {
    name: 'StepFun en',
    websiteUrl: 'https://platform.stepfun.ai/step-plan',
    apiKeyUrl: 'https://platform.stepfun.ai/interface-key',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.stepfun.ai/step_plan',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'step-3.5-flash-2603',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'step-3.5-flash-2603',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'step-3.5-flash-2603',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'step-3.5-flash-2603',
      },
    },
    category: 'cn_official',
    endpointCandidates: ['https://api.stepfun.ai/step_plan'],
    icon: 'stepfun',
    iconColor: '#16D6D2',
  },
  {
    name: 'MiniMax',
    websiteUrl: 'https://platform.minimaxi.com',
    apiKeyUrl: 'https://platform.minimaxi.com/subscribe/coding-plan',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.minimaxi.com/anthropic',
        ANTHROPIC_AUTH_TOKEN: '',
        API_TIMEOUT_MS: '3000000',
        CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: 1,
        ANTHROPIC_MODEL: 'MiniMax-M2.7',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'MiniMax-M2.7',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'MiniMax-M2.7',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'MiniMax-M2.7',
      },
    },
    category: 'cn_official',
    isPartner: true,
    partnerPromotionKey: 'minimax_cn',
    icon: 'minimax',
    iconColor: '#FF6B6B',
  },
  {
    name: 'MiniMax en',
    websiteUrl: 'https://platform.minimax.io',
    apiKeyUrl: 'https://platform.minimax.io/subscribe/coding-plan',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.minimax.io/anthropic',
        ANTHROPIC_AUTH_TOKEN: '',
        API_TIMEOUT_MS: '3000000',
        CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: 1,
        ANTHROPIC_MODEL: 'MiniMax-M2.7',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'MiniMax-M2.7',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'MiniMax-M2.7',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'MiniMax-M2.7',
      },
    },
    category: 'cn_official',
    isPartner: true,
    partnerPromotionKey: 'minimax_en',
    icon: 'minimax',
    iconColor: '#FF6B6B',
  },
  {
    name: 'DouBaoSeed',
    websiteUrl: 'https://www.volcengine.com/product/doubao',
    apiKeyUrl: 'https://www.volcengine.com/product/doubao',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://ark.cn-beijing.volces.com/api/coding',
        ANTHROPIC_AUTH_TOKEN: '',
        API_TIMEOUT_MS: '3000000',
        ANTHROPIC_MODEL: 'doubao-seed-2-0-code-preview-latest',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'doubao-seed-2-0-code-preview-latest',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'doubao-seed-2-0-code-preview-latest',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'doubao-seed-2-0-code-preview-latest',
      },
    },
    category: 'cn_official',
    icon: 'doubao',
    iconColor: '#3370FF',
  },
  {
    name: 'Xiaomi MiMo',
    websiteUrl: 'https://platform.xiaomimimo.com',
    apiKeyUrl: 'https://platform.xiaomimimo.com/#/console/api-keys',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.xiaomimimo.com/anthropic',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'mimo-v2-pro',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'mimo-v2-pro',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'mimo-v2-pro',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'mimo-v2-pro',
      },
    },
    category: 'cn_official',
    icon: 'xiaomimimo',
    iconColor: '#000000',
  },
  {
    name: 'ModelScope',
    websiteUrl: 'https://modelscope.cn',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api-inference.modelscope.cn',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'ZhipuAI/GLM-5',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'ZhipuAI/GLM-5',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'ZhipuAI/GLM-5',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'ZhipuAI/GLM-5',
      },
    },
    category: 'aggregator',
    icon: 'modelscope',
    iconColor: '#624AFF',
  },
  {
    name: 'SiliconFlow',
    websiteUrl: 'https://siliconflow.cn',
    apiKeyUrl: 'https://cloud.siliconflow.cn/i/drGuwc9k',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.siliconflow.cn',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'Pro/MiniMaxAI/MiniMax-M2.7',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'Pro/MiniMaxAI/MiniMax-M2.7',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'Pro/MiniMaxAI/MiniMax-M2.7',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'Pro/MiniMaxAI/MiniMax-M2.7',
      },
    },
    category: 'aggregator',
    isPartner: true,
    partnerPromotionKey: 'siliconflow',
    icon: 'siliconflow',
    iconColor: '#6E29F6',
  },
  {
    name: 'SiliconFlow en',
    websiteUrl: 'https://siliconflow.com',
    apiKeyUrl: 'https://cloud.siliconflow.cn/i/drGuwc9k',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.siliconflow.com',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'MiniMaxAI/MiniMax-M2.7',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'MiniMaxAI/MiniMax-M2.7',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'MiniMaxAI/MiniMax-M2.7',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'MiniMaxAI/MiniMax-M2.7',
      },
    },
    category: 'aggregator',
    isPartner: true,
    partnerPromotionKey: 'siliconflow',
    icon: 'siliconflow',
    iconColor: '#000000',
  },
  {
    name: 'OpenRouter',
    websiteUrl: 'https://openrouter.ai',
    apiKeyUrl: 'https://openrouter.ai/keys',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://openrouter.ai/api',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'anthropic/claude-sonnet-4.6',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'anthropic/claude-haiku-4.5',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'anthropic/claude-sonnet-4.6',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'anthropic/claude-opus-4.7',
      },
    },
    category: 'aggregator',
    icon: 'openrouter',
    iconColor: '#6566F1',
  },
  {
    name: 'AiHubMix',
    websiteUrl: 'https://aihubmix.com',
    apiKeyUrl: 'https://aihubmix.com',
    apiKeyField: 'ANTHROPIC_API_KEY',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://aihubmix.com',
        ANTHROPIC_API_KEY: '',
      },
    },
    endpointCandidates: ['https://aihubmix.com', 'https://api.aihubmix.com'],
    category: 'aggregator',
    icon: 'aihubmix',
    iconColor: '#006FFB',
  },
  {
    name: 'DMXAPI',
    websiteUrl: 'https://www.dmxapi.cn',
    apiKeyUrl: 'https://www.dmxapi.cn',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://www.dmxapi.cn',
        ANTHROPIC_AUTH_TOKEN: '',
      },
    },
    endpointCandidates: ['https://www.dmxapi.cn', 'https://api.dmxapi.cn'],
    category: 'aggregator',
    isPartner: true,
    partnerPromotionKey: 'dmxapi',
  },
  {
    name: 'TheRouter',
    websiteUrl: 'https://therouter.ai',
    apiKeyUrl: 'https://dashboard.therouter.ai',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.therouter.ai',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_API_KEY: '',
        ANTHROPIC_MODEL: 'anthropic/claude-sonnet-4.6',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'anthropic/claude-haiku-4.5',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'anthropic/claude-sonnet-4.6',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'anthropic/claude-opus-4.7',
      },
    },
    category: 'aggregator',
    endpointCandidates: ['https://api.therouter.ai'],
  },
  {
    name: 'Novita AI',
    websiteUrl: 'https://novita.ai',
    apiKeyUrl: 'https://novita.ai',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.novita.ai/anthropic',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'zai-org/glm-5',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'zai-org/glm-5',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'zai-org/glm-5',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'zai-org/glm-5',
      },
    },
    category: 'aggregator',
    endpointCandidates: ['https://api.novita.ai/anthropic'],
    icon: 'novita',
    iconColor: '#000000',
  },
  {
    name: 'Shengsuanyun',
    websiteUrl: 'https://www.shengsuanyun.com',
    apiKeyUrl: 'https://www.shengsuanyun.com/?from=CH_4HHXMRYF',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://router.shengsuanyun.com/api',
        ANTHROPIC_AUTH_TOKEN: '',
      },
    },
    category: 'aggregator',
    isPartner: true,
    partnerPromotionKey: 'shengsuanyun',
    icon: 'shengsuanyun',
  },
  {
    name: 'Gemini Native',
    websiteUrl: 'https://ai.google.dev/gemini-api',
    apiKeyUrl: 'https://aistudio.google.com/app/apikey',
    apiKeyField: 'ANTHROPIC_API_KEY',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://generativelanguage.googleapis.com',
        ANTHROPIC_API_KEY: '',
        ANTHROPIC_MODEL: 'gemini-3.1-pro',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'gemini-3-flash',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'gemini-3.1-pro',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'gemini-3.1-pro',
      },
    },
    category: 'third_party',
    apiFormat: 'gemini_native',
    endpointCandidates: ['https://generativelanguage.googleapis.com'],
    icon: 'gemini',
    iconColor: '#4285F4',
  },
  {
    name: 'GitHub Copilot',
    websiteUrl: 'https://github.com/features/copilot',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.githubcopilot.com',
        ANTHROPIC_MODEL: 'claude-sonnet-4.6',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'claude-haiku-4.5',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'claude-sonnet-4.6',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'claude-sonnet-4.6',
      },
    },
    category: 'third_party',
    apiFormat: 'openai_chat',
    providerType: 'github_copilot',
    requiresOAuth: true,
    icon: 'github',
    iconColor: '#000000',
  },
  {
    name: 'Codex',
    websiteUrl: 'https://openai.com/chatgpt/pricing',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://chatgpt.com/backend-api/codex',
        ANTHROPIC_MODEL: 'gpt-5.4',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'gpt-5.4-mini',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'gpt-5.4',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'gpt-5.4',
      },
    },
    category: 'third_party',
    apiFormat: 'openai_responses',
    providerType: 'codex_oauth',
    requiresOAuth: true,
    icon: 'openai',
    iconColor: '#000000',
  },
  {
    name: 'Nvidia',
    websiteUrl: 'https://build.nvidia.com',
    apiKeyUrl: 'https://build.nvidia.com/settings/api-keys',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://integrate.api.nvidia.com',
        ANTHROPIC_AUTH_TOKEN: '',
        ANTHROPIC_MODEL: 'moonshotai/kimi-k2.5',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'moonshotai/kimi-k2.5',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'moonshotai/kimi-k2.5',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'moonshotai/kimi-k2.5',
      },
    },
    category: 'aggregator',
    apiFormat: 'openai_chat',
    icon: 'nvidia',
    iconColor: '#000000',
  },
  {
    name: 'PackyCode',
    websiteUrl: 'https://www.packyapi.com',
    apiKeyUrl: 'https://www.packyapi.com/register?aff=cc-switch',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://www.packyapi.com',
        ANTHROPIC_AUTH_TOKEN: '',
      },
    },
    endpointCandidates: ['https://www.packyapi.com', 'https://api-slb.packyapi.com'],
    category: 'third_party',
    isPartner: true,
    partnerPromotionKey: 'packycode',
    icon: 'packycode',
  },
  {
    name: 'Cubence',
    websiteUrl: 'https://cubence.com',
    apiKeyUrl: 'https://cubence.com/signup?code=CCSWITCH&source=ccs',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.cubence.com',
        ANTHROPIC_AUTH_TOKEN: '',
      },
    },
    endpointCandidates: [
      'https://api.cubence.com',
      'https://api-cf.cubence.com',
      'https://api-dmit.cubence.com',
      'https://api-bwg.cubence.com',
    ],
    category: 'third_party',
    isPartner: true,
    partnerPromotionKey: 'cubence',
    icon: 'cubence',
    iconColor: '#000000',
  },
  {
    name: 'AIGoCode',
    websiteUrl: 'https://aigocode.com',
    apiKeyUrl: 'https://aigocode.com/invite/CC-SWITCH',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://api.aigocode.com',
        ANTHROPIC_AUTH_TOKEN: '',
      },
    },
    endpointCandidates: ['https://api.aigocode.com'],
    category: 'third_party',
    isPartner: true,
    partnerPromotionKey: 'aigocode',
    icon: 'aigocode',
    iconColor: '#5B7FFF',
  },
  {
    name: 'AWS Bedrock (AKSK)',
    websiteUrl: 'https://aws.amazon.com/bedrock/',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://bedrock-runtime.${AWS_REGION}.amazonaws.com',
        AWS_ACCESS_KEY_ID: '${AWS_ACCESS_KEY_ID}',
        AWS_SECRET_ACCESS_KEY: '${AWS_SECRET_ACCESS_KEY}',
        AWS_REGION: '${AWS_REGION}',
        ANTHROPIC_MODEL: 'global.anthropic.claude-opus-4-7',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'global.anthropic.claude-haiku-4-5-20251001-v1:0',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'global.anthropic.claude-sonnet-4-6',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'global.anthropic.claude-opus-4-7',
        CLAUDE_CODE_USE_BEDROCK: '1',
      },
    },
    category: 'cloud_provider',
    templateValues: {
      AWS_REGION: {
        label: 'AWS Region',
        placeholder: 'us-west-2',
        editorValue: 'us-west-2',
      },
      AWS_ACCESS_KEY_ID: {
        label: 'Access Key ID',
        placeholder: 'AKIA...',
        editorValue: '',
      },
      AWS_SECRET_ACCESS_KEY: {
        label: 'Secret Access Key',
        placeholder: 'your-secret-key',
        editorValue: '',
      },
    },
    icon: 'aws',
    iconColor: '#FF9900',
  },
  {
    name: 'AWS Bedrock (API Key)',
    websiteUrl: 'https://aws.amazon.com/bedrock/',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: 'https://bedrock-runtime.${AWS_REGION}.amazonaws.com',
        AWS_REGION: '${AWS_REGION}',
        ANTHROPIC_MODEL: 'global.anthropic.claude-opus-4-7',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'global.anthropic.claude-haiku-4-5-20251001-v1:0',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'global.anthropic.claude-sonnet-4-6',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'global.anthropic.claude-opus-4-7',
        CLAUDE_CODE_USE_BEDROCK: '1',
      },
    },
    category: 'cloud_provider',
    templateValues: {
      AWS_REGION: {
        label: 'AWS Region',
        placeholder: 'us-west-2',
        editorValue: 'us-west-2',
      },
    },
    icon: 'aws',
    iconColor: '#FF9900',
  },
  {
    name: 'Custom Provider',
    nameKey: 'customProvider',
    websiteUrl: '',
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: '',
        ANTHROPIC_AUTH_TOKEN: '',
      },
    },
    category: 'custom',
    icon: 'custom',
    iconColor: '#6366F1',
  },
]

/** 获取分类标签 */
export function getCategoryLabel(category?: ProviderCategory): string {
  if (!category) return ''
  // These labels are now handled via i18n in ProviderPresetPanel
  // This function returns a fallback for non-i18n contexts
  const labels: Record<ProviderCategory, string> = {
    official: 'Official',
    cn_official: 'CN',
    cloud_provider: 'Cloud',
    aggregator: 'Aggregator',
    third_party: 'Third-party',
    custom: 'Custom',
    omo: 'OMO',
    'omo-slim': 'OMO Slim',
  }
  return labels[category] || category
}

/** 按分类筛选预设 */
export function filterPresetsByCategory(presets: ProviderPreset[], category?: ProviderCategory): ProviderPreset[] {
  if (!category) return presets.filter(p => !p.hidden)
  return presets.filter(p => p.category === category && !p.hidden)
}