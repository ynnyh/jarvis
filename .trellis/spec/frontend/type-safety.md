# Type Safety

> 类型安全与前后端调用契约约定。

---

## Convention: Tauri invoke 参数命名必须 camelCase

**What**：前端 `invoke(cmd, args)` 的 `args` 对象，**所有 key 一律 camelCase**；Rust 命令参数用 snake_case，由 Tauri v2 自动按 camelCase↔snake_case 映射。

**Why**：Tauri v2 命令默认 `rename_all = "camelCase"`。前端写成 snake_case 的 key **不会**匹配到 Rust 参数 → 该参数被反序列化为默认值（`Option`→`None`、`bool`→`false`），且**不报错、静默失效**。`bool`/`Option` 参数尤其危险，拼错没有任何编译期或运行期提示。

**真实事故（C1）**：`CostApp.vue` 成本查询里 `include_overtime: true` 误写 snake_case → 后端 `include_overtime: Option<bool>` 收到 `None` → `unwrap_or(false)` → 加班拆分整条功能失效；因为同对象里 `startDate`/`includeResigned` 恰好是 camelCase，肉眼极难发现。

### Wrong
```ts
await invoke('project_cost_summary', {
  projectName: name,
  include_overtime: true,   // ❌ snake_case：后端收不到，静默落 false
  startDate: start,
})
```

### Correct
```ts
await invoke('project_cost_summary', {
  projectName: name,
  includeOvertime: true,    // ✅ camelCase
  startDate: start,
})
```

对应后端（无需 `#[tauri::command(rename_all=...)]`，默认即 camelCase 入参）：
```rust
#[tauri::command]
pub async fn project_cost_summary(
    project_name: String,
    include_overtime: Option<bool>,   // 前端传 includeOvertime
    start_date: Option<String>,       // 前端传 startDate
) -> Result<CostSummaryResult, String> { ... }
```

**自检**：新增/修改 invoke 调用时，逐个核对 args 的 key 为 camelCase；参数为 `Option`/`bool` 时拼错不会报错，必须人工核对或端到端验证。长期可考虑为每个命令封装类型化 wrapper，把参数名约束在一处。

---

## Type Organization

（待补充：共享类型 vs 局部类型的组织约定）

---

## Forbidden Patterns

（待补充：`any`、不安全断言等）
