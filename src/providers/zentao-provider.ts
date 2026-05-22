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

  constructor(config: ProviderConfig) {
    super(config);
    // 确保 baseUrl 末尾有 /，否则 new URL('/path', baseUrl) 会丢失子路径
    if (this.config.baseUrl && !this.config.baseUrl.endsWith('/')) {
      this.config.baseUrl += '/';
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
}
