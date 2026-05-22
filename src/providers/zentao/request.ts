import axios, { AxiosInstance, AxiosRequestConfig, AxiosResponse } from 'axios'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

if (!BASE_URL || !ACCOUNT || !PASSWORD) {
  console.error('[ZenTao] 错误: 缺少环境变量 ZENTAO_BASE_URL / ZENTAO_ACCOUNT / ZENTAO_PASSWORD')
}

class TokenManager {
  private token: string | null = null
  private expiresAt: number = 0

  getToken(): string | null {
    if (this.token && Date.now() < this.expiresAt) {
      return this.token
    }
    return null
  }

  setToken(token: string, expiresInSeconds = 7200) {
    this.token = token
    this.expiresAt = Date.now() + expiresInSeconds * 1000
    console.log(`[ZenTao] Token 已缓存，有效期 ${expiresInSeconds} 秒`)
  }

  clear() {
    this.token = null
    this.expiresAt = 0
    console.log('[ZenTao] Token 已清除')
  }
}

export const tokenManager = new TokenManager()

function createAxiosInstance(): AxiosInstance {
  const instance = axios.create({
    baseURL: BASE_URL,
    timeout: 15000,
    headers: {
      'Content-Type': 'application/json',
    },
  })

  instance.interceptors.request.use(
    (config) => {
      const token = tokenManager.getToken()
      if (token) {
        config.headers = config.headers || {}
        config.headers['Token'] = token
      }
      console.log(`[ZenTao] Request: ${config.method?.toUpperCase()} ${config.url}`)
      return config
    },
    (error) => {
      console.error('[ZenTao] Request Error:', error.message)
      return Promise.reject(error)
    }
  )

  instance.interceptors.response.use(
    (response: AxiosResponse) => {
      console.log(`[ZenTao] Response: ${response.status} ${response.config.url}`)
      return response
    },
    (error) => {
      if (error.response) {
        console.error(`[ZenTao] Response Error: ${error.response.status} ${error.response.config?.url}`)
        console.error('[ZenTao] Error Data:', JSON.stringify(error.response.data, null, 2))
      } else if (error.request) {
        console.error('[ZenTao] No Response:', error.message)
      } else {
        console.error('[ZenTao] Error:', error.message)
      }
      return Promise.reject(error)
    }
  )

  return instance
}

export const http = createAxiosInstance()
export { BASE_URL, ACCOUNT, PASSWORD }
