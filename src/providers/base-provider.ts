import type { Task, TaskFilter, ProviderConfig } from '../shared/types.js';

export abstract class BaseProvider {
  protected config: ProviderConfig;

  constructor(config: ProviderConfig) {
    this.config = config;
  }

  abstract getName(): string;

  abstract authenticate(): Promise<boolean>;

  abstract getTasks(filter?: TaskFilter): Promise<Task[]>;

  abstract getTaskById(id: string): Promise<Task | null>;

  abstract getTodayTasks(): Promise<Task[]>;

  abstract getMyTasks(): Promise<Task[]>;
}
