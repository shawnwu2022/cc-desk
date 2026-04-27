<template>
  <div
    class="session-item"
    :class="{ active: isActive, stopped: isStopped }"
    @click="handleClick"
  >
    <!-- 运行状态指示器 -->
    <span class="status-dot" :class="{ running: isRunning, stopped: isStopped }"></span>

    <!-- 会话名称 -->
    <div class="session-name-wrapper">
      <span v-if="!isRenaming" class="session-name">{{ name }}</span>
      <input
        v-else
        ref="renameInputRef"
        v-model="newName"
        class="rename-input"
        :style="{ width: inputWidth }"
        @keyup.enter="confirmRename"
        @keyup.escape="cancelRename"
        @blur="confirmRename"
        @click.stop
      />
    </div>

    <!-- 操作按钮 -->
    <template v-if="isActive || isHovered">
      <!-- 重启按钮（已停止的 Tab） -->
      <button
        v-if="isStopped"
        class="action-btn restart-btn"
        @click.stop="$emit('restart', id)"
        title="Restart"
      >
        <img src="@/assets/icons/refresh.svg" alt="Restart" />
      </button>
      <!-- 重命名按钮 -->
      <button
        v-if="isActive"
        class="action-btn rename-btn"
        @click.stop="startRename"
        title="Rename"
      >
        <img src="@/assets/icons/edit.svg" alt="Rename" />
      </button>
      <!-- 关闭按钮 -->
      <button
        v-if="closable"
        class="action-btn close-action-btn"
        @click.stop="$emit('close', id)"
        title="Close"
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="18" y1="6" x2="6" y2="18"/>
          <line x1="6" y1="6" x2="18" y2="18"/>
        </svg>
      </button>
    </template>

    <!-- 时间信息 -->
    <span class="time-info">{{ timeAgo }}</span>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, nextTick } from 'vue'

const props = defineProps<{
  id: string
  name: string
  isActive: boolean
  isRunning?: boolean
  isStopped?: boolean
  lastActiveAt: number
  closable?: boolean
}>()

const emit = defineEmits<{
  switch: [id: string]
  rename: [id: string, name: string]
  restart: [id: string]
  close: [id: string]
}>()

const isRenaming = ref(false)
const newName = ref('')
const renameInputRef = ref<HTMLInputElement>()
const inputWidth = ref('auto')
const isHovered = ref(false)

// 计算时间差
const timeAgo = computed(() => {
  const now = Date.now()
  const diff = now - props.lastActiveAt

  const minutes = Math.floor(diff / 60000)
  const hours = Math.floor(diff / 3600000)
  const days = Math.floor(diff / 86400000)

  if (minutes < 1) return 'Just now'
  if (minutes < 60) return `${minutes}m`
  if (hours < 24) return `${hours}h`
  return `${days}d`
})

function handleClick() {
  if (!isRenaming.value) {
    emit('switch', props.id)
  }
}

function startRename() {
  isRenaming.value = true
  newName.value = props.name

  const nameLen = props.name.length
  inputWidth.value = `${Math.max(50, Math.min(nameLen * 8 + 16, 180))}px`

  nextTick(() => {
    renameInputRef.value?.focus()
    renameInputRef.value?.select()
  })
}

function confirmRename() {
  if (!isRenaming.value) return
  isRenaming.value = false
  if (newName.value.trim() && newName.value !== props.name) {
    emit('rename', props.id, newName.value.trim())
  }
}

function cancelRename() {
  isRenaming.value = false
  newName.value = props.name
}
</script>

<style scoped>
.session-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 8px 12px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 6px;
  transition: background 0.15s ease;
  position: relative;
}

.session-item:hover {
  background: var(--hover-bg);
}

.session-item.active {
  background: var(--selected-bg);
}

.session-item.active::before {
  content: '';
  position: absolute;
  left: 0;
  top: 4px;
  bottom: 4px;
  width: 3px;
  background: var(--accent-color);
  border-radius: 0 2px 2px 0;
}

.session-item.active .session-name {
  color: var(--accent-color);
  font-weight: 600;
}

.session-item.stopped .session-name {
  color: var(--text-secondary);
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
  transition: all 0.2s ease;
}

.status-dot.running {
  background: #27ae60;
  box-shadow: 0 0 4px rgba(39, 174, 96, 0.5);
  animation: pulse 2s ease-in-out infinite;
}

.status-dot.stopped {
  border: 2px solid var(--text-tertiary);
  background: transparent;
}

.status-dot:not(.running):not(.stopped) {
  background: var(--text-secondary);
}

@keyframes pulse {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.7; transform: scale(0.9); }
}

.session-name-wrapper {
  flex: 1;
  min-width: 0;
}

.session-name {
  font-size: 13px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  display: block;
}

.rename-input {
  font-size: 13px;
  font-weight: 500;
  padding: 2px 6px;
  border: 1px solid var(--accent-color);
  border-radius: 3px;
  background: var(--bg-primary);
  color: var(--text-primary);
  outline: none;
}

.action-btn {
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 3px;
  opacity: 0;
  transition: opacity 0.15s ease;
  flex-shrink: 0;
}

.action-btn img {
  width: 12px;
  height: 12px;
}

.session-item:hover .action-btn {
  opacity: 1;
}

.action-btn:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.time-info {
  font-size: 11px;
  color: var(--text-tertiary);
  flex-shrink: 0;
}
</style>
