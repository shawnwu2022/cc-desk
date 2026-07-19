<template>
  <div class="settings-view">
    <!-- 左侧导航 -->
    <nav class="settings-nav">
      <div class="settings-nav-header">
        <span class="nav-title">{{ t('settings') }}</span>
        <button class="close-btn" @click="$emit('close')">
          <img src="@/assets/icons/close.svg" :alt="t('close')" />
        </button>
      </div>
      <div class="nav-items">
        <button
          v-for="item in navItems"
          :key="item.id"
          class="nav-item"
          :class="{ active: sidebarStore.activeSettingsSection === item.id }"
          @click="sidebarStore.activeSettingsSection = item.id"
        >
          <span class="nav-label">{{ item.label }}</span>
          <span v-if="item.id === 'update' && sidebarStore.updateAvailable" class="nav-badge"></span>
        </button>
      </div>
      <div class="nav-footer">
        <span class="footer-version">CC Desk v{{ currentVersion }}</span>
      </div>
    </nav>

    <!-- 右侧内容 -->
    <div class="settings-content">
      <AppearanceSection v-if="sidebarStore.activeSettingsSection === 'appearance'" />
      <StartupSection v-if="sidebarStore.activeSettingsSection === 'startup'" />
      <ProvidersSection v-if="sidebarStore.activeSettingsSection === 'providers'" />
      <ShortcutsSection v-if="sidebarStore.activeSettingsSection === 'shortcuts'" />
      <UpdateSection v-if="sidebarStore.activeSettingsSection === 'update'" />
      <AboutSection v-if="sidebarStore.activeSettingsSection === 'about'" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useSidebarStore } from '@/stores/sidebar'
import AppearanceSection from './sections/AppearanceSection.vue'
import StartupSection from './sections/StartupSection.vue'
import ProvidersSection from './sections/ProvidersSection.vue'
import ShortcutsSection from './sections/ShortcutsSection.vue'
import UpdateSection from './sections/UpdateSection.vue'
import AboutSection from './sections/AboutSection.vue'

const { t } = useI18n()
const currentVersion = __APP_VERSION__

defineEmits<{ close: [] }>()

const sidebarStore = useSidebarStore()

const navItems = computed(() => [
  { id: 'appearance', label: t('appearance') },
  { id: 'startup', label: t('startup') },
  { id: 'providers', label: t('providers') },
  { id: 'shortcuts', label: t('shortcuts') },
  { id: 'update', label: t('update') },
  { id: 'about', label: t('about') },
])
</script>

<style scoped>
.settings-view {
  display: flex;
  flex: 1;
  height: 100%;
  background: var(--bg-primary);
}

/* 左侧导航 */
.settings-nav {
  width: 180px;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-color);
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
}

.settings-nav-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 14px;
  border-bottom: 1px solid var(--border-color);
}

.nav-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.close-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 4px;
}

.close-btn img {
  width: 16px;
  height: 16px;
}

.close-btn:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.nav-items {
  flex: 1;
  padding: 8px 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.nav-item {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  cursor: pointer;
  border-radius: 6px;
  text-align: left;
  transition: all 0.15s ease;
}

.nav-item:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.nav-item.active {
  background: var(--selected-bg);
  color: var(--accent-color);
  font-weight: 500;
}

.nav-item.active::before {
  content: '';
  position: absolute;
  left: -8px;
  top: 6px;
  bottom: 6px;
  width: 3px;
  background: var(--accent-color);
  border-radius: 0 2px 2px 0;
}

.nav-badge {
  width: 8px;
  height: 8px;
  background: var(--error-color);
  border-radius: 50%;
  flex-shrink: 0;
}

.nav-footer {
  padding: 12px 14px;
  border-top: 1px solid var(--border-color);
}

.footer-version {
  font-size: 11px;
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}

/* 右侧内容 */
.settings-content {
  flex: 1;
  overflow-y: auto;
  padding: 28px 36px;
}
</style>
