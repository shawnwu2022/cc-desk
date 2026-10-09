import { createApp } from 'vue'
import { createI18n } from 'vue-i18n'
import ManagerVisualFixtureApp from './ManagerVisualFixtureApp.vue'
import '../styles/global.css'

if (!import.meta.env.DEV || import.meta.env.MODE !== 'visual' || location.pathname !== '/__visual__/version-manager/') throw new Error('VISUAL_FIXTURE_DISABLED')
const requestedLocale = new URLSearchParams(location.search).get('locale') ?? 'en'
if (!['en', 'zh'].includes(requestedLocale)) throw new Error('VISUAL_MANAGER_SCENARIO_INVALID')
const i18n = createI18n({ legacy: false, locale: requestedLocale, fallbackLocale: 'en', messages: { en: { close: 'Close' }, zh: { close: '关闭' } } })
createApp(ManagerVisualFixtureApp).use(i18n).mount('#app')
