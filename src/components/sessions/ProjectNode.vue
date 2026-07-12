<template>
  <div class="project-node" :class="{ current: isCurrent }">
    <!-- 项目行 -->
    <div class="project-row">
      <!-- ▸ 展开箭头：独立命中区，只触发展开（对抗审查 F） -->
      <button
        class="expand-arrow"
        :class="{ expanded: expanded }"
        @click.stop="$emit('toggleExpand', project.projectPath)"
        :title="expanded ? t('collapse') : t('expand')"
      >
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
          <polyline points="9 6 15 12 9 18" />
        </svg>
      </button>

      <!-- 项目名区：点击切最近会话 -->
      <div class="project-main" @click="$emit('switchToProject', project.projectPath)">
        <span class="project-name">{{ project.name }}</span>
        <!-- 状态徽标：●N 运行 / 琥珀点 pending -->
        <span v-if="project.runningCount > 0" class="badge running">●{{ project.runningCount }}</span>
        <span v-else-if="project.pendingCount > 0" class="badge pending" :title="t('pendingHint')">●</span>
        <span v-if="project.isOrphan" class="orphan-tag">{{ t('uncollected') }}</span>
      </div>

      <!-- hover 操作 -->
      <div class="hover-actions">
        <button class="action-btn" @click.stop="$emit('newSessionIn', project.projectPath)" :title="t('newSessionTitle')">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
          </svg>
        </button>
        <button class="action-btn" @click.stop="menuOpen = !menuOpen" :title="t('more')">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="5" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="12" cy="19" r="1" />
          </svg>
        </button>
      </div>

      <!-- ⋯ 菜单 -->
      <div v-if="menuOpen" class="menu" @click.stop>
        <button @click="onMenu('closeAll')">{{ t('closeAllSessions') }}</button>
        <button @click="onMenu('openInExplorer')">{{ t('openFolder') }}</button>
        <!-- 仅孤儿项目显示「加入收藏」；非孤儿不暴露「取消收藏」（后端零改动不支持移除） -->
        <button v-if="project.isOrphan" @click="onMenu('toggleFavorite')">{{ t('addFavorite') }}</button>
      </div>
    </div>

    <!-- 会话列表（展开时） -->
    <div v-if="expanded" class="session-sub">
      <SessionList
        :tabs="project.tabs"
        :history="history"
        :active-id="activeTabId"
        :closable="true"
        @switch="onSessionSwitch"
        @rename="(id, name) => emit('renameSession', id, name)"
        @restart="(id) => emit('restartSession', id)"
        @close="(id) => emit('closeTab', id)"
      />
      <div v-if="project.tabs.length === 0 && history.length === 0 && !loading" class="empty-hint">
        {{ t('noHistorySessions') }}
      </div>
      <div v-if="loading" class="loading-indicator">{{ t('loading') }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import SessionList from './SessionList.vue'
import type { ProjectGroup, HistorySession } from '@/stores/session'

const { t } = useI18n()
const props = defineProps<{
  project: ProjectGroup
  expanded: boolean
  isCurrent: boolean
  activeTabId: string | null
  history: HistorySession[]
  loading?: boolean
  matchedHistoryIds?: string[]
}>()

// <script setup> 中 defineEmits 只能调用一次；用 const 取得 emit 函数
const emit = defineEmits<{
  switchToProject: [projectPath: string]
  toggleExpand: [projectPath: string]
  newSessionIn: [projectPath: string]
  switchSession: [tabId: string]
  renameSession: [tabId: string, name: string]
  restartSession: [tabId: string]
  closeTab: [tabId: string]
  resumeSession: [sessionId: string, name?: string]
  closeAllSessions: [projectPath: string]
  toggleFavorite: [projectPath: string]
  openInExplorer: [projectPath: string]
}>()

const menuOpen = ref(false)

function onMenu(action: 'closeAll' | 'openInExplorer' | 'toggleFavorite') {
  menuOpen.value = false
  const path = props.project.projectPath
  if (action === 'closeAll') emit('closeAllSessions', path)
  else if (action === 'openInExplorer') emit('openInExplorer', path)
  else emit('toggleFavorite', path)
}

function resumeName(sessionId: string): string | undefined {
  return props.history.find(s => s.sessionId === sessionId)?.name
}

/**
 * SessionList 统一 emit switch(id)；区分 id 是 tabId（在 tabs 里）还是 sessionId（历史）：
 * tab → switchSession；历史 → resumeSession（带 name）。
 */
function onSessionSwitch(id: string) {
  if (props.project.tabs.some(t => t.tabId === id)) {
    emit('switchSession', id)
  } else {
    emit('resumeSession', id, resumeName(id))
  }
}
</script>

<style scoped>
.project-node { position: relative; }
.project-row {
  display: flex; align-items: center; gap: 4px;
  padding: 4px 4px 4px 0; border-radius: 6px;
}
.project-row:hover { background: var(--hover-bg); }
.project-node.current .project-name { color: var(--accent-color); font-weight: 600; }
.expand-arrow {
  width: 18px; height: 18px; flex-shrink: 0;
  display: flex; align-items: center; justify-content: center;
  border: none; background: transparent; color: var(--text-tertiary);
  cursor: pointer; transition: transform 0.15s ease;
}
.expand-arrow.expanded { transform: rotate(90deg); }
.project-main {
  flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px;
  cursor: pointer; padding: 4px 6px;
}
.project-name {
  font-size: 13px; font-weight: 500; color: var(--text-primary);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.badge { font-size: 11px; flex-shrink: 0; }
.badge.running { color: var(--status-success); }
.badge.pending { color: var(--accent-gold); animation: status-pulse 2s ease-in-out infinite; }
.orphan-tag {
  font-size: 10px; color: var(--text-tertiary);
  border: 1px solid var(--border-color); border-radius: 3px; padding: 0 4px;
}
.hover-actions { display: flex; gap: 2px; opacity: 0; transition: opacity 0.15s ease; }
.project-row:hover .hover-actions { opacity: 1; }
.action-btn {
  width: 20px; height: 20px; border: none; background: transparent;
  color: var(--text-secondary); cursor: pointer; border-radius: 3px;
  display: flex; align-items: center; justify-content: center;
}
.action-btn:hover { background: var(--bg-secondary); color: var(--text-primary); }
.menu {
  position: absolute; right: 8px; top: 28px; z-index: 10;
  background: var(--bg-primary); border: 1px solid var(--border-color);
  border-radius: 6px; box-shadow: var(--shadow-md); padding: 4px; min-width: 140px;
}
.menu button {
  display: block; width: 100%; text-align: left; padding: 6px 8px;
  border: none; background: transparent; color: var(--text-primary);
  cursor: pointer; font-size: 12px; border-radius: 4px;
}
.menu button:hover { background: var(--hover-bg); }
.session-sub { padding-left: 22px; }
.empty-hint, .loading-indicator { padding: 8px; font-size: 11px; color: var(--text-tertiary); text-align: center; }
@keyframes status-pulse { 0%,100% { opacity: 1; } 50% { opacity: 0.6; } }
</style>
