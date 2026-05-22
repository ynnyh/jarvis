export interface TaskTeamMember {
  account: string;
  estimate: number;
  consumed: number;
  left: number;
  status: TaskStatus | string;
}

export interface Task {
  id: string;
  title: string;
  description: string;
  status: TaskStatus;
  priority: Priority;
  estimatedHours: number;
  consumedHours: number;
  deadline: string;
  assignee: string;
  createdAt: string;
  updatedAt: string;
  comments: Comment[];
  dependencies?: string[];
  // 团队任务（mode=multi）才有；普通任务为 undefined
  mode?: 'single' | 'multi';
  team?: TaskTeamMember[];
}

export type TaskStatus = 'wait' | 'doing' | 'done' | 'closed' | 'cancel';

export type Priority = 'low' | 'normal' | 'high' | 'urgent';

export interface Comment {
  id: string;
  author: string;
  content: string;
  createdAt: string;
}

export interface TaskFilter {
  status?: TaskStatus[];
  assignee?: string;
  deadlineFrom?: string;
  deadlineTo?: string;
}

export interface RiskAnalysis {
  overdueTasks: Task[];
  highPriorityTasks: Task[];
  dependencyRisks: DependencyRisk[];
  summary: string;
}

export interface DependencyRisk {
  taskId: string;
  taskTitle: string;
  missingDependencies: string[];
  reason: string;
}

export interface ProviderConfig {
  baseUrl: string;
  apiToken?: string;
  username?: string;
  password?: string;
}
