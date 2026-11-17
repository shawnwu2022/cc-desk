<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import { parseBrowserPairingConfiguration } from '../../../remote/ui-terminal/src/browserPairingProfile'
import { parseJson } from '../../../remote/ui-terminal/src/wire'

const props = defineProps<{ scopeJson: string; disabled?: boolean }>()
const draft = ref(''), preview = ref(''), message = ref(''), copying = ref(false)
let revision = 0, disposed = false
function invalidate() { revision++; preview.value = ''; message.value = '' }
watch(draft, invalidate, { flush: 'sync' })
watch(() => props.scopeJson, invalidate, { flush: 'sync' })
onBeforeUnmount(() => { disposed = true; invalidate() })
function prepare(replaceScope: boolean) {
  if (props.disabled || disposed || (replaceScope && !props.scopeJson)) return
  invalidate()
  try {
    const encode = (text: string) => Uint8Array.from(new TextEncoder().encode(text))
    const original = parseBrowserPairingConfiguration(encode(draft.value))
    // scope 只生成另一份未授权候选；独立批准记录与原 handles 不在此组件内。
    const candidate = replaceScope ? { ...original, grant: { ...original.grant, scope: parseJson(encode(props.scopeJson), 1024) } } : original
    const text = JSON.stringify(candidate, null, 2)
    parseBrowserPairingConfiguration(encode(text))
    preview.value = text
    message.value = replaceScope ? '已生成当前会话候选，尚未批准。其他字段未改变；仍须独立核对可信配对记录。' : '公开格式预检通过，尚未批准；不证明身份、密钥绑定或连接可用。'
  } catch { message.value = '公开候选格式无效，请核对16KiB限制、字段与编码；未生成或发送配置。' }
}
async function copy() {
  if (props.disabled || disposed || copying.value || !preview.value) return
  const token = revision, text = preview.value
  copying.value = true
  try {
    await writeText(text)
    if (!disposed && token === revision && preview.value === text) message.value = '已复制未授权候选；不会自动安装、批准或连接。'
  } catch {
    if (!disposed && token === revision) message.value = '复制未确认，请手动选择输出文本；不会自动重试。'
  } finally { if (!disposed) copying.value = false }
}
</script>
<template>
  <details class="browser-candidate-editor" data-browser-candidate-editor>
    <summary>准备 Browser 公开候选（可选）</summary>
    <p>仅离线预检。此处不是 Host 配置，也不创建 Browser 身份或可信批准记录；不要填写私钥、密码或令牌。</p>
    <label>Browser 公开候选 JSON
      <textarea v-model="draft" aria-label="Browser公开候选JSON" :disabled="disabled" maxlength="16384" spellcheck="false" autocomplete="off" />
    </label>
    <div class="candidate-actions">
      <AppButton :disabled="disabled || !draft" @click="prepare(false)">预检Browser候选</AppButton>
      <AppButton :disabled="disabled || !draft || !scopeJson" @click="prepare(true)">用当前会话范围生成候选</AppButton>
    </div>
    <p>读取公开会话范围不会修改候选。更换会话范围需显式生成，并由独立可信供应方重新核对；旧连接不会续期。</p>
    <label v-if="preview">待独立批准的候选
      <textarea :value="preview" aria-label="待独立批准的Browser候选JSON" readonly spellcheck="false" />
    </label>
    <AppButton v-if="preview" :disabled="disabled || copying" @click="copy">复制未授权候选</AppButton>
    <InlineNotice v-if="message" :message="message" />
  </details>
</template>
<style scoped>
.browser-candidate-editor { margin: 12px 0; padding: 8px; border: 1px solid var(--border-color); border-radius: var(--radius-md); min-width: 0; }
summary { cursor: pointer; font-size: 12px; color: var(--text-primary); }
p, label { font-size: 12px; line-height: 1.5; color: var(--text-secondary); }
textarea { display: block; width: 100%; min-height: 100px; box-sizing: border-box; color: var(--text-primary); background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: var(--radius-sm); padding: 6px; resize: vertical; }
.candidate-actions { display: flex; flex-wrap: wrap; gap: 6px; margin: 8px 0; }
.candidate-actions :deep(button) { white-space: normal; }
</style>
