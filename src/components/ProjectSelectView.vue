<template>
  <div class="project-select-view">
    <div class="projects-panel">
      <header class="panel-header">
        <div class="header-row">
          <!-- 返回终端：无 cwd 时禁用（首次启动未进过项目） -->
          <button
            class="back-btn"
            :disabled="!appStore.cwd"
            @click="handleBack"
            :title="appStore.cwd ? t('backToTerminal') : ''"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <polyline points="15 18 9 12 15 6"/>
            </svg>
            <span>{{ t('backToTerminal') }}</span>
          </button>
          <h2>{{ t('projectManagement') }}</h2>
          <button class="settings-btn" @click="handleSettingsClick" :title="t('titleSettings', { key: ctrl + '+,' })">
            <img src="@/assets/icons/settings.svg" alt="Settings" />
            <span v-if="sidebarStore.updateAvailable" class="update-badge"></span>
          </button>
        </div>
        <div class="search-box">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="11" cy="11" r="8"/>
            <line x1="21" y1="21" x2="16.65" y2="16.65"/>
          </svg>
          <input
            type="text"
            v-model="searchQuery"
            :placeholder="t('search')"
          />
        </div>
      </header>

      <div class="project-list" ref="projectListRef" @scroll="handleProjectScroll">
        <!-- 操作失败提示（pin/hide/restore 持久化失败；内联非阻塞，可手动关闭） -->
        <div v-if="manageError" class="manage-error-banner">
          <span>{{ manageError }}</span>
          <button class="manage-error-close" @click="manageError = null" :title="t('close')">×</button>
        </div>
        <template v-if="!appStore.cacheLoaded">
          <div v-for="i in 6" :key="i" class="skeleton-item">
            <div class="skeleton-folder"></div>
            <div class="skeleton-text-group">
              <div class="skeleton-text skeleton-name"></div>
              <div class="skeleton-text skeleton-sub"></div>
            </div>
          </div>
        </template>
        <template v-else>
          <!-- 项目行 = 静态 div（非 button，不进 Tab 顺序）；点项目本身 = nothing（spec §4.5） -->
          <div
            v-for="project in filteredProjects"
            :key="project.path"
            class="project-row"
            :class="{ hidden: appStore.isHidden(project.path) }"
            :data-path="project.path"
          >
            <svg class="folder-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
            </svg>
            <div class="project-info">
              <template v-if="editingPath === project.path && editState !== 'idle'">
                <input
                  v-model="renameValue"
                  class="rename-input"
                  :maxlength="32"
                  :disabled="editState === 'submitting'"
                  :placeholder="t('aliasPlaceholder')"
                  @click.stop
                  @keyup.enter="onRenameSubmit(project.path)"
                  @keyup.escape="onRenameCancel"
                  @blur="onRenameSubmit(project.path)"
                />
                <span v-if="renameError" class="rename-error">{{ renameError }}</span>
              </template>
              <span v-else class="project-name">{{ sessionStore.getDisplayName(project.path) }}</span>
              <span class="project-path">{{ project.path }}</span>
            </div>
            <!-- 置顶标记 -->
            <span v-if="sessionStore.isPinned(project.path)" class="pin-mark" :title="t('pinned')">📌</span>
            <!-- 运行中 tab 计数徽标 -->
            <span v-if="getRunningCount(project.path) > 0" class="running-number">●{{ getRunningCount(project.path) }}</span>

            <!-- 行尾「进入终端」按钮：隐藏项目禁用 + title 提示「先显示项目」（P1.1） -->
            <button
              class="enter-btn"
              :disabled="appStore.isHidden(project.path)"
              :title="appStore.isHidden(project.path) ? t('showFirst') : t('enterTerminal')"
              @click="handleEnter(project.path)"
            >{{ t('enterTerminal') }}</button>

            <!-- ⋯ 菜单按钮（Tab 可达 + Enter 展开 + aria 协议） -->
            <button
              class="menu-btn"
              :aria-haspopup="true"
              :aria-expanded="menuOpen === project.path"
              :aria-label="t('more')"
              @click.stop="toggleMenu(project.path)"
              @keydown.enter.prevent="toggleMenu(project.path)"
              @keydown.esc.prevent="closeMenuAndRestore(project.path)"
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="12" cy="5" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="12" cy="19" r="1"/>
              </svg>
            </button>

            <!-- ⋯ 菜单：置顶/取消置顶 + 隐藏/显示（cwd 项目禁隐藏）。
                 键盘协议（spec §4.8）：ArrowUp/Down 循环焦点 + Escape 关闭并恢复焦点到触发按钮 -->
            <div
              v-if="menuOpen === project.path"
              class="menu"
              role="menu"
              @click.stop
              @keydown="onMenuKeydown($event, project.path)"
            >
              <button role="menuitem" @click="onMenuPin(project.path)">
                {{ sessionStore.isPinned(project.path) ? t('unpin') : t('pin') }}
              </button>
              <button
                role="menuitem"
                :disabled="isCwdProject(project.path)"
                @click="onMenuHide(project.path)"
              >
                {{ appStore.isHidden(project.path) ? t('show') : t('hide') }}
              </button>
              <button role="menuitem" @click="onMenuRename(project.path)">{{ t('rename') }}</button>
            </div>
          </div>

          <!-- 分页：加载更多 -->
          <div v-if="!searchQuery && appStore.hasMoreProjects && !appStore.isLoadingProjects" class="load-more-section">
            <button class="load-more-btn" @click="handleLoadMore">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="12" y1="5" x2="12" y2="19"/>
                <line x1="5" y1="12" x2="19" y2="12"/>
              </svg>
              <span>{{ t('loadMoreProjects') }}</span>
            </button>
          </div>

          <div v-if="appStore.isLoadingProjects" class="loading-more">
            <span>{{ t('loading') }}</span>
          </div>

          <div v-if="filteredProjects.length === 0 && !appStore.isLoadingProjects" class="empty-list">
            <span v-if="searchQuery">{{ t('noMatchingProjects') }}</span>
            <span v-else>{{ t('noProjectsYet') }}</span>
          </div>
        </template>
      </div>

      <footer class="panel-footer">
        <button class="add-btn" @click="emit('addProject')">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <line x1="12" y1="5" x2="12" y2="19"/>
            <line x1="5" y1="12" x2="19" y2="12"/>
          </svg>
          <span>{{ t('addProject') }}</span>
        </button>
      </footer>

      <!-- 已存档会话全局视图（v-if 块，懒加载；archivedProjects 从 archivedSessions.keys() 直接生成，
           不依赖 cachedProjects 分页，避免分页外已存档项目漏显示） -->
      <div v-if="hasArchived" class="archived-section">
        <h3>{{ t('archivedSessions') }}</h3>
        <button v-if="!archivedLoaded" class="load-archived-btn" @click="loadArchived">{{ t('loadArchived') }}</button>
        <template v-else>
          <div v-for="proj in archivedProjects" :key="proj.path" class="archived-project">
            <div class="archived-project-name">{{ proj.name }}</div>
            <!-- 该项目历史加载失败：内联错误 + 重试（不阻塞其他项目展示） -->
            <div v-if="archivedErrors.has(proj.path)" class="archived-project-error">
              <span>{{ t('loadHistoryFailed') }}</span>
              <button class="archived-retry-btn" @click="retryArchived(proj.path)">{{ t('retry') }}</button>
            </div>
            <button
              v-for="s in sessionStore.getArchivedSessionInfos(proj.path)"
              :key="s.sessionId"
              class="archived-item"
              @click="handleRestore(proj.path, s.sessionId, s.name)"
            >
              <span class="archived-name">{{ s.name }}</span>
              <span v-if="s.lastActiveAt > 0" class="archived-time">{{ formatTime(s.lastActiveAt) }}</span>
            </button>
          </div>
          <!-- 任一项目加载失败时整体提示（可重新加载全部） -->
          <div v-if="archivedErrors.size > 0" class="archived-global-error">
            <span>{{ t('loadArchivedFailed') }}</span>
            <button class="archived-retry-btn" @click="loadArchived">{{ t('retry') }}</button>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, reactive, onMounted, onUnmounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import { useSidebarStore } from '@/stores/sidebar'
import { ctrl } from '@/utils/platform'
import { sameProjectPath } from '@/utils/path'
import { matchProjectQuery, editReducer, validateDisplayName, type EditState } from '@/utils/displayName'

const { t } = useI18n()

const emit = defineEmits<{
  selectProject: [path: string]
  addProject: []
  resumeSession: [projectPath: string, sessionId: string, sessionName?: string]
  backToTerminal: []
}>()

const appStore = useAppStore()
const sessionStore = useSessionStore()
const sidebarStore = useSidebarStore()
const searchQuery = ref('')
const projectListRef = ref<HTMLElement | null>(null)
const menuOpen = ref<string | null>(null)
const archivedLoaded = ref(false)
/** 已存档历史按项目加载失败标记（v6 codex batch2 #9：loadArchived 检查 {ok:false}，逐项目错误 + 可重试） */
const archivedErrors = reactive(new Set<string>())
/** 置顶/隐藏/恢复操作失败提示（统一非阻塞内联提示，取代 .catch(()=>{}) 吞错） */
const manageError = ref<string | null>(null)

// 项目别名编辑：editingPath 逐行（镜像 menuOpen 模式）+ editState/renameValue/renameError 单值（一次只编一行）
const editingPath = ref<string | null>(null)
const editState = ref<EditState>('idle')
const renameValue = ref('')
const renameError = ref('')
let renameRequestId = 0

/** 判断给定项目是否为当前 cwd 项目（规范化比较，容忍 Windows 路径大小写/斜杠差异） */
function isCwdProject(path: string): boolean {
  if (!appStore.cwd) return false
  return sameProjectPath(appStore.cwd, path)
}

/** 过滤 + 排序：置顶优先 -> 最近活跃（与全局树排序语义一致） */
const filteredProjects = computed(() => {
  const q = searchQuery.value.trim().toLowerCase()
  let list = appStore.cachedProjects
  if (q) {
    list = list.filter(p =>
      matchProjectQuery(sessionStore.getDisplayName(p.path), p.name, p.path, q)
    )
  }
  return [...list].sort((a, b) => {
    const aPinned = sessionStore.isPinned(a.path) ? 1 : 0
    const bPinned = sessionStore.isPinned(b.path) ? 1 : 0
    if (aPinned !== bPinned) return bPinned - aPinned
    return (b.lastDuration ?? 0) - (a.lastDuration ?? 0)
  })
})

/** 该项目运行中 tab 计数 */
function getRunningCount(projectPath: string): number {
  return sessionStore.getProjectTabs(projectPath).filter(tab => tab.status === 'running').length
}

/** 进入终端：隐藏项目禁用（P1.1）；非隐藏复用 handleOpenProject（emit selectProject） */
function handleEnter(path: string) {
  if (appStore.isHidden(path)) return
  emit('selectProject', path)
}

/** 返回终端：无 cwd 时禁用（按钮已 disabled，双保险） */
function handleBack() {
  if (!appStore.cwd) return
  emit('backToTerminal')
}

function toggleMenu(path: string) {
  if (menuOpen.value === path) {
    closeMenuAndRestore(path)
  } else {
    menuOpen.value = path
    // 展开后焦点入首个菜单项（aria 菜单按钮模式：展开即焦点入菜单，便于方向键导航）
    nextTick(() => focusFirstMenuItem(path))
  }
}

/** 关闭菜单并恢复焦点到触发按钮（spec §4.8：Escape/菜单项选中后焦点回 menu-btn） */
function closeMenuAndRestore(path?: string) {
  const targetPath = path ?? menuOpen.value
  menuOpen.value = null
  if (targetPath) {
    nextTick(() => getMenuBtn(targetPath)?.focus())
  }
}

/** 按 data-path 定位项目行的菜单触发按钮 */
function getMenuBtn(path: string): HTMLButtonElement | null {
  return projectListRef.value?.querySelector(
    `.project-row[data-path="${cssEscape(path)}"] .menu-btn`
  ) ?? null
}

/** 按数据路径定位当前展开的菜单容器 */
function getMenu(path: string): HTMLElement | null {
  return projectListRef.value?.querySelector(
    `.project-row[data-path="${cssEscape(path)}"] .menu`
  ) ?? null
}

/** 焦点入菜单首个可点项 */
function focusFirstMenuItem(path: string) {
  getMenu(path)?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus()
}

/**
 * 菜单内键盘协议（spec §4.8）：
 * - ArrowDown/ArrowUp：在可点菜单项间循环焦点
 * - Escape：关闭菜单并恢复焦点到触发按钮
 */
function onMenuKeydown(e: KeyboardEvent, path: string) {
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    const menu = getMenu(path)
    if (!menu) return
    const items = Array.from(menu.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'))
    if (items.length === 0) return
    const currentIndex = items.indexOf(document.activeElement as HTMLButtonElement)
    let nextIndex: number
    if (e.key === 'ArrowDown') {
      nextIndex = currentIndex < 0 ? 0 : (currentIndex + 1) % items.length
    } else {
      nextIndex = currentIndex <= 0 ? items.length - 1 : currentIndex - 1
    }
    items[nextIndex].focus()
  } else if (e.key === 'Escape') {
    e.preventDefault()
    closeMenuAndRestore(path)
  }
}

/** CSS 属性选择器转义：路径含特殊字符（空格/括号等）时避免 querySelector 解析失败 */
function cssEscape(value: string): string {
  return (window.CSS && CSS.escape) ? CSS.escape(value) : value.replace(/["\\]/g, '\\$&')
}

/** 点击外部关闭菜单（⋯ 按钮 / 菜单内部已 @click.stop，不会触发此处） */
function closeOnOutside() {
  menuOpen.value = null
}
onMounted(() => {
  document.addEventListener('click', closeOnOutside)
  document.addEventListener('keydown', onGlobalKeydown)
})
onUnmounted(() => {
  document.removeEventListener('click', closeOnOutside)
  document.removeEventListener('keydown', onGlobalKeydown)
})
function onGlobalKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') closeMenuAndRestore()
}

function onMenuPin(path: string) {
  const op = sessionStore.isPinned(path) ? sessionStore.unpinProject(path) : sessionStore.pinProject(path)
  op.catch(() => { manageError.value = t('operationFailed') })
  closeMenuAndRestore(path)
}

function onMenuHide(path: string) {
  // cwd 项目禁隐藏（规范化比较；与模板 :disabled 一致）
  if (isCwdProject(path)) return
  const willHide = !appStore.isHidden(path)
  appStore.setHidden(path, willHide).catch(() => { manageError.value = t('operationFailed') })
  closeMenuAndRestore(path)
}

/** ⋯ 菜单「重命名」：关菜单 + 进入编辑 */
function onMenuRename(path: string) {
  closeMenuAndRestore(path)
  startRename(path)
}

/** 进入重命名：作废在途 + 设 editingPath + 预填当前别名（或 basename） + 焦点入 input（querySelector 精确定位行） */
function startRename(path: string) {
  renameRequestId++  // 作废先前在途
  editingPath.value = path
  renameValue.value = sessionStore.getDisplayName(path)
  renameError.value = ''
  // 显式重置 editing（v6-T5 Fix Round 1：修复卡死--submitting 态 reducer start 不变致 input 永久禁用；
  // renameRequestId++ 已作废旧 persist，旧请求完成时 myId!==renameRequestId 早 return 不改 editState，安全）
  editState.value = 'editing'
  nextTick(() => {
    projectListRef.value?.querySelector<HTMLInputElement>(
      `.project-row[data-path="${cssEscape(path)}"] .rename-input`
    )?.focus()
  })
}

/**
 * 提交别名：仅 editing/error 态生效（submitting 幂等；idle 忽略）。
 * 校验失败 -> error（保留 input + 错误）；persist 失败 -> error；成功才关 input + 清 editingPath。
 * request id 防 cancel-during-submit 后旧 success 覆盖 idle。
 */
async function onRenameSubmit(path: string) {
  // 仅 editing/error 态提交（submitting 幂等；idle 忽略）
  const next = editReducer(editState.value, { type: 'submit' })
  if (next === editState.value) return
  editState.value = next  // -> submitting
  const raw = renameValue.value
  const v = validateDisplayName(raw)
  if (!v.ok) {
    renameError.value = v.error === 'tooLong' ? t('aliasTooLong') : t('aliasInvalid')
    editState.value = editReducer(editState.value, { type: 'fail' })  // -> error
    return
  }
  const trimmed = raw.trim()
  // no-op：值未变（含 basename 回退场景）则直接成功关闭，不持久化
  if (trimmed === sessionStore.getDisplayName(path)) {
    editState.value = editReducer(editState.value, { type: 'success' })
    renameError.value = ''
    editingPath.value = null
    return
  }
  const myId = ++renameRequestId
  try {
    await sessionStore.setDisplayName(path, trimmed)
    if (myId !== renameRequestId) return  // 旧请求作废
    editState.value = editReducer(editState.value, { type: 'success' })  // 成功才关
    renameError.value = ''
    editingPath.value = null
  } catch {
    if (myId !== renameRequestId) return
    renameError.value = t('aliasPersistFailed')
    editState.value = editReducer(editState.value, { type: 'fail' })  // -> error 保留 input
  }
}

/** 取消重命名：作废在途 + 清错 + 清 editingPath + 回 idle（不改） */
function onRenameCancel() {
  renameRequestId++
  renameError.value = ''
  editingPath.value = null
  editState.value = editReducer(editState.value, { type: 'cancel' })
}

function handleSettingsClick() {
  if (sidebarStore.updateAvailable) {
    sidebarStore.openSettings('update')
  } else {
    sidebarStore.openSettings()
  }
}

function handleProjectScroll() {
  const el = projectListRef.value
  if (!el || searchQuery.value) return

  const nearBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 80
  if (nearBottom && appStore.hasMoreProjects && !appStore.isLoadingProjects) {
    appStore.loadMoreProjects()
  }
}

function handleLoadMore() {
  if (!appStore.isLoadingProjects && appStore.hasMoreProjects) {
    appStore.loadMoreProjects()
  }
}

// ---- 已存档会话全局视图 ----

/** archivedSessions 非空时显示入口 */
const hasArchived = computed(() => sessionStore.archivedSessions.size > 0)

/**
 * 已存档项目列表：直接从 archivedSessions.keys() 生成（不依赖 cachedProjects 分页，
 * 避免分页外的已存档项目漏显示）。name 用 getDisplayName（别名优先 basename 回退）。
 */
const archivedProjects = computed(() => {
  return [...sessionStore.archivedSessions.keys()].map(k => ({
    path: k,
    name: sessionStore.getDisplayName(k),
  }))
})

  /**
   * 懒加载所有已存档项目的历史（loadHistoryFor 无副作用，不改 currentHistoryProject）。
   * v6 codex batch2 #9：逐项目检查 loadHistoryFor 返回 {ok:false} -> 标记失败，支持重试。
   * 任一失败不阻塞其余项目，archivedLoaded 仍置 true 展示成功部分 + 失败项重试入口。
   */
  async function loadArchived() {
    archivedErrors.clear()
    const results = await Promise.all(
      archivedProjects.value.map(async p => ({ path: p.path, res: await sessionStore.loadHistoryFor(p.path) }))
    )
    for (const { path, res } of results) {
      if (!res.ok) archivedErrors.add(path)
    }
    archivedLoaded.value = true
  }

  /** 重试单个失败项目的已存档历史（force 刷新；成功则清错误标记） */
  async function retryArchived(path: string) {
    const res = await sessionStore.loadHistoryFor(path, true)
    if (res.ok) archivedErrors.delete(path)
  }

  /** 恢复存档会话：restoreSession 持久化后切到该会话（--resume），透传 sessionName 供 tab 命名 */
  function handleRestore(projectPath: string, sessionId: string, sessionName?: string) {
    sessionStore.restoreSession(projectPath, sessionId).then(() => {
      emit('resumeSession', projectPath, sessionId, sessionName)
    }).catch(() => { manageError.value = t('operationFailed') })
  }

function formatTime(ts: number): string {
  if (!ts) return ''
  const diff = Date.now() - ts
  const minutes = Math.floor(diff / 60000)
  if (minutes < 1) return t('timeNow')
  if (minutes < 60) return t('timeMinutes', { n: minutes })
  const hours = Math.floor(diff / 3600000)
  if (hours < 24) return t('timeHours', { n: hours })
  return t('timeDays', { n: Math.floor(diff / 86400000) })
}
</script>

<style scoped>
.project-select-view {
  display: flex;
  background: var(--bg-primary);
}

.projects-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 400px;
  max-width: 720px;
  margin: 0 auto;
  width: 100%;
}

.panel-header {
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-color);
}

.header-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}

.back-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border: 1px solid var(--border-color);
  background: transparent;
  color: var(--text-secondary);
  border-radius: var(--radius-md);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.back-btn:hover:not(:disabled) {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
}

.back-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.panel-header h2 {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  letter-spacing: -0.3px;
  flex: 1;
  text-align: center;
}

.settings-btn {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 6px;
  transition: all 0.15s ease;
}

.update-badge {
  position: absolute;
  top: 4px;
  right: 4px;
  width: 8px;
  height: 8px;
  background: var(--status-error);
  border-radius: 50%;
  border: 2px solid var(--bg-primary);
  animation: pulse 2s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.6; }
}

.settings-btn img {
  width: 16px;
  height: 16px;
  opacity: 0.85;
}

.settings-btn:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.settings-btn:hover img {
  opacity: 1;
}

.search-box {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  transition: border-color 0.15s ease;
}

.search-box:focus-within {
  border-color: var(--focus-ring);
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
  position: relative;
}

/* 项目行 = 静态 div（非 button，不进 Tab 顺序）；点项目本身 = nothing */
.project-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  transition: background 0.15s ease;
  position: relative;
}

.project-row:hover {
  background: var(--bg-secondary);
}

/* 隐藏项目整行置灰；hover 显示 enter/menu 按钮仍可点（菜单用于"显示"恢复） */
.project-row.hidden {
  opacity: 0.5;
}

.folder-icon {
  flex-shrink: 0;
  color: var(--text-secondary);
  transition: color 0.15s ease;
}

.project-row:hover .folder-icon {
  color: var(--accent-gold);
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
  font-size: 11px;
  color: var(--text-secondary);
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.rename-input {
  display: block;
  width: 100%;
  box-sizing: border-box;
  font-size: 13px;
  padding: 2px 4px;
  border: 1px solid var(--accent-primary);
  border-radius: 3px;
  background: var(--bg-primary);
  color: var(--text-primary);
  outline: none;
}

.rename-input:disabled {
  opacity: 0.6;
}

.rename-error {
  display: block;
  font-size: 10px;
  color: var(--status-error);
  margin-top: 2px;
}

.pin-mark {
  flex-shrink: 0;
  font-size: 12px;
}

.running-number {
  font-size: 12px;
  color: var(--status-success);
  font-weight: 500;
  flex-shrink: 0;
}

/* 行尾按钮：默认隐藏，hover 行时显示（隐藏项目 enter 禁用） */
.enter-btn,
.menu-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 5px 10px;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-secondary);
  border-radius: var(--radius-sm);
  font-size: 11px;
  cursor: pointer;
  transition: all 0.15s ease;
  flex-shrink: 0;
}

.menu-btn {
  padding: 5px 7px;
}

.enter-btn:hover:not(:disabled) {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
}

.enter-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.menu-btn:hover {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
}

/* ⋯ 菜单下拉 */
.menu {
  position: absolute;
  top: calc(100% - 4px);
  right: 8px;
  min-width: 120px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.12);
  padding: 4px;
  z-index: 10;
}

.menu button {
  display: block;
  width: 100%;
  text-align: left;
  padding: 7px 10px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  font-size: 12px;
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.menu button:hover:not(:disabled) {
  background: var(--hover-bg);
}

.menu button:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.empty-list {
  padding: 32px;
  text-align: center;
  color: var(--text-secondary);
  font-size: 13px;
}

.load-more-section {
  padding: 16px;
  text-align: center;
}

.load-more-btn {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 10px 20px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  color: var(--text-secondary);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.load-more-btn:hover {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
  background: var(--hover-bg);
}

.loading-more {
  padding: 16px;
  text-align: center;
  color: var(--text-tertiary);
  font-size: 12px;
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
  border: 1px dashed var(--border-dark);
  border-radius: var(--radius-md);
  color: var(--text-secondary);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.add-btn:hover {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
  border-style: solid;
}

/* 已存档会话全局视图（管理页底部 v-if 块） */
.archived-section {
  border-top: 1px solid var(--border-color);
  padding: 16px 20px;
}

.archived-section h3 {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin-bottom: 12px;
}

.load-archived-btn {
  padding: 8px 16px;
  border: 1px solid var(--border-color);
  background: transparent;
  color: var(--text-secondary);
  border-radius: var(--radius-md);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.load-archived-btn:hover {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
}

.archived-project {
  margin-bottom: 12px;
}

.archived-project-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 4px;
  padding: 0 4px;
}

.archived-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
  padding: 8px 12px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  cursor: pointer;
  text-align: left;
  transition: background 0.15s ease;
}

.archived-item:hover {
  background: var(--hover-bg);
}

.archived-name {
  font-size: 13px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.archived-time {
  font-size: 11px;
  color: var(--text-tertiary);
  flex-shrink: 0;
}

/* 已存档历史加载失败（逐项目 + 全局提示）+ 操作失败 banner（v6 codex batch2 #9） */
.archived-project-error,
.archived-global-error {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 4px 12px;
  font-size: 11px;
  color: var(--status-error);
}
.archived-retry-btn {
  padding: 2px 10px;
  border: 1px solid var(--border-color);
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  border-radius: var(--radius-sm);
  font-size: 11px;
  transition: all 0.15s ease;
  flex-shrink: 0;
}
.archived-retry-btn:hover {
  border-color: var(--accent-primary);
  color: var(--accent-primary);
}
.manage-error-banner {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin: 8px;
  padding: 8px 12px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-left: 3px solid var(--status-error);
  border-radius: var(--radius-sm);
  font-size: 12px;
  color: var(--status-error);
}
.manage-error-close {
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 16px;
  line-height: 1;
  padding: 0 4px;
  border-radius: 3px;
}
.manage-error-close:hover {
  color: var(--text-primary);
  background: var(--hover-bg);
}

/* 滚动条 */
.project-list::-webkit-scrollbar {
  width: 6px;
}

.project-list::-webkit-scrollbar-thumb {
  background: var(--border-dark);
  border-radius: 3px;
}

.project-list::-webkit-scrollbar-track {
  background: transparent;
}

/* 骨架屏 */
@keyframes skeleton-pulse {
  0%, 100% { opacity: 0.4; }
  50% { opacity: 0.8; }
}

.skeleton-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
}

.skeleton-folder {
  width: 18px;
  height: 18px;
  border-radius: 3px;
  background: var(--border-color);
  animation: skeleton-pulse 1.5s ease-in-out infinite;
  flex-shrink: 0;
}

.skeleton-text-group {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.skeleton-text {
  border-radius: 3px;
  background: var(--border-color);
  animation: skeleton-pulse 1.5s ease-in-out infinite;
}

.skeleton-name {
  width: 60%;
  height: 13px;
}

.skeleton-sub {
  width: 80%;
  height: 11px;
}
</style>
