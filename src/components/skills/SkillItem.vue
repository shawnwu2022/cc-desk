<template>
  <div class="skill-item" :class="{ expanded: isExpanded, disabled: isDisabled }">
    <div
      class="skill-header"
      role="button"
      tabindex="0"
      @click="toggleExpand"
      @keydown.enter.self.prevent="toggleExpand"
      @keydown.space.self.prevent="toggleExpand"
    >
      <img
        class="expand-icon"
        :class="{ expanded: isExpanded }"
        src="@/assets/icons/chevron.svg"
        alt="Toggle"
      />
      <div class="skill-info">
        <span class="skill-name">{{ skill.displayName }}</span>
        <span v-if="skill.sourceType === 'plugin'" class="skill-full-name">{{ skill.name }}</span>
      </div>
      <button
        class="use-btn"
        :disabled="isDisabled"
        :title="t('useThisSkill')"
        @click.stop="emitUseSkill"
      >
        <img src="@/assets/icons/use.svg" :alt="t('useBtn')" />
      </button>
    </div>

    <div v-if="isExpanded" class="skill-details">
      <div v-if="skill.description" class="skill-description-full">
        {{ skill.description }}
      </div>
      <div v-else class="skill-description-empty">
        {{ t('noDescription') }}
      </div>
      <div class="skill-invoke-format">
        <span class="invoke-label">Invoke:</span>
        <span class="invoke-value">{{ skill.invokeFormat }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SkillInfo } from '@/types'
import { sendTerminalCommand } from '@/composables/useTerminalCommand'

const { t } = useI18n()
const props = defineProps<{
  skill: SkillInfo
}>()

const isExpanded = ref(false)
const isDisabled = computed(() => props.skill.enabled === false)

function toggleExpand() {
  isExpanded.value = !isExpanded.value
}

function emitUseSkill() {
  if (isDisabled.value) return
  sendTerminalCommand(props.skill.invokeFormat)
}
</script>

<style scoped>
.skill-item {
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--bg-primary);
  transition: background 0.15s ease;
}

.skill-item:hover {
  background: var(--hover-bg);
}

.skill-item.disabled {
  opacity: 0.55;
}

.skill-item.disabled .skill-name {
  color: var(--text-tertiary);
  text-decoration: line-through;
}

.skill-header {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  user-select: none;
}

.expand-icon {
  width: 12px;
  height: 12px;
  flex-shrink: 0;
  color: var(--text-secondary);
  transition: transform 0.15s ease;
}

.expand-icon.expanded {
  transform: rotate(90deg);
}

.skill-info {
  min-width: 0;
  flex: 1;
}

.skill-name {
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 500;
}

.skill-full-name {
  display: block;
  margin-top: 2px;
  color: var(--text-tertiary);
  font-family: var(--font-mono);
  font-size: 11px;
}

.use-btn {
  display: flex;
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
}

.use-btn:disabled {
  cursor: default;
  opacity: 0.4;
}

.use-btn:not(:disabled):hover {
  background: var(--bg-tertiary);
  color: var(--accent-color);
}

.use-btn img {
  width: 14px;
  height: 14px;
}

.skill-details {
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--border-color);
}

.skill-description-full {
  overflow-wrap: break-word;
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
}

.skill-description-empty {
  color: var(--text-tertiary);
  font-size: 12px;
  font-style: italic;
}

.skill-invoke-format {
  display: flex;
  gap: 6px;
  margin-top: 8px;
  font-size: 11px;
}

.invoke-label {
  color: var(--text-tertiary);
}

.invoke-value {
  padding: 2px 6px;
  border-radius: 4px;
  background: var(--bg-tertiary);
  color: var(--text-primary);
  font-family: var(--font-mono);
}
</style>
