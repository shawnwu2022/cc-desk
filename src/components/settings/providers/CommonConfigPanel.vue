<template>
  <div class="common-panel">
    <div class="panel-header">
      <button class="back-btn" @click="$emit('close')">&#8592; {{ t('back') }}</button>
      <span class="panel-title">{{ t('commonConfigTitle') }}</span>
    </div>

    <div class="panel-body">
      <div class="config-info">
        <p class="info-desc">
          {{ t('commonConfigDesc') }}
        </p>
      </div>

      <div class="json-toolbar">
        <span class="toolbar-label">{{ t('configContentJson') }}</span>
        <div class="toolbar-actions">
          <button class="btn-extract" @click="extractFromSource" v-if="sourceJson">{{ t('extractFromEdit') }}</button>
          <button class="btn-format" @click="formatJson">{{ t('format') }}</button>
        </div>
      </div>

      <div class="json-editor-wrap" :class="{ 'has-error': jsonError }">
        <codemirror
          v-model="jsonContent"
          :style="{ height: 'auto' }"
          :extensions="cmExtensions"
          @change="onJsonChange"
        />
      </div>
      <p class="json-status" v-if="jsonError">{{ jsonError }}</p>
    </div>

    <div class="panel-footer">
      <button class="btn-cancel" @click="$emit('close')">{{ t('cancel') }}</button>
      <button class="btn-save" @click="handleSave" :disabled="!!jsonError">{{ t('save') }}</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { Codemirror } from 'vue-codemirror'
import { json, jsonParseLinter } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'
import { linter } from '@codemirror/lint'
import { EditorView } from '@codemirror/view'
import { useAppStore } from '@/stores/app'
import type { CommonConfig } from '@/types/provider'

const { t } = useI18n()
const appStore = useAppStore()

const props = defineProps<{
  config: CommonConfig
  initialSettings?: Record<string, any> | null
  sourceJson?: Record<string, any> | null
}>()

const emit = defineEmits<{
  close: []
  save: [settings: Record<string, any>]
}>()

const cmExtensions = computed(() => {
  const exts: any[] = [
    json(),
    linter(jsonParseLinter()),
    EditorView.theme({
      '&': { fontSize: '14px', fontFamily: 'var(--font-mono)' },
      '.cm-content': { fontFamily: 'var(--font-mono)' },
      '.cm-gutters': { fontFamily: 'var(--font-mono)' },
    }),
  ]
  // 使用应用主题而非系统偏好
  if (appStore.theme === 'dark') {
    exts.push(oneDark)
  }
  return exts
})

const jsonContent = ref('')
const jsonError = ref('')

onMounted(() => {
  const source = props.initialSettings && Object.keys(props.initialSettings).length > 0
    ? props.initialSettings
    : props.config.settings
  jsonContent.value = Object.keys(source).length > 0
    ? JSON.stringify(source, null, 2)
    : '{\n  \n}'
})

function onJsonChange(value: string) {
  jsonContent.value = value
  validateJson()
}

function validateJson() {
  try {
    JSON.parse(jsonContent.value)
    jsonError.value = ''
  } catch (e: any) {
    jsonError.value = e.message || t('jsonFormatError')
  }
}

function formatJson() {
  try {
    const parsed = JSON.parse(jsonContent.value)
    jsonContent.value = JSON.stringify(parsed, null, 2)
    jsonError.value = ''
  } catch { /* keep */ }
}

function extractFromSource() {
  if (!props.sourceJson) return
  const cleaned = { ...props.sourceJson }
  delete (cleaned as any).env
  if ('model' in cleaned && typeof cleaned.model === 'string') {
    const defaults = ['claude-sonnet-4-6', 'claude-haiku-4-5-20251001', 'claude-opus-4-7']
    if (defaults.includes(cleaned.model as string)) {
      delete (cleaned as any).model
    }
  }
  jsonContent.value = Object.keys(cleaned).length > 0
    ? JSON.stringify(cleaned, null, 2)
    : '{\n  \n}'
  jsonError.value = ''
}

function handleSave() {
  if (jsonError.value) return
  try {
    const settings = JSON.parse(jsonContent.value)
    emit('save', settings)
  } catch {
    jsonError.value = t('jsonParseError')
  }
}
</script>

<style scoped>
.common-panel {
  position: absolute;
  inset: 0;
  background: var(--bg-primary);
  z-index: 200;
  display: flex;
  flex-direction: column;
}

.panel-header {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 14px 20px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}
.back-btn {
  padding: 6px 14px;
  background: transparent;
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
  font-family: var(--font-sans);
}
.back-btn:hover { background: var(--hover-bg); }
.panel-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
}

.panel-body {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow-y: auto;
  padding-bottom: 20px;
}

.config-info {
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}
.info-desc {
  font-size: 13px;
  color: var(--text-secondary);
  margin: 0;
  line-height: 1.5;
}

.json-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 16px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}
.toolbar-label { font-size: 13px; color: var(--text-secondary); }
.toolbar-actions { display: flex; gap: 8px; }
.btn-format {
  padding: 4px 12px;
  background: var(--bg-secondary);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  font-family: var(--font-sans);
}
.btn-format:hover { background: var(--hover-bg); }
.btn-extract {
  padding: 4px 12px;
  background: transparent;
  color: var(--accent-color);
  border: 1px solid var(--accent-color);
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  font-family: var(--font-sans);
}
.btn-extract:hover { background: var(--accent-color); color: #fff; }

.json-editor-wrap {
  flex-shrink: 0;
  overflow: hidden;
  border-bottom: 2px solid transparent;
}
.json-editor-wrap.has-error { border-bottom-color: var(--error-color, #c0392b); }

.json-status {
  font-size: 11px;
  padding: 4px 16px;
  color: var(--error-color, #c0392b);
  flex-shrink: 0;
}

.panel-footer {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  padding: 14px 20px;
  border-top: 1px solid var(--border-color);
  flex-shrink: 0;
}
.btn-cancel {
  padding: 8px 20px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
  font-family: var(--font-sans);
}
.btn-cancel:hover { background: var(--hover-bg); }
.btn-save {
  padding: 8px 20px;
  background: var(--accent-primary);
  color: #fff;
  border: none;
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
  font-family: var(--font-sans);
}
.btn-save:hover { opacity: 0.9; }
.btn-save:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
