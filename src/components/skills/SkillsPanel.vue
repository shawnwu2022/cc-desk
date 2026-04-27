<template>
  <div class="skills-panel">
    <!-- Header -->
    <PanelHeader title="Skills" @close="$emit('close')">
      <template #actions>
        <button class="action-btn" @click="handleRefresh" title="Refresh skills">
          <img src="@/assets/icons/refresh.svg" alt="Refresh" />
        </button>
      </template>
    </PanelHeader>

    <!-- Loading -->
    <div v-if="loading" class="loading-state">
      <span class="loading-text">Loading skills...</span>
    </div>

    <!-- Error -->
    <div v-else-if="error" class="error-state">
      <span class="error-text">{{ error }}</span>
    </div>

    <!-- Empty -->
    <div v-else-if="allSkills.length === 0" class="empty-state">
      <span class="empty-text">No skills available</span>
      <span class="empty-hint">Add skills to ~/.claude/skills/ or .claude/skills/</span>
    </div>

    <!-- Skills List (按顺序: Project -> User -> Plugin) -->
    <div v-else class="skills-list">
      <!-- Project Skills -->
      <SkillGroup
        v-if="projectSkills.length > 0"
        title="Project Skills"
        :expanded="sidebarStore.skillsExpandedGroups.project"
        :count="projectSkills.length"
        :skills="projectSkills"
        @toggle="sidebarStore.toggleSkillGroup('project')"
      />

      <!-- User Skills -->
      <SkillGroup
        v-if="userSkills.length > 0"
        title="User Skills"
        :expanded="sidebarStore.skillsExpandedGroups.user"
        :count="userSkills.length"
        :skills="userSkills"
        @toggle="sidebarStore.toggleSkillGroup('user')"
      />

      <!-- Plugin Skills -->
      <SkillGroup
        v-if="pluginSkills.length > 0"
        title="Plugin Skills"
        :expanded="sidebarStore.skillsExpandedGroups.plugin"
        :count="pluginSkills.length"
        :skills="pluginSkills"
        @toggle="sidebarStore.toggleSkillGroup('plugin')"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useSidebarStore } from '@/stores/sidebar'
import { useAppStore } from '@/stores/app'
import { getAllSkills } from '@/api/tauri'
import type { SkillInfo } from '@/types'
import SkillGroup from './SkillGroup.vue'
import PanelHeader from '../sidebar/PanelHeader.vue'

const sidebarStore = useSidebarStore()
const appStore = useAppStore()

const skills = ref<SkillInfo[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const loadedCwd = ref<string | null>(null)

// 所有 skills
const allSkills = computed(() => skills.value)

// 按 sourceType 分组
const projectSkills = computed(() =>
  skills.value.filter(s => s.sourceType === 'project')
)

const userSkills = computed(() =>
  skills.value.filter(s => s.sourceType === 'user')
)

const pluginSkills = computed(() =>
  skills.value.filter(s => s.sourceType === 'plugin')
)

// 加载 Skills（带缓存）
async function loadSkills(projectPath: string, force = false) {
  if (!projectPath) return
  if (!force && loadedCwd.value === projectPath && skills.value.length > 0) return

  loading.value = true
  error.value = null

  try {
    const result = await getAllSkills(projectPath)
    skills.value = result
    loadedCwd.value = projectPath
  } catch (err) {
    error.value = 'Failed to load skills'
    console.error('[SkillsPanel] Failed to load skills:', err)
  } finally {
    loading.value = false
  }
}

function handleRefresh() {
  if (appStore.cwd) {
    loadSkills(appStore.cwd, true)
  }
}

onMounted(() => {
  if (appStore.cwd) {
    loadSkills(appStore.cwd)
  }
})
</script>

<style scoped>
.skills-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-secondary);
}

/* action-btn slot 样式 */
.skills-panel :deep(.action-btn) {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  padding: 0;
}

.skills-panel :deep(.action-btn img) {
  width: 16px;
  height: 16px;
}

.skills-panel :deep(.action-btn:hover) {
  color: var(--text-primary);
}

.loading-state,
.error-state,
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 24px;
  gap: 8px;
}

.loading-text,
.error-text,
.empty-text {
  font-size: 13px;
  color: var(--text-secondary);
}

.error-text {
  color: var(--error-color);
}

.empty-hint {
  font-size: 12px;
  color: var(--text-tertiary);
}

.skills-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 8px 0;
}
</style>
