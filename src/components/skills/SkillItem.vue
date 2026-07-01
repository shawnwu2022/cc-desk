<template>
  <div class="skill-item" :class="{ expanded: isExpanded }">
    <!-- Skill Header -->
    <div class="skill-header" @click="toggleExpand">
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
      <button class="use-btn" @click.stop="emitUseSkill" :title="t('useThisSkill')">
        <img src="@/assets/icons/use.svg" :alt="t('useBtn')" />
      </button>
    </div>

    <!-- Skill Details (expanded) -->
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
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SkillInfo } from '@/types'
import { sendTerminalCommand } from '@/composables/useTerminalCommand'

const { t } = useI18n()
const props = defineProps<{
  skill: SkillInfo
}>()

const isExpanded = ref(false)

function toggleExpand() {
  isExpanded.value = !isExpanded.value
}

function emitUseSkill() {
  sendTerminalCommand(props.skill.invokeFormat)
}
</script>

<style scoped>
.skill-item {
  background: var(--bg-primary);
  border-radius: 8px;
  padding: 10px 12px;
  transition: background 0.15s ease;
}

.skill-item:hover {
  background: var(--hover-bg);
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
  color: var(--text-secondary);
  flex-shrink: 0;
  transition: transform 0.15s ease;
}

.expand-icon.expanded {
  transform: rotate(90deg);
}

.skill-info {
  flex: 1;
  min-width: 0;
}

.skill-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
}

.skill-full-name {
  display: block;
  font-size: 11px;
  color: var(--text-tertiary);
  font-family: var(--font-mono);
  margin-top: 2px;
}

.use-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  border-radius: 4px;
  flex-shrink: 0;
}

.use-btn img {
  width: 14px;
  height: 14px;
}

.use-btn:hover {
  color: var(--accent-color);
  background: var(--bg-tertiary);
}

.skill-details {
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--border-color);
}

.skill-description-full {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.5;
  white-space: pre-wrap;
  overflow-wrap: break-word;
  word-break: break-word;
}

.skill-description-empty {
  font-size: 12px;
  color: var(--text-tertiary);
  font-style: italic;
}

.skill-invoke-format {
  margin-top: 8px;
  display: flex;
  gap: 6px;
  font-size: 11px;
}

.invoke-label {
  color: var(--text-tertiary);
}

.invoke-value {
  font-family: var(--font-mono);
  color: var(--text-primary);
  background: var(--bg-tertiary);
  padding: 2px 6px;
  border-radius: 4px;
}
</style>