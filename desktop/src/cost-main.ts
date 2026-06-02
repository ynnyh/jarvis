import { createApp } from 'vue'
import { createPinia } from 'pinia'
import CostApp from './CostApp.vue'
import './style.css'

const app = createApp(CostApp)
app.use(createPinia())
app.mount('#app')
