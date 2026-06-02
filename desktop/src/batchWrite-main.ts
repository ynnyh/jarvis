import { createApp } from 'vue'
import { createPinia } from 'pinia'
import BatchWriteApp from './BatchWriteApp.vue'
import './style.css'

const app = createApp(BatchWriteApp)
app.use(createPinia())
app.mount('#app')
