import { http, tokenManager, ACCOUNT, PASSWORD } from '../src/providers/zentao/request.js'

async function test() {
  const tokenRes = await http.post('/api.php/v1/tokens', { account: ACCOUNT, password: PASSWORD })
  tokenManager.setToken(tokenRes.data.token)
  console.log('Token OK')

  // 1. 试试不加 assignedTo，看有没有任务
  console.log('\n=== 所有任务(前5) ===')
  const all = await http.get('/api.php/v2/tasks', { params: { recPerPage: 5 } })
  console.log('任务数:', all.data.tasks?.length || 0)
  if (all.data.tasks?.length > 0) {
    console.log('第一个任务:', JSON.stringify(all.data.tasks[0], null, 2))
  } else {
    console.log('响应结构:', Object.keys(all.data))
  }

  // 2. 试试 projects
  console.log('\n=== 项目列表 ===')
  const projects = await http.get('/api.php/v2/projects', { params: { recPerPage: 5 } })
  console.log('项目数:', projects.data.projects?.length || 0)
  if (projects.data.projects?.length > 0) {
    console.log('第一个项目:', projects.data.projects[0].name)
  } else {
    console.log('响应结构:', Object.keys(projects.data))
  }

  // 3. 试试 executions
  console.log('\n=== 执行列表 ===')
  const execs = await http.get('/api.php/v2/executions', { params: { recPerPage: 5 } })
  console.log('执行数:', execs.data.executions?.length || 0)
  if (execs.data.executions?.length > 0) {
    console.log('第一个执行:', execs.data.executions[0].name)
  } else {
    console.log('响应结构:', Object.keys(execs.data))
  }

  // 4. 试试 products
  console.log('\n=== 产品列表 ===')
  const products = await http.get('/api.php/v2/products', { params: { recPerPage: 5 } })
  console.log('产品数:', products.data.products?.length || 0)
  if (products.data.products?.length > 0) {
    console.log('第一个产品:', products.data.products[0].name)
  } else {
    console.log('响应结构:', Object.keys(products.data))
  }
}

test().catch(console.error)
