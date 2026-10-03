import { createApp } from 'vue'
import { createPinia } from 'pinia'
import i18n from '@/i18n'
import VisualFixtureApp from './VisualFixtureApp.vue'
import '@/styles/global.css'
import './fixture.css'

// Defense in depth if a future entry accidentally imports the fixture elsewhere.
if (!import.meta.env.DEV || import.meta.env.MODE !== 'visual' || location.pathname !== '/__visual__/') throw new Error('VISUAL_FIXTURE_DISABLED')
createApp(VisualFixtureApp).use(createPinia()).use(i18n).mount('#app')
