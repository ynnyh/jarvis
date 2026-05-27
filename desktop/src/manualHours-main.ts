import { createApp } from 'vue'
import { createPinia } from 'pinia'
import ManualHoursApp from './ManualHoursApp.vue'
import './style.css'

const app = createApp(ManualHoursApp)
app.use(createPinia())
app.mount('#app')
