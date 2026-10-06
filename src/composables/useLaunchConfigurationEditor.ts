import { nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import type { LaunchConfigurationEditorRequest } from '@/types/profile'
import { mapSafeUserError, safeUserErrorCode, type UserErrorPresentation } from '@/utils/userError'

/** Read the shared workspace CAS before mounting a new frozen editor draft. */
export function useLaunchConfigurationEditor(active: () => boolean) {
  const profiles = useCliProfilesStore()
  const editor = shallowRef<LaunchConfigurationEditorRequest | null>(null)
  const opening = ref(false), error = ref<UserErrorPresentation | null>(null)
  let epoch = 0, disposed = false
  let pending: LaunchConfigurationEditorRequest | null = null
  function close() { ++epoch; pending = null; opening.value = false; editor.value = null; error.value = null }
  watch(active, value => { if (!value) close() }, { flush: 'sync' })
  onBeforeUnmount(() => { disposed = true; close() })

  async function open(request: LaunchConfigurationEditorRequest, canContinue: () => boolean = () => true, opener: Element | null = document.activeElement) {
    if (disposed || !active() || editor.value) return
    if (opening.value && JSON.stringify(pending) === JSON.stringify(request)) return
    const owner = ++epoch
    pending = request; opening.value = true; error.value = null
    const owns = () => !disposed && owner === epoch && active() && canContinue()
    try {
      await profiles.load()
      if (!owns()) return
      if (request.kind !== 'create' && !profiles.profile(request.profileId)) throw new Error('PROFILE_SELECTION_CHANGED')
      opening.value = false
      await nextTick()
      if (!owns()) return
      // Loading can blur a disabled trigger, and a menu item can disappear.
      // Restore only a still-visible opener before AppDialog captures it.
      if (opener instanceof HTMLElement && opener.isConnected && !opener.matches(':disabled')
        && !opener.closest('[hidden], [inert], [aria-hidden="true"]')) {
        let visible = true
        for (let element: HTMLElement | null = opener; element; element = element.parentElement) {
          const style = getComputedStyle(element)
          if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') visible = false
          if (element instanceof HTMLDetailsElement && !element.open && !element.querySelector('summary')?.contains(opener)) visible = false
        }
        if (visible) opener.focus()
      }
      editor.value = request
    } catch (failure) {
      if (owns()) error.value = mapSafeUserError(safeUserErrorCode(failure), 'settings')
    } finally {
      if (owner === epoch) { opening.value = false; pending = null }
    }
  }
  return { editor, opening, error, open, close }
}
