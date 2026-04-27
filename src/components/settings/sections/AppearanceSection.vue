<template>
  <div class="section-content">
    <h2 class="section-heading">Appearance</h2>

    <div class="setting-group">
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">Font Size</span>
          <span class="setting-desc">Terminal font size (10–24px)</span>
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
          <span class="setting-label">Theme</span>
          <span class="setting-desc">Application color scheme</span>
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
          <span>Light</span>
        </label>
        <label class="theme-card" :class="{ active: theme === 'dark', disabled: true }">
          <input type="radio" v-model="theme" value="dark" disabled />
          <div class="theme-preview dark-preview">
            <div class="preview-bar"></div>
            <div class="preview-lines">
              <div class="preview-line"></div>
              <div class="preview-line short"></div>
            </div>
          </div>
          <span>Dark <small>(Soon)</small></span>
        </label>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useAppStore } from '@/stores/app'

const appStore = useAppStore()
const fontSize = computed(() => appStore.fontSize)
const theme = ref(appStore.theme)

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
</style>
