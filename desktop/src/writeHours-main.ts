import { createApp } from 'vue'
import { createPinia } from 'pinia'
import WriteHoursApp from './WriteHoursApp.vue'
import './style.css'

const app = createApp(WriteHoursApp)
app.use(createPinia())
app.mount('#app')
