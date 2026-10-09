import { createApp } from 'vue'
import { createI18n } from 'vue-i18n'
import VersionManagerApp from './VersionManagerApp.vue'
import '../styles/global.css'

// This maintenance document never starts the ordinary App or its stores.
const locale = navigator.language.toLowerCase().startsWith('zh') ? 'zh' : 'en'
const i18n = createI18n({ legacy: false, locale, fallbackLocale: 'en', messages: {
  en: { close: 'Close' }, zh: { close: '关闭' },
} })
createApp(VersionManagerApp).use(i18n).mount('#app')
