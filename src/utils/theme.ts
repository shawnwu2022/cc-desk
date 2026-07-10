/**
 * 应用 GUI 主题（light/dark）到 DOM：设置 data-theme 属性并切换 light/dark class。
 *
 * 集中这一逻辑，是因为 GUI 主题到 DOM 的同步发生在多处，且曾因「loadAppConfig 异步加载
 * 持久化值后没人同步到 DOM」导致重启后应用配色不生效的 bug：
 *  - App.vue initAfterChecks：启动时用 store 初始值（'light'）先设 DOM，避免首屏闪烁
 *  - App.vue watch(appStore.theme)：loadAppConfig 完成后把持久化值同步到 DOM
 *  - stores/app.ts setTheme：设置页实时切换
 * 任一处都必须走本函数，保证 data-theme 属性与 class 始终一致。
 */
export function applyThemeToDom(theme: string): void {
  const html = document.documentElement
  html.setAttribute('data-theme', theme)
  html.classList.remove('light', 'dark')
  html.classList.add(theme)
}
