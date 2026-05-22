import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  const token = tokenRes.data.token
  tokenManager.setToken(token)
  console.log('Token OK\n')

  // 测试各种可能的"用户视角"接口
  const endpoints = [
    // 直接用户任务接口
    { method: 'GET', url: '/api.php/v1/tasks', params: { assignedTo: 'REDACTED_ACCOUNT' }, desc: 'GET /tasks?assignedTo=REDACTED_ACCOUNT' },
    { method: 'GET', url: '/api.php/v1/tasks', params: { assignedTo: 'REDACTED_ACCOUNT', status: 'wait,doing' }, desc: 'GET /tasks?assignedTo=REDACTED_ACCOUNT&status=wait,doing' },
    { method: 'GET', url: '/api.php/v1/tasks', params: { assignedTo: 'REDACTED_ACCOUNT', status: 'undone' }, desc: 'GET /tasks?assignedTo=REDACTED_ACCOUNT&status=undone' },

    // my 相关接口
    { method: 'GET', url: '/api.php/v1/my-tasks', params: {}, desc: 'GET /my-tasks' },
    { method: 'GET', url: '/api.php/v1/mytasks', params: {}, desc: 'GET /mytasks' },
    { method: 'GET', url: '/api.php/v1/my/tasks', params: {}, desc: 'GET /my/tasks' },

    // user 相关接口
    { method: 'GET', url: '/api.php/v1/users/REDACTED_ACCOUNT/tasks', params: {}, desc: 'GET /users/REDACTED_ACCOUNT/tasks' },
    { method: 'GET', url: '/api.php/v1/user/tasks', params: {}, desc: 'GET /user/tasks' },

    // todo 接口
    { method: 'GET', url: '/api.php/v1/todos', params: {}, desc: 'GET /todos' },
    { method: 'GET', url: '/api.php/v1/todo', params: {}, desc: 'GET /todo' },

    // assignedtome
    { method: 'GET', url: '/api.php/v1/tasks', params: { status: 'assignedtome' }, desc: 'GET /tasks?status=assignedtome' },

    // v2 接口
    { method: 'GET', url: '/api.php/v2/tasks', params: { assignedTo: 'REDACTED_ACCOUNT' }, desc: 'GET v2/tasks?assignedTo=REDACTED_ACCOUNT' },
    { method: 'GET', url: '/api.php/v2/my-tasks', params: {}, desc: 'GET v2/my-tasks' },
  ]

  for (const ep of endpoints) {
    console.log(`=== ${ep.desc} ===`)
    try {
      const res = await http.get(ep.url, { params: ep.params })
      const data = res.data

      // 判断返回类型
      if (Array.isArray(data)) {
        console.log(`✅ 返回数组，长度: ${data.length}`)
        if (data.length > 0) {
          console.log(`   第一条: ${JSON.stringify(data[0]).slice(0, 150)}`)
        }
      } else if (data.tasks && Array.isArray(data.tasks)) {
        console.log(`✅ 返回 tasks 数组，长度: ${data.tasks.length}`)
        if (data.tasks.length > 0) {
          console.log(`   第一条: ${JSON.stringify(data.tasks[0]).slice(0, 150)}`)
        }
      } else if (typeof data === 'object' && Object.keys(data).length > 0) {
        console.log(`✅ 返回对象，键: ${Object.keys(data).join(', ')}`)
        console.log(`   内容: ${JSON.stringify(data).slice(0, 200)}`)
      } else {
        console.log(`✅ 返回空/字符串: ${JSON.stringify(data).slice(0, 100)}`)
      }
    } catch (err: any) {
      console.log(`❌ ${err.response?.status} ${err.response?.data?.message || err.message}`)
    }
    console.log()
  }
}

test().catch(console.error)
