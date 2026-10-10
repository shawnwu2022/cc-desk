import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DOMWrapper, mount, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import { compileStyle } from '@vue/compiler-sfc'
import { h, nextTick, ref } from 'vue'
import { readFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { resolve } from 'node:path'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import SessionStatusIcon from '@/components/sessions/SessionStatusIcon.vue'
import CliAppIcon from '@/components/sessions/CliAppIcon.vue'
import type { SessionVisualState, UnifiedCliKind } from '@/types/unifiedSession'

const body = new DOMWrapper(document.body)
const mounted: VueWrapper[] = []
let i18n = createI18n({ legacy: false, locale: 'en', fallbackLocale: 'en', messages: { en, zh } })
beforeEach(() => {
  i18n = createI18n({ legacy: false, locale: 'en', fallbackLocale: 'en', messages: { en, zh } })
})
afterEach(() => {
  vi.restoreAllMocks()
  mounted.splice(0).forEach((wrapper) => wrapper.unmount())
  document.body.innerHTML = ''
  document.documentElement.removeAttribute('data-theme')
  document.head.querySelectorAll('[data-test-session-icons]').forEach((style) => style.remove())
})

describe('SessionIcons', () => {
  // 六种状态统一圆形外轮廓，并用弧段及内部符号区分，颜色不是唯一辨识手段。
  it('Status_DistinctShapes_001', () => {
    const states: SessionVisualState[] = ['starting', 'running', 'needs-user', 'confirming', 'ended', 'failed']
    const shapes: string[] = []
    const geometries: string[] = []
    for (const state of states) {
      const wrapper = mount(SessionStatusIcon, { props: { state }, global: { plugins: [i18n] } })
      mounted.push(wrapper)
      const svg = wrapper.get('svg')
      shapes.push(svg.attributes('data-shape')!)
      geometries.push(svg.element.innerHTML)
      expect(svg.attributes('viewBox')).toBe('0 0 16 16')
      expect(svg.attributes('aria-hidden')).toBe('true')
      expect(svg.find('text').exists()).toBe(false)
      const circle = svg.find('circle[cx="8"][cy="8"][r="6"]')
      expect(circle.exists(), `${state} must keep the shared circular outline`).toBe(true)
      expect(circle.attributes('fill')).toBe('currentColor')
      expect(svg.find('[data-status-mark]').exists()).toBe(true)
    }
    expect(shapes).toEqual(['gap-ring', 'idle-dot', 'reply-dot', 'confirmation-clock', 'stop-circle', 'alert-circle'])
    expect(new Set(geometries).size).toBe(6)
  })

  // 状态提示通过实际键盘触发点建立描述关联，初始会话行不含状态文字。
  it.each([
    { name: 'Status_StartingEnglish_002', state: 'starting', locale: 'en', label: 'Starting' },
    { name: 'Status_RunningEnglish_003', state: 'running', locale: 'en', label: 'Running' },
    { name: 'Status_ReplyEnglish_004', state: 'needs-user', locale: 'en', label: 'Needs reply' },
    { name: 'Status_ConfirmEnglish_005', state: 'confirming', locale: 'en', label: 'Confirming status' },
    { name: 'Status_EndedEnglish_006', state: 'ended', locale: 'en', label: 'Ended' },
    { name: 'Status_FailedEnglish_007', state: 'failed', locale: 'en', label: 'Failed' },
    { name: 'Status_StartingChinese_008', state: 'starting', locale: 'zh', label: '启动中' },
    { name: 'Status_RunningChinese_009', state: 'running', locale: 'zh', label: '运行中' },
    { name: 'Status_ReplyChinese_010', state: 'needs-user', locale: 'zh', label: '需要回复' },
    { name: 'Status_ConfirmChinese_011', state: 'confirming', locale: 'zh', label: '状态确认中' },
    { name: 'Status_EndedChinese_012', state: 'ended', locale: 'zh', label: '已结束' },
    { name: 'Status_FailedChinese_013', state: 'failed', locale: 'zh', label: '失败' },
  ] as const)('$name', async ({ state, locale, label }) => {
    i18n.global.locale.value = locale
    const wrapper = mount(SessionStatusIcon, { attachTo: document.body, props: { state }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    const trigger = wrapper.get('[role="img"]')
    expect(trigger.attributes('aria-label')).toBe(label)
    expect(trigger.attributes('tabindex')).toBe('0')
    expect(wrapper.text()).toBe('')
    expect(body.find('[role="tooltip"]').exists()).toBe(false)
    ;(trigger.element as HTMLElement).focus()
    await nextTick()
    expect(document.activeElement).toBe(trigger.element)
    expect(body.get('[role="tooltip"]').text()).toBe(label)
    expect(trigger.attributes('aria-describedby')).toBe(body.get('[role="tooltip"]').attributes('id'))
    expect(trigger.text()).toBe('')
    await trigger.trigger('keydown', { key: 'Escape' })
    expect(body.find('[role="tooltip"]').exists()).toBe(false)
  })

  // 已打开的状态提示随语言切换更新，形状不因本地化刷新而重新创建。
  it('Status_LocaleKeepsShape_014', async () => {
    const wrapper = mount(SessionStatusIcon, { props: { state: 'needs-user' }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    const svg = wrapper.get('svg').element
    await wrapper.get('[role="img"]').trigger('mouseenter')
    expect(body.get('[role="tooltip"]').text()).toBe('Needs reply')
    i18n.global.locale.value = 'zh'
    await nextTick()
    expect(wrapper.get('[role="img"]').attributes('aria-label')).toBe('需要回复')
    expect(body.get('[role="tooltip"]').text()).toBe('需要回复')
    expect(wrapper.get('svg').element).toBe(svg)
  })

  // 需要回复的轻提示只在进入状态时执行一次，重复渲染不重播。
  it('Status_AttentionEntry_015', async () => {
    const wrapper = mount(SessionStatusIcon, { props: { state: 'running' }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    const running = wrapper.get('svg').element
    await wrapper.setProps({ state: 'needs-user' })
    const attention = wrapper.get('svg').element
    expect(attention).not.toBe(running)
    await wrapper.setProps({ state: 'needs-user' })
    expect(wrapper.get('svg').element).toBe(attention)
    await wrapper.setProps({ state: 'running' })
    await wrapper.setProps({ state: 'needs-user' })
    expect(wrapper.get('svg').element).not.toBe(attention)
  })

  // 完成与等待轻提示只在真正状态转换时进入，初次挂载和刷新不重播。
  it('Status_TransitionsNotRemounts_028', async () => {
    const wrapper = mount(SessionStatusIcon, { props: { state: 'completed' }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBeUndefined()
    await wrapper.setProps({ state: 'running' })
    await wrapper.setProps({ state: 'permission' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBe('permission')
    await wrapper.get('[role="img"]').trigger('animationend', { animationName: 'session-status-entry' })
    await wrapper.setProps({ state: 'permission' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBeUndefined()
    await wrapper.setProps({ state: 'completed' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBe('completed')
    await wrapper.get('[role="img"]').trigger('animationend', { animationName: 'session-completion-entry' })
    await wrapper.setProps({ state: 'completed' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBeUndefined()
    await wrapper.setProps({ activityState: 'idle' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBeUndefined()
    const remounted = mount(SessionStatusIcon, { props: { state: 'completed' }, global: { plugins: [i18n] } })
    mounted.push(remounted)
    expect(remounted.get('[role="img"]').attributes('data-status-entry')).toBeUndefined()
  })

  // 选择造成的徽标抑制不代表新事件；未知态没有工作动画，空闲不用播放动作符号。
  it('Status_SelectionDoesNotReplay_029', async () => {
    const wrapper = mount(SessionStatusIcon, { props: { state: 'running', transitionState: 'permission', activityState: 'waiting_permission' }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    await wrapper.setProps({ state: 'permission' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBeUndefined()
    await wrapper.setProps({ state: 'working', transitionState: 'working', activityState: 'working' })
    await wrapper.setProps({ state: 'needs-user', transitionState: 'needs-user', activityState: 'waiting_input' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBe('needs-user')
    await wrapper.setProps({ state: 'unknown', transitionState: 'unknown', activityState: 'unknown' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBeUndefined()
    await wrapper.setProps({ state: 'running', transitionState: 'running', activityState: 'idle' })
    expect(wrapper.get('svg [data-status-mark]').element.children).toHaveLength(0)
    await wrapper.setProps({ state: 'confirming', transitionState: 'confirming' })
    expect(wrapper.get('svg').attributes('data-shape')).toBe('confirmation-clock')
  })

  // 减弱动效的状态转换不保留待播放标记；动画取消也清除一次性过渡。
  it('Status_ReducedTransitionStatic_030', async () => {
    const media = vi.spyOn(window, 'matchMedia').mockReturnValue({ matches: true } as MediaQueryList)
    const wrapper = mount(SessionStatusIcon, { props: { state: 'working' }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    await wrapper.setProps({ state: 'completed' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBeUndefined()
    media.mockReturnValue({ matches: false } as MediaQueryList)
    await wrapper.setProps({ state: 'permission' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBe('permission')
    await wrapper.get('[role="img"]').trigger('animationcancel', { animationName: 'session-status-entry' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBeUndefined()
  })

  // 旧工作子图形的迟到动画取消，不能清除新等待状态的一次轻提示。
  it('Status_LoopCancelKeepsEntry_031', async () => {
    const wrapper = mount(SessionStatusIcon, { props: { state: 'working', activityState: 'thinking' }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    await wrapper.setProps({ state: 'permission', activityState: 'waiting_permission' })
    await wrapper.get('[role="img"]').trigger('animationcancel', { animationName: 'session-thinking-dot' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBe('permission')
    await wrapper.get('[role="img"]').trigger('animationcancel', { animationName: 'session-status-entry' })
    expect(wrapper.get('[role="img"]').attributes('data-status-entry')).toBeUndefined()
  })

  // 检查生产样式的状态动效次数与减弱动效分支，不把 jsdom 当作视觉验收。
  it('Status_ReducedMotion_016', () => {
    const source = readFileSync(resolve('src/components/sessions/SessionStatusIcon.vue'), 'utf8')
    const style = document.createElement('style')
    style.dataset.testSessionIcons = ''
    style.textContent = source.match(/<style[^>]*>([\s\S]*?)<\/style>/)![1]
    document.head.append(style)
    const rules = Array.from(style.sheet!.cssRules)
    const animated = rules.filter((rule): rule is CSSStyleRule => rule instanceof CSSStyleRule && !!rule.style.getPropertyValue('animation'))
    const loops = animated.filter(rule => rule.style.getPropertyValue('animation').includes('infinite'))
    expect(loops).toHaveLength(3)
    expect(loops.every(rule => /--(?:working|starting)/.test(rule.selectorText))).toBe(true)
    expect(animated.filter(rule => / 1$/.test(rule.style.getPropertyValue('animation')))).toHaveLength(2)
    expect(animated.some(rule => /rotate/.test(rule.style.getPropertyValue('animation')))).toBe(false)
    const reduced = rules.find((rule) => rule instanceof CSSMediaRule && rule.conditionText === '(prefers-reduced-motion: reduce)') as CSSMediaRule
    const reducedShape = Array.from(reduced.cssRules).find((rule) => rule instanceof CSSStyleRule && rule.selectorText.split(',').map(selector => selector.trim()).includes('.session-status-icon .session-status-icon__shape')) as CSSStyleRule
    expect(reducedShape, 'Reduced-motion override must match the state animation specificity').toBeDefined()
    expect(reducedShape.style.getPropertyValue('animation')).toBe('none')
  })

  // 图标键盘焦点沿用全局墨蓝 2px 焦点线，CLI 图标保持 16px 且绕过旧图片滤镜。
  it('Icons_FocusAndSize_017', () => {
    for (const file of ['SessionStatusIcon', 'CliAppIcon']) {
      const source = readFileSync(resolve(`src/components/sessions/${file}.vue`), 'utf8')
      const style = document.createElement('style')
      style.dataset.testSessionIcons = ''
      style.textContent = source.match(/<style[^>]*>([\s\S]*?)<\/style>/)![1]
      document.head.append(style)
      const rules = Array.from(style.sheet!.cssRules).filter((rule): rule is CSSStyleRule => rule instanceof CSSStyleRule)
      const focus = rules.find((rule) => rule.selectorText.includes(':focus-visible'))!
      expect(focus.style.getPropertyValue('outline')).toBe('2px solid var(--focus-ring)')
      const size = rules.find((rule) => rule.style.getPropertyValue('width') === '16px' && rule.style.getPropertyValue('height') === '16px')
      expect(size).toBeDefined()
      if (file === 'CliAppIcon') {
        const image = rules.find((rule) => rule.selectorText.includes('.cli-app-icon__image'))!
        expect(image.style.getPropertyValue('filter')).toBe('none')
      }
    }
  })

  // CLI 名称只出现在可访问名称与 Tooltip；图像加载前后不预先显示字母回退。
  it.each([
    { name: 'Cli_ClaudeAccessible_018', cli: 'claude', label: 'Claude Code' },
    { name: 'Cli_CodexAccessible_019', cli: 'codex', label: 'Codex CLI' },
  ] as const)('$name', async ({ cli, label }) => {
    const wrapper = mount(CliAppIcon, { attachTo: document.body, props: { cli } })
    mounted.push(wrapper)
    const trigger = wrapper.get('[role="img"]')
    expect(trigger.attributes('aria-label')).toBe(label)
    expect(trigger.attributes('tabindex')).toBe('0')
    expect(wrapper.text()).toBe('')
    expect(wrapper.get('img').attributes('src')).toContain(`/cli/${cli}.svg`)
    expect(wrapper.get('img').attributes('alt')).toBe('')
    expect(wrapper.get('img').attributes('aria-hidden')).toBe('true')
    await wrapper.get('img').trigger('load')
    expect(trigger.text()).toBe('')
    ;(trigger.element as HTMLElement).focus()
    await nextTick()
    expect(body.get('[role="tooltip"]').text()).toBe(label)
    expect(trigger.attributes('aria-describedby')).toBe(body.get('[role="tooltip"]').attributes('id'))
    await trigger.trigger('keydown', { key: 'Escape' })
    expect(body.find('[role="tooltip"]').exists()).toBe(false)
    await trigger.trigger('blur')
    await trigger.trigger('mouseenter')
    expect(body.get('[role="tooltip"]').text()).toBe(label)
  })

  // 只有实际 SVG error 事件允许显示 CC 或 CX，仍由完整 CLI 名称标记。
  it.each([
    { name: 'Cli_ClaudeErrorFallback_020', cli: 'claude', label: 'Claude Code', fallback: 'CC' },
    { name: 'Cli_CodexErrorFallback_021', cli: 'codex', label: 'Codex CLI', fallback: 'CX' },
  ] as const)('$name', async ({ cli, label, fallback }) => {
    const wrapper = mount(CliAppIcon, { props: { cli } })
    mounted.push(wrapper)
    const trigger = wrapper.get('[role="img"]')
    const triggerElement = trigger.element
    expect(trigger.text()).toBe('')
    await wrapper.get('img').trigger('error')
    expect(trigger.text()).toBe(fallback)
    expect(trigger.find('img').exists()).toBe(false)
    expect(trigger.get('[aria-hidden="true"]').text()).toBe(fallback)
    expect(trigger.attributes('aria-label')).toBe(label)
    expect(trigger.element).toBe(triggerElement)
    await trigger.trigger('focus')
    expect(body.get('[role="tooltip"]').text()).toBe(label)
  })

  // CLI 变更会清除旧图片失败态，不能把 CC/CX 带到另一种 CLI。
  it('Cli_SwitchClearsFailure_022', async () => {
    const wrapper = mount(CliAppIcon, { props: { cli: 'claude' } })
    mounted.push(wrapper)
    await wrapper.get('img').trigger('error')
    expect(wrapper.get('[role="img"]').text()).toBe('CC')
    await wrapper.setProps({ cli: 'codex' })
    expect(wrapper.get('[role="img"]').text()).toBe('')
    expect(wrapper.get('img').attributes('src')).toContain('/cli/codex.svg')
    await wrapper.get('img').trigger('error')
    expect(wrapper.get('[role="img"]').text()).toBe('CX')
    await wrapper.setProps({ cli: 'claude' })
    expect(wrapper.get('[role="img"]').text()).toBe('')
    expect(wrapper.get('img').attributes('src')).toContain('/cli/claude.svg')
  })

  // 切换后旧 DOM 图片的迟到失败不能覆盖当前 CLI 的加载状态。
  it('Cli_StaleErrorIgnored_023', async () => {
    const wrapper = mount(CliAppIcon, { props: { cli: 'claude' } })
    mounted.push(wrapper)
    const oldImage = wrapper.get('img').element
    await wrapper.setProps({ cli: 'codex' })
    expect(wrapper.get('img').element).not.toBe(oldImage)
    oldImage.dispatchEvent(new Event('error'))
    await nextTick()
    expect(wrapper.get('[role="img"]').text()).toBe('')
    expect(wrapper.get('img').attributes('src')).toContain('/cli/codex.svg')
  })

  // 六种会话状态切换不会修改旁边 CLI 应用图标或品牌颜色。
  it('Cli_StatusIndependent_024', async () => {
    const state = ref<SessionVisualState>('starting')
    const cli = ref<UnifiedCliKind>('claude')
    const wrapper = mount({ render: () => h('div', [h(SessionStatusIcon, { state: state.value }), h(CliAppIcon, { cli: cli.value })]) }, { global: { plugins: [i18n] } })
    mounted.push(wrapper)
    const image = wrapper.get('img').element
    const attributes = wrapper.get('img').attributes()
    for (const visualState of ['running', 'needs-user', 'confirming', 'ended', 'failed'] as const) {
      state.value = visualState
      await nextTick()
      expect(wrapper.get('img').element).toBe(image)
      expect(wrapper.get('img').attributes()).toEqual(attributes)
    }
    const claude = readFileSync(resolve('src/assets/icons/cli/claude.svg'), 'utf8')
    const codex = readFileSync(resolve('src/assets/icons/cli/codex.svg'), 'utf8')
    expect(claude).not.toBe(codex)
  })

  // 捆绑 SVG 只包含静态图形；品牌图标与自有状态图标分别记录来源。
  it('Icons_StaticAssets_025', () => {
    for (const directory of ['cli', 'session-status']) {
      const notice = readFileSync(resolve(`src/assets/icons/${directory}/README.md`), 'utf8')
      expect(notice).toContain('CC Desk')
      if (directory === 'session-status') expect(notice).toContain('MIT')
      else {
        expect(notice).toContain('Anthropic')
        expect(notice).toContain('OpenAI')
        expect(notice).toContain('https://claude.com/')
        expect(notice).toContain('https://openai.com/brand/')
        expect(notice).toContain('Blossom_Light.svg')
        expect(notice).not.toContain('self-owned project artwork')
      }
      const names = directory === 'cli' ? ['claude', 'codex'] : ['starting', 'running', 'working', 'permission', 'completed', 'stopped', 'closed', 'unknown', 'needs-user', 'confirming', 'ended', 'failed', 'thinking', 'tool-executing', 'subagent', 'compacting', 'waiting-input', 'archived']
      for (const name of names) {
        const source = readFileSync(resolve(`src/assets/icons/${directory}/${name}.svg`), 'utf8')
        const svg = new DOMParser().parseFromString(source, 'image/svg+xml').documentElement
        expect(svg.nodeName).toBe('svg')
        expect(svg.querySelector('parsererror, text, script, foreignObject, image, use, animate, animateTransform, style')).toBeNull()
        for (const element of [svg, ...Array.from(svg.querySelectorAll('*'))]) {
          expect(Array.from(element.attributes).some((attribute) => /^on|href$|^style$/i.test(attribute.name))).toBe(false)
        }
      }
    }
  })

  // Codex 黑白图标在双主题行背景和选中叠色上至少达到 3:1；Claude 保留原始品牌色。
  it('Cli_ThemeContrast_026', () => {
    const globalCss = readFileSync(resolve('src/styles/global.css'), 'utf8')
    const component = readFileSync(resolve('src/components/sessions/CliAppIcon.vue'), 'utf8')
    const styles = document.createElement('style')
    styles.dataset.testSessionIcons = ''
    const compiled = compileStyle({ filename: 'CliAppIcon.vue', source: component.match(/<style[^>]*>([\s\S]*?)<\/style>/)![1], id: (CliAppIcon as unknown as { __scopeId: string }).__scopeId, scoped: true })
    expect(compiled.errors).toEqual([])
    styles.textContent = globalCss + '\n' + compiled.code
    document.head.append(styles)
    const wrapper = mount(CliAppIcon, { attachTo: document.body, props: { cli: 'codex' } })
    const claudeWrapper = mount(CliAppIcon, { attachTo: document.body, props: { cli: 'claude' } })
    mounted.push(wrapper, claudeWrapper)
    const luminance = (rgb: number[]) => rgb.map((channel) => channel / 255).map((channel) => channel <= .04045 ? channel / 12.92 : ((channel + .055) / 1.055) ** 2.4).reduce((sum, channel, index) => sum + channel * [.2126, .7152, .0722][index], 0)
    for (const theme of ['light', 'dark']) {
      document.documentElement.dataset.theme = theme
      const tokens = Object.fromEntries(Array.from(globalCss.match(theme === 'dark' ? /\[data-theme="dark"\]\s*\{([^}]*)\}/ : /:root\s*\{([^}]*)\}/)![1].matchAll(/--([\w-]+):\s*([^;]+);/g), (match) => [match[1], match[2].trim()]))
      const filter = getComputedStyle(wrapper.get('img').element).filter
      expect(filter).toBe(theme === 'dark' ? 'invert(1)' : 'none')
      expect(getComputedStyle(claudeWrapper.get('img').element).filter, 'Claude brand color must not be brightened or inverted').toBe('none')
      const foreground = theme === 'dark' ? [255, 255, 255] : [0, 0, 0]
      const selected = tokens['selected-bg'].match(/[\d.]+/g)!.map(Number)
      for (const name of ['bg-primary', 'bg-secondary', 'bg-tertiary', 'bg-hover']) {
        const hex = tokens[name]
        const background = [1, 3, 5].map((offset) => parseInt(hex.slice(offset, offset + 2), 16))
        const selectedBackground = background.map((channel, index) => Math.round(selected[index] * selected[3] + channel * (1 - selected[3])))
        for (const [surface, rgb] of [[name, background], [`selected over ${name}`, selectedBackground]] as const) {
          const fg = luminance(foreground)
          const bg = luminance(rgb)
          const ratio = (Math.max(fg, bg) + .05) / (Math.min(fg, bg) + .05)
          expect(ratio, `${theme} CLI ink on ${surface}: ${ratio.toFixed(4)}:1`).toBeGreaterThanOrEqual(3)
        }
      }
    }
  })

  // Codex 按用户偏好采用官方 ChatGPT/OpenAI 花结；防止回退到终端字形或自绘图案。
  it.each([
    { cli: 'claude', viewBox: '0 0 125 125', hash: '055f133268cfc756c83c8731e02b234d522d27bdb7745bb46eb5439de61cc7dc' },
    { cli: 'codex', viewBox: '146.694 227.042 267.198 264.812', hash: 'fb0a32a5384df5cdacc7d1a304f5b14df3c38a9a41ed08878adc99653efd0a33' },
  ])('Cli_OfficialGeometry_$cli', ({ cli, viewBox, hash }) => {
    const source = readFileSync(resolve(`src/assets/icons/cli/${cli}.svg`), 'utf8')
    const svg = new DOMParser().parseFromString(source, 'image/svg+xml').documentElement
    expect(svg.getAttribute('viewBox')).toBe(viewBox)
    const paths = Array.from(svg.querySelectorAll('path'))
    expect(paths).toHaveLength(1)
    expect(createHash('sha256').update(paths[0].getAttribute('d')!).digest('hex')).toBe(hash)
    if (cli === 'claude') expect(paths[0].getAttribute('fill')).toBe('#D97757')
    else expect(paths[0].getAttribute('fill')).toBe('black')
  })

  // 编译后的 scoped CSS 必须直接覆盖图标自身变量，不能只在主题祖先定义。
  it('Status_CompiledThemeCascade_034', () => {
    const component = readFileSync(resolve('src/components/sessions/SessionStatusIcon.vue'), 'utf8')
    const style = document.createElement('style')
    style.dataset.testSessionIcons = ''
    style.textContent = compileStyle({ filename: 'SessionStatusIcon.vue', source: component.match(/<style[^>]*>([\s\S]*?)<\/style>/)![1], id: (SessionStatusIcon as unknown as { __scopeId: string }).__scopeId, scoped: true }).code
    document.head.append(style)
    const wrapper = mount(SessionStatusIcon, { attachTo: document.body, props: { state: 'starting' }, global: { plugins: [i18n] } })
    mounted.push(wrapper)
    for (const [theme, info, success, error] of [['light', '#2a5082', '#367e63', '#c45c4a'], ['dark', '#82acdc', '#5dad8e', '#f28a78']]) {
      document.documentElement.setAttribute('data-theme', theme)
      const actual = getComputedStyle(wrapper.get('.session-status-icon').element)
      expect(actual.getPropertyValue('--session-status-info').trim()).toBe(info)
      expect(actual.getPropertyValue('--session-status-success').trim()).toBe(success)
      expect(actual.getPropertyValue('--session-status-error').trim()).toBe(error)
    }
  })

  // 实心底与对比符号在双主题及选中叠色上达到 3:1，工作动效不降低透明度。
  it('Status_FilledGlyphContrast_027', () => {
    const globalCss = readFileSync(resolve('src/styles/global.css'), 'utf8')
    const component = readFileSync(resolve('src/components/sessions/SessionStatusIcon.vue'), 'utf8')
    const base = component.match(/\.session-status-icon\s*\{([^}]*)\}/)![1]
    const dark = component.match(/\[data-theme="dark"\] \.session-status-icon\s*\{([^}]*)\}/)![1]
    const luminance = (rgb: number[]) => rgb.map(channel => channel / 255).map(channel => channel <= .04045 ? channel / 12.92 : ((channel + .055) / 1.055) ** 2.4).reduce((sum, channel, index) => sum + channel * [.2126, .7152, .0722][index], 0)
    const rgb = (hex: string) => [1, 3, 5].map(offset => parseInt(hex.slice(offset, offset + 2), 16))
    const contrast = (left: number[], right: number[]) => (Math.max(luminance(left), luminance(right)) + .05) / (Math.min(luminance(left), luminance(right)) + .05)
    for (const theme of ['light', 'dark']) {
      const tokens = Object.fromEntries(Array.from(globalCss.match(theme === 'dark' ? /\[data-theme="dark"\]\s*\{([^}]*)\}/ : /:root\s*\{([^}]*)\}/)![1].matchAll(/--([\w-]+):\s*([^;]+);/g), match => [match[1], match[2].trim()]))
      Object.assign(tokens, Object.fromEntries(Array.from((base + (theme === 'dark' ? dark : '')).matchAll(/--([\w-]+):\s*([^;]+);/g), match => [match[1], match[2].trim()])))
      const selection = tokens['selected-bg'].match(/[\d.]+/g)!.map(Number)
      for (const ink of ['text-secondary', 'session-status-info', 'session-status-success', 'session-status-error', 'accent-gold-text']) {
        expect(contrast(rgb(tokens[ink]), rgb(tokens['bg-primary'])), `${theme} ${ink} symbol/backplate`).toBeGreaterThanOrEqual(3)
        if (ink === 'session-status-success') {
          const opacity = Number(component.match(/@keyframes session-thinking-dot[^}]*opacity:\s*([\d.]+)/)![1])
          const dimDot = rgb(tokens['bg-primary']).map((channel, index) => Math.round(channel * opacity + rgb(tokens[ink])[index] * (1 - opacity)))
          expect(contrast(dimDot, rgb(tokens[ink])), `${theme} thinking dot at dim trough`).toBeGreaterThanOrEqual(3)
        }
        for (const surface of ['bg-primary', 'bg-secondary', 'bg-tertiary', 'bg-hover']) {
          const background = rgb(tokens[surface])
          const selected = background.map((channel, index) => Math.round(selection[index] * selection[3] + channel * (1 - selection[3])))
          expect(contrast(rgb(tokens[ink]), background), `${theme} ${ink} on ${surface}`).toBeGreaterThanOrEqual(3)
          expect(contrast(rgb(tokens[ink]), selected), `${theme} ${ink} selected ${surface}`).toBeGreaterThanOrEqual(3)
        }
      }
    }
  })

})
