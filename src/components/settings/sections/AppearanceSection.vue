<template>
  <div class="section-content">
    <h2 class="section-heading">{{ t('appearanceTitle') }}</h2>

    <div class="setting-group">
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{{ t('fontSize') }}</span>
          <span class="setting-desc">{{ t('fontSizeDesc') }}</span>
        </div>
        <div class="font-size-control">
          <button class="size-btn" @click="decreaseFontSize" :disabled="fontSize <= 10">−</button>
          <span class="size-value">{{ fontSize }}px</span>
          <button class="size-btn" @click="increaseFontSize" :disabled="fontSize >= 24">+</button>
        </div>
      </div>
    </div>

    <div class="setting-group">
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{{ t('theme') }}</span>
          <span class="setting-desc">{{ t('themeDesc') }}</span>
        </div>
      </div>
      <div class="theme-options">
        <label class="theme-card" :class="{ active: theme === 'light' }">
          <input type="radio" v-model="theme" value="light" />
          <div class="theme-preview light-preview">
            <div class="preview-bar"></div>
            <div class="preview-lines">
              <div class="preview-line"></div>
              <div class="preview-line short"></div>
            </div>
          </div>
          <span>{{ t('light') }}</span>
        </label>
        <label class="theme-card" :class="{ active: theme === 'dark' }">
          <input type="radio" v-model="theme" value="dark" />
          <div class="theme-preview dark-preview">
            <div class="preview-bar"></div>
            <div class="preview-lines">
              <div class="preview-line"></div>
              <div class="preview-line short"></div>
            </div>
          </div>
          <span>{{ t('dark') }}</span>
        </label>
      </div>
    </div>

    <div class="setting-group">
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{{ t('terminalTheme') }}</span>
          <span class="setting-desc">{{ t('terminalThemeDesc') }}</span>
        </div>
      </div>
      <div class="terminal-theme-row">
        <select
          class="terminal-theme-select"
          v-model="terminalTheme"
          :style="{ color: 'var(--text-primary)', backgroundColor: 'var(--bg-primary)' }"
        >
          <optgroup :label="t('terminalThemeBuiltin')">
            <option v-for="th in builtinThemes" :key="th.id" :value="th.id">{{ th.name }}</option>
          </optgroup>
          <optgroup :label="t('terminalThemeDark')">
            <option v-for="th in darkThemes" :key="th.id" :value="th.id">{{ th.name }}</option>
          </optgroup>
          <optgroup :label="t('terminalThemeLight')">
            <option v-for="th in lightThemes" :key="th.id" :value="th.id">{{ th.name }}</option>
          </optgroup>
        </select>
        <div class="terminal-theme-preview" :style="previewStyle">
          <div class="preview-line">$ npm test</div>
          <div class="preview-line" :style="{ color: previewColors.green }">✔ 12 passed</div>
          <div class="preview-line" :style="{ color: previewColors.yellow }">⠿ building...</div>
          <div class="preview-line" :style="{ color: previewColors.red }">✖ 1 failed</div>
        </div>
      </div>
    </div>

    <div class="setting-group">
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{{ t('terminalRenderer') }}</span>
          <span class="setting-desc">{{ t('terminalRendererDesc') }}</span>
        </div>
        <select
          class="renderer-select"
          v-model="webglRenderer"
          :style="{ color: 'var(--text-primary)', backgroundColor: 'var(--bg-primary)' }"
        >
          <option :value="false">{{ t('terminalRendererDom') }}</option>
          <option :value="true">{{ t('terminalRendererWebgl') }}</option>
        </select>
      </div>
    </div>

    <div class="setting-group">
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{{ t('language') }}</span>
          <span class="setting-desc">{{ t('languageDesc') }}</span>
        </div>
      </div>
      <div class="theme-options">
        <label class="theme-card" :class="{ active: currentLanguage === 'en' }">
          <input type="radio" v-model="currentLanguage" value="en" />
          <div class="lang-preview">
            <span class="lang-icon">EN</span>
          </div>
          <span>{{ t('languageEn') }}</span>
        </label>
        <label class="theme-card" :class="{ active: currentLanguage === 'zh' }">
          <input type="radio" v-model="currentLanguage" value="zh" />
          <div class="lang-preview">
            <span class="lang-icon">中</span>
          </div>
          <span>{{ t('languageZh') }}</span>
        </label>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import { TERMINAL_THEMES, getTerminalTheme } from '@/config/terminalThemes'

const { t } = useI18n()
const appStore = useAppStore()
const fontSize = computed(() => appStore.fontSize)
const theme = ref(appStore.theme)

// 终端主题：双向绑定到 store（set 时内部归一化）
const terminalTheme = computed({
  get: () => appStore.terminalTheme,
  set: (val: string) => appStore.setTerminalTheme(val),
})

// 渲染后端：双向绑定到 store（set 时持久化）
const webglRenderer = computed({
  get: () => appStore.webglRenderer,
  set: (val: boolean) => appStore.setWebglRenderer(val),
})

// 按 category 分组供 optgroup 使用
const builtinThemes = computed(() => TERMINAL_THEMES.filter(t => t.category === 'builtin'))
const darkThemes = computed(() => TERMINAL_THEMES.filter(t => t.category === 'dark'))
const lightThemes = computed(() => TERMINAL_THEMES.filter(t => t.category === 'light'))

// 预览配色：随选中主题实时变化（颜色取值逻辑在 getTerminalTheme，已单测）
const previewColors = computed(() => getTerminalTheme(appStore.terminalTheme))
const previewStyle = computed(() => ({
  backgroundColor: previewColors.value.background,
  color: previewColors.value.foreground,
}))

const currentLanguage = computed({
  get: () => appStore.language,
  set: (val: string) => appStore.setLanguage(val)
})

watch(theme, (val) => { appStore.setTheme(val) })

function increaseFontSize() { appStore.setFontSize(fontSize.value + 1) }
function decreaseFontSize() { appStore.setFontSize(fontSize.value - 1) }
</script>

<style scoped>
.section-content {
  padding: 8px 0;
}

.section-heading {
  font-size: 20px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 24px;
}

.setting-group {
  margin-bottom: 28px;
}

.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.setting-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.setting-label {
  font-size: 14px;
  font-weight: 500;
  color: var(--text-primary);
}

.setting-desc {
  font-size: 12px;
  color: var(--text-tertiary);
}

.font-size-control {
  display: flex;
  align-items: center;
  gap: 8px;
}

.size-btn {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 6px;
  font-size: 16px;
  transition: all 0.15s ease;
}

.size-btn:hover:not(:disabled) {
  border-color: var(--accent-color);
  color: var(--accent-color);
}

.size-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.size-value {
  font-size: 14px;
  font-weight: 500;
  color: var(--text-primary);
  min-width: 40px;
  text-align: center;
  font-variant-numeric: tabular-nums;
}

.theme-options {
  display: flex;
  gap: 12px;
  margin-top: 12px;
}

.theme-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 12px;
  border: 2px solid var(--border-color);
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.15s ease;
  width: 120px;
}

.theme-card:hover {
  border-color: var(--accent-color);
}

.theme-card.active {
  border-color: var(--accent-color);
  background: var(--accent-light);
}

.theme-card.disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.theme-card input[type="radio"] {
  display: none;
}

.theme-card span {
  font-size: 13px;
  color: var(--text-primary);
}

.theme-card small {
  color: var(--text-tertiary);
}

.theme-preview {
  width: 80px;
  height: 52px;
  border-radius: 4px;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.light-preview {
  background: #f7f8fa;
  border: 1px solid #e5e7eb;
}

.dark-preview {
  background: #1a1a2e;
  border: 1px solid #2d2d44;
}

.preview-bar {
  height: 8px;
  background: var(--accent-color);
  opacity: 0.3;
}

.preview-lines {
  padding: 6px 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.light-preview .preview-line {
  height: 3px;
  background: #d1d5db;
  border-radius: 1px;
}

.dark-preview .preview-line {
  height: 3px;
  background: #4a4a6a;
  border-radius: 1px;
}

.preview-line.short {
  width: 60%;
}

.lang-preview {
  width: 80px;
  height: 52px;
  border-radius: 4px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  display: flex;
  align-items: center;
  justify-content: center;
}

.lang-icon {
  font-size: 20px;
  font-weight: 700;
  color: var(--accent-color);
}

.terminal-theme-row {
  display: flex;
  gap: 12px;
  margin-top: 12px;
  align-items: stretch;
}

.terminal-theme-select {
  flex: 0 0 200px;
  padding: 8px 10px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
  /* 自定义箭头（最小美化） */
  appearance: none;
  background-image: none;
}

.terminal-theme-select:focus {
  border-color: var(--accent-color);
}

.terminal-theme-preview {
  flex: 1;
  min-width: 0;
  padding: 10px 12px;
  border-radius: 6px;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.6;
  border: 1px solid var(--border-color);
  overflow: hidden;
}

.preview-line {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.renderer-select {
  padding: 8px 10px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
  appearance: none;
  background-image: none;
  min-width: 140px;
}

.renderer-select:focus {
  border-color: var(--accent-color);
}
</style>
