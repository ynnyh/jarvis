import { createApp } from 'vue'
import { createPinia } from 'pinia'
import SettingsDetailApp from './SettingsDetailApp.vue'
import './style.css'

const app = createApp(SettingsDetailApp)
app.use(createPinia())
app.mount('#app')
