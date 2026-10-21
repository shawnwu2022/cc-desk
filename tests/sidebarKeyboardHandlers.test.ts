import { afterEach, describe, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { createPinia, setActivePinia } from 'pinia'
import ProjectNode from '@/components/sessions/ProjectNode.vue'
import en from '@/i18n/locales/en'

let wrapper: VueWrapper | undefined
afterEach(() => { wrapper?.unmount(); document.body.innerHTML = '' })

describe('SidebarKeyboardHandlers', () => {
  // 验证父子控件真实分发，不把某一种 Vue 模板修饰符当作行为契约。
  it('Keyboard_NestedControlDoesNotToggleParent_001', () => {
    setActivePinia(createPinia())
    wrapper = mount(ProjectNode, { attachTo: document.body,
      props: { expanded: false, project: { projectKey: '/repo', projectPath: '/repo', name: 'Repo', sessions: [], pinned: false, hidden: false, runningCount: 0, needsUserCount: 0, lastActivityAt: 1 } },
      global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] },
    })
    const main = wrapper.get('.project-main').element as HTMLElement
    const child = wrapper.get('.project-name').element as HTMLElement
    main.focus()
    for (const key of ['Enter', ' ']) child.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
    const button = wrapper.get('button.project-new-session').element as HTMLElement; button.focus()
    for (const key of ['Enter', ' ']) button.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
    expect(wrapper.emitted('toggle-expand')).toBeUndefined()
    main.focus()
    for (const key of ['Enter', ' ']) {
      const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })
      main.dispatchEvent(event); expect(event.defaultPrevented).toBe(true)
    }
    expect(wrapper.emitted('toggle-expand')).toEqual([['/repo'], ['/repo']])
  })
})
