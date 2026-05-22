import axios from 'axios'
import { wrapper } from 'axios-cookiejar-support'
import { CookieJar } from 'tough-cookie'
import * as cheerio from 'cheerio'
import dotenv from 'dotenv'

dotenv.config()

const BASE_URL = process.env.ZENTAO_BASE_URL || ''
const ACCOUNT = process.env.ZENTAO_ACCOUNT || ''
const PASSWORD = process.env.ZENTAO_PASSWORD || ''

async function main() {
  const jar = new CookieJar()
  const http = wrapper(axios.create({
    baseURL: BASE_URL,
    timeout: 30000,
    jar,
    withCredentials: true,
    headers: { 'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36' },
  }))

  // 获取登录页
  const loginPage = await http.get('/user-login.html')
  const $ = cheerio.load(loginPage.data)

  // 打印所有 form 信息
  $('form').each((i, el) => {
    const $form = $(el)
    console.log(`Form #${i}:`)
    console.log('  action:', $form.attr('action'))
    console.log('  method:', $form.attr('method'))
    $form.find('input').each((j, input) => {
      const $input = $(input)
      console.log(`  input: name=${$input.attr('name')} type=${$input.attr('type')} value=${$input.attr('value')?.slice(0,20)}`)
    })
    console.log()
  })

  // 提取 tokenVerify 或 hidden fields
  const tokenVerify = $('input[name="tokenVerify"]').attr('value')
  console.log('tokenVerify:', tokenVerify)

  // 尝试用 json 格式登录
  console.log('\n--- 尝试 JSON 登录 ---')
  try {
    const res = await http.post('/user-login.html', {
      account: ACCOUNT,
      password: PASSWORD,
      keepLogin: 'on',
    }, {
      headers: { 'X-Requested-With': 'XMLHttpRequest' },
    })
    console.log('Status:', res.status)
    console.log('Response:', typeof res.data === 'string' ? res.data.slice(0, 500) : JSON.stringify(res.data).slice(0, 500))
  } catch (e: any) {
    console.log('JSON login error:', e.response?.status, e.response?.data?.toString().slice(0, 200))
  }

  // 尝试用 form-urlencoded 登录，带所有 hidden fields
  console.log('\n--- 尝试 form-urlencoded 登录 ---')
  try {
    const params = new URLSearchParams()
    params.append('account', ACCOUNT)
    params.append('password', PASSWORD)
    params.append('keepLogin', 'on')
    if (tokenVerify) params.append('tokenVerify', tokenVerify)

    const res = await http.post('/user-login.html', params.toString(), {
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      maxRedirects: 0,
      validateStatus: (s) => s < 400,
    })
    console.log('Status:', res.status)
    console.log('Location:', res.headers['location'] || 'none')
    console.log('Response preview:', (typeof res.data === 'string' ? res.data : '').slice(0, 300))
  } catch (e: any) {
    console.log('Form login error:', e.response?.status)
    if (e.response?.headers?.location) console.log('Redirect to:', e.response.headers.location)
  }
}

main().catch(console.error)
