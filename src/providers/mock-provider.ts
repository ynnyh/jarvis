import { BaseProvider } from './base-provider.js';
import type { Task, TaskFilter, ProviderConfig } from '../shared/types.js';

export class MockProvider extends BaseProvider {
  private mockTasks: Task[] = [
    {
      id: '101',
      title: '完成用户登录模块开发',
      description: '实现基于 JWT 的用户认证系统，包括登录、注册、找回密码功能。',
      status: 'doing',
      priority: 'high',
      estimatedHours: 16,
      consumedHours: 8,
      deadline: '2026-05-15',
      assignee: '张三',
      createdAt: '2026-05-01T09:00:00Z',
      updatedAt: '2026-05-10T14:30:00Z',
      comments: [
        { id: '1', author: '李四', content: '登录接口已联调通过', createdAt: '2026-05-08T10:00:00Z' },
      ],
      dependencies: ['100'],
    },
    {
      id: '102',
      title: '订单列表页面优化',
      description: '优化订单列表加载性能，支持分页和筛选。',
      status: 'wait',
      priority: 'urgent',
      estimatedHours: 8,
      consumedHours: 0,
      deadline: '2026-05-14',
      assignee: '张三',
      createdAt: '2026-05-05T09:00:00Z',
      updatedAt: '2026-05-05T09:00:00Z',
      comments: [],
    },
    {
      id: '103',
      title: '修复支付回调漏洞',
      description: '支付回调接口存在重复通知处理不当的问题，需要修复。',
      status: 'doing',
      priority: 'urgent',
      estimatedHours: 4,
      consumedHours: 2,
      deadline: '2026-05-13',
      assignee: '张三',
      createdAt: '2026-05-10T09:00:00Z',
      updatedAt: '2026-05-11T16:00:00Z',
      comments: [
        { id: '2', author: '王五', content: '已复现问题，正在修复', createdAt: '2026-05-11T16:00:00Z' },
      ],
    },
    {
      id: '104',
      title: '编写 API 接口文档',
      description: '使用 Swagger 编写所有后端接口文档。',
      status: 'wait',
      priority: 'normal',
      estimatedHours: 12,
      consumedHours: 0,
      deadline: '2026-05-20',
      assignee: '张三',
      createdAt: '2026-05-08T09:00:00Z',
      updatedAt: '2026-05-08T09:00:00Z',
      comments: [],
    },
    {
      id: '105',
      title: '数据库迁移脚本',
      description: '编写 v2.0 版本数据库迁移脚本。',
      status: 'done',
      priority: 'high',
      estimatedHours: 6,
      consumedHours: 6,
      deadline: '2026-05-10',
      assignee: '张三',
      createdAt: '2026-05-02T09:00:00Z',
      updatedAt: '2026-05-09T18:00:00Z',
      comments: [],
    },
  ];

  constructor(config: ProviderConfig) {
    super(config);
  }

  getName(): string {
    return 'mock';
  }

  async authenticate(): Promise<boolean> {
    return true;
  }

  async getTasks(filter?: TaskFilter): Promise<Task[]> {
    let tasks = this.mockTasks;
    if (filter?.status && filter.status.length > 0) {
      tasks = tasks.filter(t => filter.status!.includes(t.status));
    }
    if (filter?.assignee) {
      tasks = tasks.filter(t => t.assignee === filter.assignee);
    }
    return tasks;
  }

  async getTaskById(id: string): Promise<Task | null> {
    return this.mockTasks.find(t => t.id === id) || null;
  }

  async getTodayTasks(): Promise<Task[]> {
    const today = new Date().toISOString().split('T')[0];
    return this.mockTasks.filter(t => t.deadline === today);
  }

  async getMyTasks(): Promise<Task[]> {
    return this.mockTasks.filter(t => t.status !== 'closed' && t.status !== 'cancel');
  }
}
