import { BaseProvider } from '../providers/base-provider.js';
import type { Task, TaskFilter } from '../shared/types.js';

export class TaskService {
  private provider: BaseProvider;

  constructor(provider: BaseProvider) {
    this.provider = provider;
  }

  async getAllTasks(filter?: TaskFilter): Promise<Task[]> {
    return this.provider.getTasks(filter);
  }

  async getTodayTasks(): Promise<Task[]> {
    return this.provider.getTodayTasks();
  }

  async getMyTasks(): Promise<Task[]> {
    return this.provider.getMyTasks();
  }

  async getTaskDetail(id: string): Promise<Task | null> {
    return this.provider.getTaskById(id);
  }
}
