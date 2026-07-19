<template>
  <div class="attention-panel">
    <!-- Header -->
    <PanelHeader :title="t('attention')" @close="$emit('close')" />

    <!-- 焦点队列：错误 > 等权限 > 新完成 -->
    <div class="panel-content">
      <div v-if="queue.length === 0" class="empty-hint">{{ t('attentionEmpty') }}</div>
      <button
        v-for="item in queue"
        :key="item.ptyId"
        class="attention-item"
        :class="`kind-${item.kind}`"
        @click="handleClick(item)"
        :title="t(`attentionKind_${item.kind}`)"
      >
        <span class="kind-dot"></span>
        <span class="item-name">{{ nameFor(item) }}</span>
        <span class="item-kind">{{ t(`attentionKind_${item.kind}`) }}</span>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAttentionStore } from '@/stores/attention'
import { useSessionStore } from '@/stores/session'
import PanelHeader from '../sidebar/PanelHeader.vue'
import type { AttentionItem } from '@/composables/useAttentionQueue'

const { t } = useI18n()
const attentionStore = useAttentionStore()
const sessionStore = useSessionStore()

const emit = defineEmits<{
  close: []
  switchSession: [tabId: string]
}>()

// 焦点队列（store 已按 错误>等权限>新完成 排序 + 去重）
const queue = computed(() => attentionStore.queue)

// 会话名：优先 tab 名，回退 sessionId / ptyId 前缀
function nameFor(item: AttentionItem): string {
  const tab = sessionStore.getTabByPtyId(item.ptyId)
  return tab?.name ?? item.sessionId?.slice(0, 8) ?? item.ptyId.slice(0, 8)
}

// 点击 = 跳转到该会话 + 确认关注（用户主动点开即视为已关注）
function handleClick(item: AttentionItem) {
  const tab = sessionStore.getTabByPtyId(item.ptyId)
  if (tab) {
    emit('switchSession', tab.tabId)
  }
  // tab 已不在（被关）时也清除残留关注，避免焦点队列堆积幽灵项
  attentionStore.ackPty(item.ptyId)
}
</script>

<style scoped>
.attention-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.panel-content {
  flex: 1;
  overflow-y: auto;
  padding: 8px 12px;
  min-height: 0;
}

.empty-hint {
  text-align: center;
  padding: 24px 12px;
  font-size: 12px;
  color: var(--text-secondary);
}

.attention-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 8px 10px;
  margin-bottom: 4px;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 6px;
  font-size: 13px;
  text-align: left;
  transition: all 0.15s ease;
}

.attention-item:hover {
  border-color: var(--accent-color);
  background: var(--hover-bg);
}

.kind-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.attention-item.kind-error .kind-dot {
  background: var(--status-error);
}

.attention-item.kind-permission .kind-dot {
  background: var(--accent-gold);
}

.attention-item.kind-completed .kind-dot {
  background: var(--status-success);
}

.item-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.item-kind {
  font-size: 11px;
  color: var(--text-secondary);
  flex-shrink: 0;
}
</style>
