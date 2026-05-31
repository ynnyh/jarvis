import { createApp } from 'vue'
import { createPinia } from 'pinia'
import TodayPlanApp from './TodayPlanApp.vue'
import './style.css'

const app = createApp(TodayPlanApp)
app.use(createPinia())
app.mount('#app')
