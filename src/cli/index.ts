import { Command } from 'commander';
import { ZenTaoProvider, MockProvider } from '../providers/index.js';
import { TaskService } from '../services/task-service.js';
import { getTasks, getTodayTasks, getTaskDetail, analyzeRisk } from '../tools/index.js';
import { getZentaoCredentials } from '../config/settings.js';

const program = new Command();

program
  .name('agent')
  .description('AI Project Agent - 任务助手')
  .version('1.0.0');

function createService() {
  const { baseUrl, account, password } = getZentaoCredentials();
  const apiToken = process.env.ZENTAO_TOKEN;
  const useMock = process.env.USE_MOCK === 'true';

  if (useMock) {
    return new TaskService(new MockProvider({ baseUrl: '' }));
  }

  if (!baseUrl) {
    console.error('错误：请在 Jarvis 设置里配置禅道地址（或设环境变量 ZENTAO_BASE_URL）');
    process.exit(1);
  }

  const provider = new ZenTaoProvider({ baseUrl, username: account, password, apiToken });
  return new TaskService(provider);
}

function formatTask(task: Awaited<ReturnType<typeof getTasks>>[number]) {
  const statusMap: Record<string, string> = {
    wait: '未开始',
    doing: '进行中',
    done: '已完成',
    closed: '已关闭',
    cancel: '已取消',
  };
  const priorityMap: Record<string, string> = {
    low: '低',
    normal: '中',
    high: '高',
    urgent: '紧急',
  };
  return `  [#${task.id}] ${task.title}\n     状态: ${statusMap[task.status] || task.status} | 优先级: ${priorityMap[task.priority] || task.priority} | 截止: ${task.deadline || '无'}`;
}

program
  .command('tasks')
  .description('获取我的全部任务列表')
  .action(async () => {
    try {
      const service = createService();
      const tasks = await getTasks(service);
      if (tasks.length === 0) {
        console.log('暂无任务');
        return;
      }
      console.log(`\n📋 任务列表（共 ${tasks.length} 条）\n`);
      tasks.forEach(t => console.log(formatTask(t)));
    } catch (err) {
      console.error('获取任务失败:', err instanceof Error ? err.message : String(err));
      process.exit(1);
    }
  });

program
  .command('today')
  .description('获取今天截止的任务')
  .action(async () => {
    try {
      const service = createService();
      const tasks = await getTodayTasks(service);
      if (tasks.length === 0) {
        console.log('今天没有截止的任务');
        return;
      }
      console.log(`\n📅 今天截止的任务（共 ${tasks.length} 条）\n`);
      tasks.forEach(t => console.log(formatTask(t)));
    } catch (err) {
      console.error('获取今日任务失败:', err instanceof Error ? err.message : String(err));
      process.exit(1);
    }
  });

program
  .command('task <id>')
  .description('获取任务详情')
  .action(async (id: string) => {
    try {
      const service = createService();
      const task = await getTaskDetail(service, id);
      if (!task) {
        console.log(`任务 ${id} 不存在`);
        return;
      }
      const statusMap: Record<string, string> = {
        wait: '未开始',
        doing: '进行中',
        done: '已完成',
        closed: '已关闭',
        cancel: '已取消',
      };
      const priorityMap: Record<string, string> = {
        low: '低',
        normal: '中',
        high: '高',
        urgent: '紧急',
      };
      console.log(`\n📌 任务详情 [#${task.id}]\n`);
      console.log(`  标题: ${task.title}`);
      console.log(`  描述: ${task.description || '无'}`);
      console.log(`  状态: ${statusMap[task.status] || task.status}`);
      console.log(`  优先级: ${priorityMap[task.priority] || task.priority}`);
      console.log(`  预计工时: ${task.estimatedHours}h`);
      console.log(`  已消耗: ${task.consumedHours}h`);
      console.log(`  截止时间: ${task.deadline || '无'}`);
      console.log(`  负责人: ${task.assignee || '未分配'}`);
      if (task.comments.length > 0) {
        console.log(`\n  💬 评论（${task.comments.length} 条）:`);
        task.comments.forEach(c => {
          console.log(`    [${c.createdAt}] ${c.author}: ${c.content}`);
        });
      } else {
        console.log(`\n  💬 评论: 无`);
      }
    } catch (err) {
      console.error('获取任务详情失败:', err instanceof Error ? err.message : String(err));
      process.exit(1);
    }
  });

program
  .command('analyze')
  .description('AI 分析任务风险')
  .action(async () => {
    try {
      const service = createService();
      const result = await analyzeRisk(service);
      console.log('\n🔍 任务风险分析\n');
      console.log(result.summary);
      console.log('');

      if (result.overdueTasks.length > 0) {
        console.log('⚠️  可能延期的任务:');
        result.overdueTasks.forEach(t => {
          console.log(`   - [${t.deadline}] ${t.title} (${t.status})`);
        });
        console.log('');
      }

      if (result.highPriorityTasks.length > 0) {
        console.log('🔥 高优先级任务:');
        result.highPriorityTasks.forEach(t => {
          console.log(`   - [${t.priority}] ${t.title}`);
        });
        console.log('');
      }

      if (result.dependencyRisks.length > 0) {
        console.log('🔗 依赖风险:');
        result.dependencyRisks.forEach(r => {
          console.log(`   - ${r.taskTitle}: ${r.reason}`);
        });
        console.log('');
      }
    } catch (err) {
      console.error('分析失败:', err instanceof Error ? err.message : String(err));
      process.exit(1);
    }
  });

program.parse();
