import axios from 'axios'
import { wrapper } from 'axios-cookiejar-support'
import { CookieJar } from 'tough-cookie'
import * as cheerio from 'cheerio'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function debug() {
  console.log('=====================================')
  console.log('  禅道验证码调试')
  console.log('=====================================')
  console.log()

  const jar = new CookieJar()
  const http = wrapper(axios.create({
    baseURL: BASE_URL,
    timeout: 30000,
    jar: jar,
    withCredentials: true,
    headers: {
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
    },
  }))

  try {
    // 1. 访问登录页
    console.log('【步骤 1】访问登录页...')
    const loginPage = await http.get('/user-login.html')
    const $ = cheerio.load(loginPage.data)

    // 2. 分析登录表单
    console.log('\n【步骤 2】分析登录表单...')

    const form = $('form#loginForm, form[action*="login"]').first()
    console.log('表单 action:', form.attr('action') || '未找到')
    console.log('表单 method:', form.attr('method') || '未找到')

    // 提取所有 input 字段
    console.log('\n表单字段:')
    form.find('input').each((i, el) => {
      const name = $(el).attr('name')
      const type = $(el).attr('type')
      const value = $(el).val()
      console.log(`  ${name}: type=${type}, value=${value || '空'}`)
    })

    // 3. 检查验证码
    const captchaImg = $('img#captcha, img[src*="captcha"]').first()
    if (captchaImg.length > 0) {
      console.log('\n验证码图片 src:', captchaImg.attr('src'))

      // 尝试获取验证码图片
      const captchaSrc = captchaImg.attr('src')
      if (captchaSrc) {
        const fullCaptchaUrl = captchaSrc.startsWith('http') ? captchaSrc : `${BASE_URL}${captchaSrc}`
        console.log('完整验证码 URL:', fullCaptchaUrl)

        try {
          const captchaRes = await http.get(captchaSrc, {
            responseType: 'arraybuffer',
          })
          console.log('验证码图片大小:', captchaRes.data.length, 'bytes')
        } catch (e: any) {
          console.log('获取验证码图片失败:', e.message)
        }
      }
    }

    // 4. 尝试使用 API Token 方式（绕过验证码）
    console.log('\n【步骤 3】尝试 API Token 登录...')
    try {
      const tokenRes = await http.post('/api.php/v1/tokens', {
        account: ACCOUNT,
        password: PASSWORD,
      })
      console.log('Token API 响应:', tokenRes.status)
      if (tokenRes.data.token) {
        console.log('✅ Token 获取成功:', tokenRes.data.token.slice(0, 20) + '...')

        // 尝试用 cookie 访问页面
        console.log('\n【步骤 4】用 Token Cookie 访问工作台...')
        const myRes = await http.get('/my/', {
          headers: {
            'Token': tokenRes.data.token,
          },
        })
        console.log('工作台状态:', myRes.status)
        console.log('工作台路径:', myRes.request?.path)

        if (myRes.request?.path?.includes('changePassword')) {
          console.log('⚠️ 需要修改密码')
        } else if (myRes.data.includes('工作台') || myRes.data.includes('我的地盘')) {
          console.log('✅ 成功访问工作台')
        }
      }
    } catch (e: any) {
      console.log('Token API 失败:', e.response?.status, e.message)
    }

    // 5. 检查是否有其他登录方式
    console.log('\n【步骤 5】检查登录页其他信息...')
    const scripts = $('script').map((i, el) => $(el).html()).get()
    const loginScript = scripts.find(s => s && s.includes('login'))
    if (loginScript) {
      console.log('找到登录相关脚本')
    }

  } catch (error: any) {
    console.error('\n❌ 错误:', error.message)
  }
}

debug()
