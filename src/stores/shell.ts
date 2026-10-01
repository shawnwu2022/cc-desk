import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import type {
  ProjectActionRequest, SessionMenuAction, SessionPrimaryAction,
  SessionTreeConfirmationRequest, NewSessionRequest, CreateUnifiedSessionInput, ResumeDialogRequest,
} from '@/types/unifiedSession'

export type ShellSection = 'workspace' | 'projects' | 'settings'
export type ShellResponsiveMode = 'wide' | 'overlay' | 'compact'
/** Presentation requests only. Task 11 owns adapter admission and runtime dispatch. */
export type WorkspaceRequest =
  | { kind: 'add-project' | 'refresh' }
  | { kind: 'open-project'; projectPath: string }
  | { kind: 'new-session'; project: NewSessionRequest }
  | { kind: 'create-session'; input: CreateUnifiedSessionInput }
  | ({ kind: 'restore-session' } & ResumeDialogRequest)
  | { kind: 'activate' | 'restore-archive' | 'rename-cancel'; sessionId: string }
  | { kind: 'primary-action'; sessionId: string; action: SessionPrimaryAction }
  | { kind: 'menu-action'; sessionId: string; action: SessionMenuAction }
  | { kind: 'rename'; sessionId: string; title: string }
  | { kind: 'project-action'; request: ProjectActionRequest }
  | { kind: 'confirmation'; request: SessionTreeConfirmationRequest }

export const useShellStore = defineStore('shell', () => {
  const section = ref<ShellSection>('workspace')
  const navigationSequence = ref(0)
  const viewportWidth = ref(1280)
  const sidebarWidth = ref(288)
  const drawerWidth = ref(344)
  const drawerVisible = ref(false)
  const desktopSidebarVisible = ref(true)
  const compactSidebarVisible = ref(false)
  const responsiveMode = computed<ShellResponsiveMode>(() =>
    viewportWidth.value < 900 ? 'compact' : viewportWidth.value < 1180 ? 'overlay' : 'wide',
  )
  // Keep the user's desktop choice while logical scaling enters/leaves compact mode.
  const sidebarVisible = computed({
    get: () => responsiveMode.value === 'compact' ? compactSidebarVisible.value : desktopSidebarVisible.value,
    set: (visible: boolean) => {
      if (responsiveMode.value === 'compact') compactSidebarVisible.value = visible
      else desktopSidebarVisible.value = visible
    },
  })
  const pendingRequest = ref<WorkspaceRequest | null>(null)
  const requestSequence = ref(0)

  function navigate(destination: ShellSection) { section.value = destination; ++navigationSequence.value }
  function setViewportWidth(width: number) {
    if (Number.isFinite(width) && width > 0) viewportWidth.value = width
  }
  function setSidebarWidth(width: number) {
    if (Number.isFinite(width)) sidebarWidth.value = Math.min(360, Math.max(240, width))
  }
  function setDrawerWidth(width: number) {
    if (Number.isFinite(width)) drawerWidth.value = Math.min(420, Math.max(300, width))
  }
  function toggleSidebar() { sidebarVisible.value = !sidebarVisible.value }
  function toggleDrawer() { drawerVisible.value = !drawerVisible.value }
  function requestWorkspaceAction(request: WorkspaceRequest) {
    pendingRequest.value = request
    ++requestSequence.value
  }
  // No automatic replay: an integrating owner must explicitly handle the current
  // request and clear only the sequence it owns. This is not a persistent queue.
  function clearWorkspaceRequest(sequence: number) {
    if (sequence === requestSequence.value) pendingRequest.value = null
  }
  return {
    section, navigationSequence, viewportWidth, sidebarVisible, sidebarWidth, drawerVisible, drawerWidth,
    responsiveMode, pendingRequest, requestSequence, navigate, setViewportWidth,
    setSidebarWidth, setDrawerWidth, toggleSidebar, toggleDrawer,
    requestWorkspaceAction, clearWorkspaceRequest,
  }
})
