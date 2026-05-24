import crypto from 'node:crypto';
import { BaseProvider } from './base-provider.js';
import type { Task, TaskFilter, ProviderConfig, TaskStatus, Priority, Comment } from '../shared/types.js';

interface ZenTaoTeamMember {
  account: string;
  estimate?: string | number;
  consumed?: string | number;
  left?: string | number;
  status?: string;
}

interface ZenTaoTask {
  id: number;
  name: string;
  desc?: string;
  status: string;
  pri: number;
  estimate?: number;
  consumed?: number;
  deadline?: string;
  assignedTo?: { id: number; account: string; realname: string } | string;
  openedDate?: string;
  lastEditedDate?: string;
  comments?: ZenTaoComment[];
  parent?: number;
  story?: number;
  mode?: string;                  // 'multi' 表示团队任务
  team?: ZenTaoTeamMember[];      // 团队任务的成员列表（每人独立工时）
}

interface ZenTaoComment {
  id: number;
  actor: string;
  comment: string;
  date: string;
}

export class ZenTaoProvider extends BaseProvider {
  private authenticated = false;
  private token: string | null = null;
  /**
   * 传统表单端点（task-recordWorkhour-X.json 等 PATH_INFO 路由）的会话身份。
   * 禅道 OSS 的 OpenAPI v1（/api.php/v1/*）认 Token，但表单端点只认 zentaosid cookie，
   * 两套鉴权独立——读任务可以用 token，写工时必须先 loginViaForm。
   */
  private sessionCookie: string | null = null;

  constructor(config: ProviderConfig) {
    super(config);
    // 确保 baseUrl 末尾有 /，否则 new URL('/path', baseUrl) 会丢失子路径
    if (this.config.baseUrl && !this.config.baseUrl.endsWith('/')) {
      this.config.baseUrl += '/';
    }
    // 如果配置里手动塞了 zentaosid（应急通道），直接当作已登录
    if (this.config.sessionCookie) {
      this.sessionCookie = this.config.sessionCookie;
    }
  }

  getName(): string {
    return 'zentao';
  }

  async authenticate(): Promise<boolean> {
    try {
      const url = new URL('api.php/v1/tokens', this.config.baseUrl);
      const response = await fetch(url.toString(), {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
        },
        body: JSON.stringify({
          account: this.config.username,
          password: this.config.password,
        }),
      });
      if (!response.ok) {
        const errorBody = await response.text().catch(() => '');
        console.error(`[ZenTao] 认证失败: ${response.status} ${errorBody}`);
        this.authenticated = false;
        return false;
      }
      const data = await response.json() as { token: string; expires?: number };
      this.token = data.token;
      this.authenticated = true;
      return true;
    } catch (e) {
      console.error('[ZenTao] 认证异常:', e);
      this.authenticated = false;
      return false;
    }
  }

  async getTasks(filter?: TaskFilter): Promise<Task[]> {
    let tasks = await this.getMyTasks();
    if (filter?.status && filter.status.length > 0) {
      tasks = tasks.filter(t => filter.status!.includes(t.status));
    }
    if (filter?.assignee) {
      tasks = tasks.filter(t => t.assignee === filter.assignee);
    }
    return tasks;
  }

  async getTaskById(id: string): Promise<Task | null> {
    await this.ensureAuthenticated();
    const url = new URL(`api.php/v1/tasks/${id}`, this.config.baseUrl);
    const response = await fetch(url.toString(), {
      headers: this.getHeaders(),
    });

    if (!response.ok) {
      if (response.status === 404) return null;
      throw new Error(`ZenTao API error: ${response.status}`);
    }

    const data = await response.json() as { task: ZenTaoTask };
    return this.mapTask(data.task);
  }

  async getTodayTasks(): Promise<Task[]> {
    const allTasks = await this.getMyTasks();
    const today = new Date().toISOString().split('T')[0];
    return allTasks.filter(t => t.deadline && t.deadline.startsWith(today));
  }

  async getMyTasks(): Promise<Task[]> {
    await this.ensureAuthenticated();

    // 使用工作台 .json 端点获取"指派给我"页面的任务
    // 通过 pagerMyWork Cookie 设置每页数量，一次性获取全部任务
    const url = new URL('my-work-task-assignedTo--id_desc.json', this.config.baseUrl);
    const headers: Record<string, string> = {
      ...this.getHeaders(),
      'Cookie': 'pagerMyWork=200',
    };

    const response = await fetch(url.toString(), { headers });
    if (!response.ok) {
      throw new Error(`获取任务失败: ${response.status}`);
    }

    const json = await response.json() as any;
    if (json.status !== 'success' || !json.data) {
      throw new Error('禅道返回数据异常');
    }

    // data 是一个字符串化的 JSON
    const innerData = typeof json.data === 'string' ? JSON.parse(json.data) : json.data;
    const tasks: ZenTaoTask[] = innerData.tasks || [];

    // 过滤掉已关闭/已取消的任务
    return tasks
      .filter(t => t.status !== 'closed' && t.status !== 'cancel')
      .map(t => this.mapTask(t));
  }

  private async ensureAuthenticated(): Promise<void> {
    if (!this.authenticated || !this.token) {
      const ok = await this.authenticate();
      if (!ok) throw new Error(`ZenTao 认证失败: ${this.config.baseUrl}`);
    }
  }

  private getHeaders(): Record<string, string> {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
    };
    const token = this.token || this.config.apiToken;
    if (token) {
      headers['Token'] = token;
    }
    return headers;
  }

  /**
   * 用账号密码做一次传统表单登录，拿到 zentaosid cookie。
   *
   * 禅道 OSS 的 PATH_INFO 表单端点（如 task-recordWorkhour-X.json）**只认 session
   * cookie 不认 OpenAPI Token**。服务端拿不到登录身份时不会返回 401，而是给一份
   * 登录页 JSON 假装成功——所以只有走 form login 才能真写工时。
   *
   * 完整流程（实测 v20.7.1，对应禅道 v18+ "passwordStrength" 模式）：
   *  1. GET /user-login.html —— 拿初始 zentaosid（PHP session 在 GET 时分配）
   *  2. **GET /user-refreshRandom.html** —— 服务端把新 rand 塞进 session **并**返回给前端
   *     这一步是关键：之前从 HTML 解析 verifyRand 拿到的是占位值，跟 session 里
   *     实际存的对不上 → md5 算法对但盐错 → 永远验不过
   *  3. password = md5(md5(plain) + rand)
   *  4. POST /user-login.html（带 cookie，否则 verifyRand 对不上 session）
   *  5. verify：用拿到的 cookie GET /my.html，**不是工作台就抛错**
   *     (未登录态也会下发 zentaosid，光检查 "有 cookie" 不够)
   *
   * `redirect: 'manual'` —— 登录成功是 302 跳 /，跟随会让 fetch 丢 Set-Cookie。
   */
  private async loginViaForm(): Promise<void> {
    if (!this.config.username || !this.config.password) {
      throw new Error('禅道 session 登录失败：账号或密码为空（检查 keychain 与 ~/.jarvis/config.json）');
    }

    const ua = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36';
    const loginUrl = new URL('user-login.html', this.config.baseUrl);

    // 1. GET 登录页拿初始 session
    const pageRes = await fetch(loginUrl.toString(), {
      headers: { 'User-Agent': ua },
      redirect: 'manual',
    });

    let initialSid = '';
    for (const sc of this.extractSetCookies(pageRes)) {
      const m = sc.match(/zentaosid=([^;]+)/);
      if (m) { initialSid = m[1]; break; }
    }

    // 2. GET refreshRandom —— 这一步让服务端把新 rand 塞进 session，**必须**用同一个 cookie
    const randUrl = new URL('user-refreshRandom.html', this.config.baseUrl);
    const randRes = await fetch(randUrl.toString(), {
      headers: {
        'User-Agent': ua,
        'X-Requested-With': 'XMLHttpRequest',
        'Accept': 'application/json, text/javascript, */*; q=0.01',
        'Referer': loginUrl.toString(),
        ...(initialSid ? { 'Cookie': `zentaosid=${initialSid}` } : {}),
      },
      redirect: 'manual',
    });
    const randText = await randRes.text();
    // 响应可能是纯数字 "12345"、JSON `{"rand":12345}`、HTML 等，用正则抓第一串连续数字
    const randMatch = randText.match(/(\d+)/);
    if (!randMatch) {
      throw new Error(
        `禅道 session 登录失败：refreshRandom 响应无可识别的 rand。HTTP ${randRes.status}，前 200 字: ${randText.slice(0, 200)}`
      );
    }
    const verifyRand = randMatch[1];

    // refreshRandom 也可能换 cookie，更新一下
    for (const sc of this.extractSetCookies(randRes)) {
      const m = sc.match(/zentaosid=([^;]+)/);
      if (m) { initialSid = m[1]; break; }
    }

    // 3. md5 加密密码（用真正 session 里的 rand）
    const md5 = (s: string) => crypto.createHash('md5').update(s).digest('hex');
    const encryptedPwd = md5(md5(this.config.password) + verifyRand);

    // 4. POST 登录（带同一个 cookie 保证 verifyRand 与 session 一致）
    const enc = encodeURIComponent;
    const body =
      `account=${enc(this.config.username)}` +
      `&password=${encryptedPwd}` +
      `&passwordStrength=1` +
      `&verifyRand=${verifyRand}` +
      `&referer=${enc('/')}`;

    const loginRes = await fetch(loginUrl.toString(), {
      method: 'POST',
      headers: {
        'Content-Type': 'application/x-www-form-urlencoded',
        'User-Agent': ua,
        'X-Requested-With': 'XMLHttpRequest',
        'Referer': loginUrl.toString(),
        ...(initialSid ? { 'Cookie': `zentaosid=${initialSid}` } : {}),
      },
      body,
      redirect: 'manual',
    });

    // 拿登录后 cookie（多数版本不换，保险起见还是读一次）
    let zentaosid = initialSid;
    for (const sc of this.extractSetCookies(loginRes)) {
      const m = sc.match(/zentaosid=([^;]+)/);
      if (m) { zentaosid = m[1]; break; }
    }

    if (!zentaosid) {
      const text = await loginRes.text().catch(() => '');
      throw new Error(
        `禅道 session 登录失败：响应未带 zentaosid cookie。HTTP ${loginRes.status}，前 200 字: ${text.slice(0, 200)}`
      );
    }

    // 5. verify —— 未登录态也会下发 cookie，必须实测一次
    const verifyUrl = new URL('my.html', this.config.baseUrl);
    const verifyRes = await fetch(verifyUrl.toString(), {
      headers: { 'Cookie': `zentaosid=${zentaosid}`, 'User-Agent': ua },
      redirect: 'manual',
    });

    // 30x 跳登录页 → 没登进去
    if (verifyRes.status >= 300 && verifyRes.status < 400) {
      const loc = verifyRes.headers.get('location') || '';
      if (/login/i.test(loc)) {
        throw new Error(
          `禅道 session 登录失败：访问 /my.html 被重定向到登录页 (${loc})。检查账号/密码是否正确（rand=${verifyRand}）`
        );
      }
    }
    // 200 但 body 是登录页（禅道有时返回 200 + 登录 HTML）
    if (verifyRes.status === 200) {
      const verifyText = await verifyRes.text();
      if (/id=["']userLogin["']|name=["']passwordStrength["']/.test(verifyText)) {
        throw new Error(
          `禅道 session 登录失败：/my.html 返回登录页（密码 md5 流程未通过，rand=${verifyRand}）。前 200 字: ${verifyText.slice(0, 200)}`
        );
      }
    }

    this.sessionCookie = zentaosid;
  }

  private extractSetCookies(response: Response): string[] {
    const anyHeaders = response.headers as any;
    if (typeof anyHeaders.getSetCookie === 'function') {
      return anyHeaders.getSetCookie();
    }
    const sc = response.headers.get('set-cookie');
    return sc ? [sc] : [];
  }

  private async ensureSessionLogin(): Promise<void> {
    if (!this.sessionCookie) {
      await this.loginViaForm();
    }
  }

  private getFormHeaders(): Record<string, string> {
    const headers: Record<string, string> = {
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
      // 关键：禅道服务端用这几个判断是不是浏览器 AJAX 提交。少了任何一个，
      // 它会把请求当 GET 显示表单页处理（响应里 title="工时" + 完整 task 字段
      // 就是这个迹象），实际不写库。
      'X-Requested-With': 'XMLHttpRequest',
      'Accept': 'application/json, text/javascript, */*; q=0.01',
    };
    if (this.sessionCookie) {
      headers['Cookie'] = `zentaosid=${this.sessionCookie}`;
    }
    return headers;
  }

  private mapTask(zt: ZenTaoTask): Task {
    const statusMap: Record<string, TaskStatus> = {
      wait: 'wait',
      doing: 'doing',
      done: 'done',
      closed: 'closed',
      cancel: 'cancel',
    };

    const priorityMap: Record<number, Priority> = {
      1: 'low',
      2: 'normal',
      3: 'high',
      4: 'urgent',
    };

    const assignee = typeof zt.assignedTo === 'object' ? (zt.assignedTo?.account || '') : (zt.assignedTo || '');

    const team = Array.isArray(zt.team) ? zt.team.map(m => ({
      account: m.account,
      estimate: Number(m.estimate) || 0,
      consumed: Number(m.consumed) || 0,
      left: Number(m.left) || 0,
      status: m.status || 'wait',
    })) : undefined;

    return {
      id: String(zt.id),
      title: zt.name,
      description: zt.desc || '',
      status: statusMap[zt.status] || 'wait',
      priority: priorityMap[zt.pri] || 'normal',
      estimatedHours: zt.estimate || 0,
      consumedHours: zt.consumed || 0,
      deadline: zt.deadline || '',
      assignee,
      createdAt: zt.openedDate || '',
      updatedAt: zt.lastEditedDate || '',
      comments: (zt.comments || []).map(c => this.mapComment(c)),
      dependencies: zt.parent ? [String(zt.parent)] : undefined,
      mode: zt.mode === 'multi' ? 'multi' : 'single',
      team,
    };
  }

  private mapComment(zc: ZenTaoComment): Comment {
    return {
      id: String(zc.id),
      author: zc.actor,
      content: zc.comment,
      createdAt: zc.date,
    };
  }

  /**
   * 给任务追加一条工时记录。
   *
   * 重要：禅道开源版（实测 v20.7.1）**没有 efforts 的 REST 端点**，OpenAPI v1
   * 只暴露 tasks/bugs/stories 等少数资源。工时录入只能走传统 PATH_INFO 表单：
   *
   *   POST {baseUrl}/task-recordWorkhour-{taskID}.json
   *   Content-Type: application/x-www-form-urlencoded
   *   Body: date[1]=YYYY-MM-DD&work[1]=...&consumed[1]=H&left[1]=L
   *
   * **关键 1 - 鉴权**：表单端点只认 zentaosid cookie，不认 OpenAPI Token。带
   * Token 服务端拿不到登录身份，会返回任务详情 JSON 假装成功但实际不写库
   * （表现：HTTP 200 + consumed 不变）。所以必须先 loginViaForm 拿 cookie。
   *
   * **关键 2 - 防误关**：禅道服务端用 `left[1]` 决定任务状态——填 0 任务会自动
   * 变 done/closed。用户原话："不小心关掉了任务对我会有很大的影响"。所以
   * 这里**先读当前任务的 left 原样传回**，保证 consumed 加了 0.01 但 left
   * 不变，禅道服务端看到 left>0 就维持原状态不动。
   */
  async addEffort(params: {
    taskId: string | number;
    hours: number;
    work: string;
    /** YYYY-MM-DD，默认今天本地日期 */
    date?: string;
  }): Promise<{
    id?: number;
    raw?: any;
    endpoint?: string;
    preservedLeft?: number;
    consumedBefore?: number;
    consumedAfter?: number;
    responseText?: string;
  }> {
    await this.ensureAuthenticated();   // token：用于读任务
    await this.ensureSessionLogin();    // cookie：表单端点真正写工时的身份

    // 1. 读当前任务，拿 left（防误关）+ consumed（用于事后验证）
    const taskUrl = new URL(`api.php/v1/tasks/${params.taskId}`, this.config.baseUrl);
    const taskRes = await fetch(taskUrl.toString(), { headers: this.getHeaders() });
    if (!taskRes.ok) {
      throw new Error(`读取任务 #${params.taskId} 失败（无法确认 left 值，拒绝写工时）: HTTP ${taskRes.status}`);
    }
    const taskJson = await taskRes.json() as any;
    const raw = taskJson?.task ?? taskJson;
    const currentLeft = Number(raw?.left ?? 0);
    const consumedBefore = Number(raw?.consumed ?? 0);

    // 2. 构造 form body —— 与浏览器真实请求 1:1 对齐：
    //    a) 方括号字面发送（不 url-encode），URLSearchParams 会强制 encode 所以不能用
    //    b) 发 3 组占位行（浏览器表单有 3 行批量输入，[2][3] 都是空占位）
    const date = params.date || new Date().toISOString().slice(0, 10);
    const enc = encodeURIComponent;
    const bodyParts: string[] = [
      `date[1]=${enc(date)}`,
      `work[1]=${enc(params.work)}`,
      `consumed[1]=${params.hours}`,
      `left[1]=${currentLeft}`,
    ];
    for (const i of [2, 3]) {
      bodyParts.push(`date[${i}]=${enc(date)}`);
      bodyParts.push(`work[${i}]=`);
      bodyParts.push(`consumed[${i}]=`);
      bodyParts.push(`left[${i}]=`);
    }
    const formBody = bodyParts.join('&');

    // 3. POST 到 PATH_INFO 端点（必须用 cookie；token 在这里写不进去）
    const endpoint = `task-recordWorkhour-${params.taskId}.json`;
    const url = new URL(endpoint, this.config.baseUrl);
    // Referer：浏览器从 task-view-X.html 提交，禅道有些版本会校验同源
    const refererUrl = new URL(`task-view-${params.taskId}.html`, this.config.baseUrl);
    const response = await fetch(url.toString(), {
      method: 'POST',
      headers: {
        ...this.getFormHeaders(),
        'Content-Type': 'application/x-www-form-urlencoded',
        'Referer': refererUrl.toString(),
      },
      body: formBody,
    });

    const responseText = await response.text();

    if (!response.ok) {
      throw new Error(`禅道写工时失败: ${response.status} ${responseText.slice(0, 300)} (端点 ${endpoint})`);
    }

    let data: any = {};
    try {
      data = JSON.parse(responseText);
    } catch {
      // 响应不是 JSON：可能是 HTML 登录页（session 失效）或服务端异常
      throw new Error(
        `禅道写工时失败：返回非 JSON（session 可能已过期）。前 300 字: ${responseText.slice(0, 300)}`
      );
    }

    if (data?.result === 'fail') {
      throw new Error(`禅道写工时失败: ${data.message || JSON.stringify(data).slice(0, 300)}`);
    }

    // 4. **verify-after-write**：禅道服务端在 token 失效时会"假成功"——返回正常
    //    响应但实际未写入。再 GET 一次对比 consumed 是 100% 能识别的兜底。
    let consumedAfter = consumedBefore;
    try {
      const verifyRes = await fetch(taskUrl.toString(), { headers: this.getHeaders() });
      if (verifyRes.ok) {
        const verifyJson = await verifyRes.json() as any;
        const verifyRaw = verifyJson?.task ?? verifyJson;
        consumedAfter = Number(verifyRaw?.consumed ?? consumedBefore);
      }
    } catch {
      // verify 失败不阻塞主流程，但也不能误判成功 —— 抛错
      throw new Error('写入响应正常但验证读取失败，无法确认是否真写入');
    }

    const actualDelta = consumedAfter - consumedBefore;
    const expectedDelta = params.hours;
    if (Math.abs(actualDelta - expectedDelta) > 0.001) {
      throw new Error(
        `禅道服务端返回成功但实际未生效。consumed: ${consumedBefore}h → ${consumedAfter}h（预期 +${expectedDelta}h，实际 +${actualDelta}h）。响应: ${responseText.slice(0, 200)}`
      );
    }

    return {
      id: data?.id,
      raw: data,
      endpoint,
      preservedLeft: currentLeft,
      consumedBefore,
      consumedAfter,
      responseText: responseText.slice(0, 500),
    };
  }
}
