<template>
  <div class="project-select-view">
    <!-- 左侧：项目详情面板 -->
    <aside class="detail-panel">
      <div class="panel-inner">
        <!-- 无选中状态：显示打开新项目和默认配置 -->
        <div v-if="!selectedProject" class="default-panel">
          <header class="panel-header">
            <h2>Quick Start</h2>
          </header>

          <div class="panel-body">
            <!-- 打开新项目 -->
            <section class="action-section">
              <button class="new-project-btn" @click="$emit('addProject')">
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
                  <line x1="12" y1="11" x2="12" y2="17"/>
                  <line x1="9" y1="14" x2="15" y2="14"/>
                </svg>
                <span>Open New Project</span>
              </button>
            </section>

            <!-- 默认启动配置 -->
            <section class="config-section">
              <h3>Default Startup Options</h3>
              <p class="config-hint">These options apply when launching a project</p>

              <label class="option-item">
                <input type="checkbox" v-model="localOptions.continue" />
                <span class="option-label">Continue last session</span>
                <code class="option-flag">--continue</code>
              </label>

              <label class="option-item">
                <input type="checkbox" v-model="localOptions.skipPermissions" />
                <span class="option-label">Allow</span>
                <code class="option-flag warning">--skip-permissions</code>
              </label>

              <div class="option-item text-option">
                <span class="option-label">Custom args</span>
                <input type="text" v-model="localOptions.customArgs" placeholder="--model sonnet" />
              </div>

              <button
                class="save-default-btn"
                :class="{ saving: isSaving, success: saveSuccess }"
                :disabled="isSaving"
                @click="handleSaveDefault"
              >
                <span v-if="isSaving">Saving...</span>
                <span v-else-if="saveSuccess">Saved!</span>
                <span v-else>Save as Default</span>
              </button>
            </section>
          </div>
        </div>

        <!-- 项目详情 -->
        <div v-else class="project-detail">
          <header class="detail-header">
            <div class="project-title">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
              </svg>
              <h2>{{ selectedProject.name }}</h2>
            </div>
            <button class="close-btn" @click="selectedProject = null">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="18" y1="6" x2="6" y2="18"/>
                <line x1="6" y1="6" x2="18" y2="18"/>
              </svg>
            </button>
          </header>

          <!-- 启动按钮放在顶部 -->
          <div class="detail-launch-section">
            <button class="launch-btn" @click="handleLaunch">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <polygon points="5 3 19 12 5 21 5 3"/>
              </svg>
              <span>Launch</span>
            </button>
          </div>

          <div class="detail-body">
            <!-- 项目路径 -->
            <section class="info-section">
              <label>Path</label>
              <div class="path-display">{{ selectedProject.path }}</div>
            </section>

            <!-- 启动配置 -->
            <section class="config-section">
              <h3>Startup Options</h3>

              <label class="option-item">
                <input type="checkbox" v-model="localOptions.continue" />
                <span class="option-label">Continue last session</span>
                <code class="option-flag">--continue</code>
              </label>

              <label class="option-item">
                <input type="checkbox" v-model="localOptions.skipPermissions" />
                <span class="option-label">Allow</span>
                <code class="option-flag warning">--skip-permissions</code>
              </label>

              <div class="option-item text-option">
                <span class="option-label">Resume session</span>
                <input type="text" v-model="localOptions.resume" placeholder="session ID" />
              </div>

              <div class="option-item text-option">
                <span class="option-label">Custom args</span>
                <input type="text" v-model="localOptions.customArgs" placeholder="--model sonnet" />
              </div>

              <button
                class="save-default-btn"
                :class="{ saving: isSaving, success: saveSuccess }"
                :disabled="isSaving"
                @click="handleSaveDefault"
              >
                <span v-if="isSaving">Saving...</span>
                <span v-else-if="saveSuccess">Saved!</span>
                <span v-else>Save as Default</span>
              </button>
            </section>
          </div>
        </div>
      </div>
    </aside>

    <!-- 右侧：项目列表 -->
    <div class="projects-panel">
      <header class="panel-header">
        <h2>Projects</h2>
        <div class="search-box">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="11" cy="11" r="8"/>
            <line x1="21" y1="21" x2="16.65" y2="16.65"/>
          </svg>
          <input
            type="text"
            v-model="searchQuery"
            placeholder="Search..."
          />
        </div>
      </header>

      <div class="project-list">
        <button
          v-for="project in filteredProjects"
          :key="project.path"
          class="project-item"
          :class="{ active: selectedProject?.path === project.path }"
          @click="handleSelectProject(project)"
        >
          <svg class="folder-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
          </svg>
          <div class="project-info">
            <span class="project-name">{{ project.name }}</span>
            <span class="project-path">{{ project.path }}</span>
          </div>
        </button>

        <div v-if="filteredProjects.length === 0" class="empty-list">
          <span v-if="searchQuery">No matching projects</span>
          <span v-else>No projects yet</span>
        </div>
      </div>

      <!-- 添加项目按钮 -->
      <footer class="panel-footer">
        <button class="add-btn" @click="$emit('addProject')">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <line x1="12" y1="5" x2="12" y2="19"/>
            <line x1="5" y1="12" x2="19" y2="12"/>
          </svg>
          <span>Add Project</span>
        </button>
      </footer>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useAppStore } from '@/stores/app'
import { getProjects, getProjectInfo } from '@/api/tauri'
import type { Project } from '@/api/tauri'

const emit = defineEmits<{
  selectProject: [path: string]
  addProject: []
}>()

const appStore = useAppStore()
const projects = ref<Project[]>([])
const searchQuery = ref('')
const selectedProject = ref<Project | null>(null)
const projectInfo = ref<{ lastSessionId?: string; lastCost?: number } | null>(null)

const localOptions = ref({
  continue: appStore.claudeOptions.continue,
  resume: '',
  skipPermissions: appStore.claudeOptions.skipPermissions,
  customArgs: appStore.claudeOptions.customArgs
})

// 保存状态
const isSaving = ref(false)
const saveSuccess = ref(false)

// 过滤后的项目列表
const filteredProjects = computed(() => {
  const query = searchQuery.value.toLowerCase()
  return projects.value.filter(p => {
    if (!query) return true
    return p.name.toLowerCase().includes(query) ||
           p.path.toLowerCase().includes(query)
  })
})

// 同步选项到 store
watch(localOptions, (val) => {
  appStore.setClaudeOptions(val)
}, { deep: true })

// 当输入 resume 时，自动取消 continue 选项（避免冲突）
watch(() => localOptions.value.resume, (newResume) => {
  if (newResume && localOptions.value.continue) {
    localOptions.value.continue = false
  }
})

onMounted(async () => {
  projects.value = await getProjects()
})

// 选择项目 -> 显示详情（再次点击取消选中）
async function handleSelectProject(project: Project) {
  if (selectedProject.value?.path === project.path) {
    // 点击已选中的项目 -> 取消选中
    selectedProject.value = null
    projectInfo.value = null
  } else {
    selectedProject.value = project

    // 获取项目详细信息
    const info = await getProjectInfo(project.path)
    projectInfo.value = info

    // 如果有 lastSessionId，自动填入 resume
    if (info?.lastSessionId) {
      localOptions.value.resume = info.lastSessionId
    }
  }
}

// 启动项目
function handleLaunch() {
  if (selectedProject.value) {
    emit('selectProject', selectedProject.value.path)
  }
}

// 保存为默认配置
async function handleSaveDefault() {
  isSaving.value = true
  saveSuccess.value = false

  const success = await appStore.saveAsDefault()

  isSaving.value = false
  if (success) {
    saveSuccess.value = true
    // 3秒后自动隐藏成功提示
    setTimeout(() => {
      saveSuccess.value = false
    }, 3000)
  }
}
</script>

<style scoped>
.project-select-view {
  display: flex;
  height: 100vh;
  background: var(--bg-primary);
}

/* 左侧详情面板 */
.detail-panel {
  width: 280px;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-color);
  order: -1;  /* 放在最左边 */
}

.panel-inner {
  width: 280px;
  height: 100%;
  display: flex;
  flex-direction: column;
}

/* 默认面板（无选中状态） */
.default-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
}

.default-panel .panel-header {
  padding: 16px;
  border-bottom: 1px solid var(--border-color);
}

.default-panel .panel-header h2 {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.default-panel .panel-body {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}

.action-section {
  margin-bottom: 20px;
}

.new-project-btn {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 14px 20px;
  background: var(--accent-color);
  border: none;
  border-radius: 6px;
  color: #fff;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: background 0.15s ease;
}

.new-project-btn:hover {
  background: #5b4cdb;
}

.config-hint {
  font-size: 12px;
  color: var(--text-tertiary);
  margin-bottom: 12px;
}

/* 项目详情 */
.project-detail {
  flex: 1;
  display: flex;
  flex-direction: column;
}

.detail-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px;
  border-bottom: 1px solid var(--border-color);
}

.project-title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.project-title h2 {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
}

.project-title svg {
  color: var(--accent-color);
}

.close-btn {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 4px;
}

.close-btn:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.detail-body {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}

.info-section {
  margin-bottom: 16px;
}

.info-section label {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin-bottom: 6px;
  display: block;
}

.path-display {
  font-size: 12px;
  color: var(--text-primary);
  background: var(--bg-primary);
  padding: 8px 12px;
  border-radius: 4px;
  word-break: break-all;
}

.session-info {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--text-secondary);
}

.config-section {
  margin-top: 20px;
  padding-top: 16px;
  border-top: 1px solid var(--border-color);
}

.config-section h3 {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 12px;
}

.option-item {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
  cursor: pointer;
}

.option-item input[type="checkbox"] {
  width: 16px;
  height: 16px;
  accent-color: var(--accent-color);
}

.option-label {
  font-size: 13px;
  color: var(--text-primary);
}

.option-flag {
  font-size: 11px;
  padding: 2px 6px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 3px;
  font-family: 'SF Mono', 'Consolas', 'Monaco', 'Menlo', monospace;
  font-weight: 500;
  color: var(--text-secondary);
  box-shadow: 0 1px 1px rgba(0, 0, 0, 0.04);
}

.option-flag.warning {
  color: #e74c3c;
  border-color: rgba(231, 76, 60, 0.3);
}

.text-option {
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
}

.text-option input[type="text"] {
  width: 100%;
  padding: 6px 10px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-size: 12px;
  color: var(--text-primary);
}

.text-option input[type="text"]:focus {
  outline: none;
  border-color: var(--accent-color);
}

.save-default-btn {
  margin-top: 12px;
  padding: 8px 12px;
  background: transparent;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-size: 12px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s ease;
}

.save-default-btn:hover:not(:disabled):not(.success) {
  border-color: var(--accent-color);
  color: var(--accent-color);
}

.save-default-btn.saving {
  opacity: 0.6;
  cursor: wait;
}

.save-default-btn.success {
  border-color: #27ae60;
  color: #27ae60;
  background: rgba(39, 174, 96, 0.1);
  cursor: default;
}

.save-default-btn:disabled {
  cursor: not-allowed;
}

/* 顶部启动按钮区域 */
.detail-launch-section {
  padding: 16px;
  border-bottom: 1px solid var(--border-color);
}

.launch-btn {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 12px 20px;
  background: var(--accent-color);
  border: none;
  border-radius: 6px;
  color: #fff;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: background 0.15s ease;
}

.launch-btn:hover {
  background: #5b4cdb;
}

/* 右侧项目列表 */
.projects-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 400px;
}

.panel-header {
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-color);
}

.panel-header h2 {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 12px;
}

.search-box {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
}

.search-box svg {
  color: var(--text-secondary);
}

.search-box input {
  flex: 1;
  border: none;
  background: transparent;
  font-size: 13px;
  color: var(--text-primary);
  outline: none;
}

.search-box input::placeholder {
  color: var(--text-secondary);
}

.project-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.project-item {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  padding: 12px 16px;
  border: none;
  background: transparent;
  border-radius: 6px;
  cursor: pointer;
  text-align: left;
  transition: background 0.15s ease;
}

.project-item:hover {
  background: var(--bg-secondary);
}

.project-item.active {
  background: var(--selected-bg);
  border-left: 3px solid var(--accent-color);
}

.folder-icon {
  flex-shrink: 0;
  color: var(--text-secondary);
}

.project-item.active .folder-icon {
  color: var(--accent-color);
}

.project-info {
  flex: 1;
  min-width: 0;
}

.project-name {
  font-size: 14px;
  font-weight: 500;
  color: var(--text-primary);
  display: block;
}

.project-path {
  font-size: 12px;
  color: var(--text-secondary);
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.empty-list {
  padding: 32px;
  text-align: center;
  color: var(--text-secondary);
  font-size: 13px;
}

.panel-footer {
  padding: 12px 16px;
  border-top: 1px solid var(--border-color);
}

.add-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  width: 100%;
  padding: 10px 16px;
  background: transparent;
  border: 1px dashed var(--border-color);
  border-radius: 6px;
  color: var(--text-secondary);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.add-btn:hover {
  border-color: var(--accent-color);
  color: var(--accent-color);
}

/* 滚动条 */
.project-list::-webkit-scrollbar,
.detail-body::-webkit-scrollbar {
  width: 6px;
}

.project-list::-webkit-scrollbar-thumb,
.detail-body::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 3px;
}

.project-list::-webkit-scrollbar-track,
.detail-body::-webkit-scrollbar-track {
  background: transparent;
}
</style>