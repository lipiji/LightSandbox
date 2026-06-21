你是一个资深系统工程师、Rust/Python 后端工程师、AI Agent Runtime 架构师。请在我已经创建好的本地项目和 GitHub 仓库中，从零开发一个名为 **LightSandbox** 的开源项目。

## 一、项目定位

LightSandbox 是一个面向 AI Agent 的轻量、高效、高并发、易用的 sandbox 执行层。

它不是一开始就做重型容器平台，也不是直接复制 OpenSandbox、agent-sandbox 或 CubeSandbox，而是吸收它们的思想，做一个更适合个人开发者、小团队、科研智能体平台、本地 Agent、私有化部署的轻量级 sandbox runtime。

LightSandbox 的核心目标：

* 极轻量：v0.1 默认不依赖 Docker、Kubernetes、Firecracker、containerd。
* 易安装：本地 clone 后即可运行。
* 易使用：提供 REST API、Python SDK、CLI。
* 高并发：支持多个 agent 并发创建 sandbox、执行命令、读写文件。
* 可管理：支持 sandbox 生命周期、TTL、GC、超时、日志、状态查询。
* AI Agent 原生：适合代码执行、文件操作、任务执行、工具调用、实验脚本运行。
* 可扩展：后续可以插入 DockerRuntime、ContainerRuntime、KubernetesRuntime、MicroVMRuntime。
* 默认安全意识：虽然 v0.1 是轻量 LocalProcessRuntime，但必须清楚声明其安全边界，不把它伪装成强安全隔离环境。

一句话定位：

```text
LightSandbox = AI Agent 的轻量执行层，比 subprocess 更工程化，比 Docker 更轻，比云端 sandbox 更适合本地和私有化。
```

## 二、重要原则

v0.1 的核心原则：

1. **不要强依赖 Docker。**
2. **不要强依赖 Kubernetes。**
3. **不要强依赖数据库。**
4. **不要强依赖 Redis。**
5. **不要强依赖消息队列。**
6. **不要做复杂 Web UI。**
7. **不要做重型多租户平台。**
8. **不要一开始追求绝对安全隔离。**
9. **先做一个可运行、可测试、可扩展、代码干净的 MVP。**

如果某个功能会显著增加依赖，请先保持接口设计，暂时不实现。

## 三、参考项目理解

请参考但不要照搬以下项目：

1. OpenSandbox
   学习其统一 API、多语言 SDK、多 runtime 抽象、AI Code Execution 场景。

2. kubernetes-sigs/agent-sandbox
   学习其面向 AI agent workload 的状态管理、隔离运行、生命周期管理思路。

3. TencentCloud/CubeSandbox
   学习其高性能、低冷启动、高并发、E2B 兼容、硬件隔离方向。

LightSandbox v0.1 不追求这些项目的完整能力，而是做一个轻量起点。

## 四、v0.1 默认 Runtime：LocalProcessRuntime

v0.1 默认实现 **LocalProcessRuntime**。

LocalProcessRuntime 不依赖 Docker。每个 sandbox 对应一个独立 workspace 目录和一组进程执行上下文。

例如：

```text
~/.lightsandbox/
  workspaces/
    sbx_abc123/
      main.py
      output.txt
```

执行命令时：

```bash
cd ~/.lightsandbox/workspaces/sbx_abc123
python main.py
```

但必须增加工程化管理能力：

* sandbox_id 管理
* 独立 workspace
* 命令执行超时
* stdout/stderr 大小限制
* 文件读写限制
* 防 path traversal
* sandbox TTL
* 后台 GC
* 并发限制
* 进程树清理
* 状态管理
* 错误结构化返回

## 五、LocalProcessRuntime 安全边界

必须在 README 和 docs/security.md 中明确说明：

LocalProcessRuntime 不是强安全隔离环境。它适合：

* 本地 AI Agent 开发
* 可信代码执行
* 科研脚本运行
* 私有工具调用
* 内部自动化任务

它不适合直接运行不可信用户代码。

因为本地进程理论上仍可能：

* 访问宿主机可访问的文件
* 访问网络
* 启动子进程
* 占用 CPU 或内存
* 调用系统命令

LightSandbox v0.1 的目标是轻量和可管理，不是替代 Docker、gVisor、Firecracker 或 KVM。

README 中必须写明：

```text
LocalProcessRuntime is designed for trusted workloads and local AI agent development. 
For untrusted code execution, use DockerRuntime, gVisor, Firecracker, or another stronger isolation backend.
```

## 六、未来 Runtime 预留

请设计 runtime-agnostic 架构，使用统一 trait/interface。

v0.1 实现：

```text
LocalProcessRuntime
```

后续预留：

```text
DockerRuntime
ContainerdRuntime
KubernetesRuntime
FirecrackerRuntime
E2BCompatibleRuntime
CubeSandboxAdapter
```

但 v0.1 不需要实现这些重 runtime。

项目启动时，如果系统没有 Docker，也必须能正常运行。

## 七、建议技术栈

优先使用：

* Rust 作为核心服务语言
* tokio 作为异步运行时
* axum 作为 HTTP server
* serde / serde_json 做序列化
* clap 做 CLI
* tracing 做日志
* tempfile 或标准库管理 workspace
* Python SDK 使用 requests 或 httpx
* 配置文件使用 TOML

避免引入过重依赖。

v0.1 可以不使用数据库。sandbox 状态可以先用内存管理，同时 workspace 落盘。

后续可以预留 SQLite 持久化。

## 八、建议项目结构

请创建如下结构：

```text
LightSandbox
├── crates/
│   ├── lightsandbox-core/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── models.rs
│   │   │   ├── error.rs
│   │   │   └── runtime.rs
│   ├── lightsandbox-runtime-local/
│   │   ├── src/
│   │   │   └── lib.rs
│   ├── lightsandbox-server/
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── api.rs
│   │   │   ├── state.rs
│   │   │   └── gc.rs
│   └── lightsandbox-cli/
│       ├── src/
│       │   └── main.rs
├── sdk/
│   └── python/
│       ├── lightsandbox/
│       │   ├── __init__.py
│       │   ├── client.py
│       │   ├── sandbox.py
│       │   └── exceptions.py
│       ├── pyproject.toml
│       └── README.md
├── examples/
│   ├── python_agent_demo/
│   ├── code_execution_demo/
│   └── concurrent_sandboxes/
├── docs/
│   ├── quickstart.md
│   ├── api.md
│   ├── architecture.md
│   └── security.md
├── config.example.toml
├── README.md
├── ROADMAP.md
├── LICENSE
└── Cargo.toml
```

如果你认为可以先简化结构，也可以简化，但必须保证后续容易扩展 runtime。

## 九、核心数据模型

请实现以下核心结构：

```rust
SandboxId
SandboxSpec
SandboxInfo
SandboxStatus
ExecRequest
ExecResult
FileWriteRequest
FileReadResponse
ResourceLimits
RuntimeConfig
LightSandboxError
```

SandboxStatus 至少包括：

```rust
Creating
Running
Stopped
Failed
Expired
Removed
```

SandboxSpec 至少包含：

```rust
pub struct SandboxSpec {
    pub ttl_seconds: Option<u64>,
    pub metadata: Option<HashMap<String, String>>,
    pub env: Option<HashMap<String, String>>,
}
```

SandboxInfo 至少包含：

```rust
pub struct SandboxInfo {
    pub id: String,
    pub status: SandboxStatus,
    pub workspace_path: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub metadata: HashMap<String, String>,
}
```

ExecRequest 至少包含：

```rust
pub struct ExecRequest {
    pub cmd: String,
    pub timeout_seconds: Option<u64>,
    pub env: Option<HashMap<String, String>>,
}
```

ExecResult 至少包含：

```rust
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub timed_out: bool,
}
```

## 十、Runtime Trait

请定义统一 Runtime Trait，例如：

```rust
#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    async fn create(&self, spec: SandboxSpec) -> Result<SandboxInfo, LightSandboxError>;
    async fn list(&self) -> Result<Vec<SandboxInfo>, LightSandboxError>;
    async fn get(&self, id: &str) -> Result<SandboxInfo, LightSandboxError>;
    async fn exec(&self, id: &str, req: ExecRequest) -> Result<ExecResult, LightSandboxError>;
    async fn write_file(&self, id: &str, path: &str, content: Vec<u8>) -> Result<(), LightSandboxError>;
    async fn read_file(&self, id: &str, path: &str) -> Result<Vec<u8>, LightSandboxError>;
    async fn remove(&self, id: &str) -> Result<(), LightSandboxError>;
    async fn cleanup_expired(&self) -> Result<usize, LightSandboxError>;
}
```

LocalProcessRuntime 实现这个 trait。

## 十一、LocalProcessRuntime 实现要求

LocalProcessRuntime 需要实现：

### 1. 创建 sandbox

* 生成唯一 sandbox_id，例如 sbx_xxxxxxxx。
* 在 workspace_root 下创建目录。
* 保存状态到内存 map。
* 设置 created_at 和 expires_at。
* 返回 SandboxInfo。

### 2. 执行命令

* 命令在该 sandbox 的 workspace 中执行。
* 支持 timeout。
* 捕获 stdout/stderr。
* 限制 stdout/stderr 最大长度。
* 返回 exit_code、stdout、stderr、duration_ms、timed_out。
* 超时后必须尽力终止进程。
* 禁止无限挂起。

### 3. 文件写入

* 只能写入 sandbox workspace 内部。
* 防止 path traversal。
* 支持自动创建父目录。
* 限制单文件大小。
* 支持文本和二进制内容，API 层可以先用 base64 或普通字符串。

### 4. 文件读取

* 只能读取 sandbox workspace 内部。
* 防止 path traversal。
* 限制最大读取大小。
* 文件不存在返回结构化错误。

### 5. 删除 sandbox

* 删除 workspace。
* 更新状态。
* 从 map 移除或标记 removed。

### 6. GC

* 后台定期扫描 expired sandbox。
* 删除 workspace。
* 清理状态。
* 返回清理数量。

### 7. 并发安全

* 使用 Arc/RwLock 或 DashMap 管理状态。
* 支持多个请求同时创建和执行。
* 避免数据竞争。

## 十二、HTTP API

实现 REST API。

### 健康检查

```http
GET /health
```

返回：

```json
{"status":"ok"}
```

### 创建 sandbox

```http
POST /v1/sandboxes
```

请求：

```json
{
  "ttl_seconds": 600,
  "env": {
    "PYTHONUNBUFFERED": "1"
  },
  "metadata": {
    "agent_id": "demo-agent"
  }
}
```

返回：

```json
{
  "id": "sbx_xxx",
  "status": "running",
  "workspace_path": "...",
  "created_at": "...",
  "expires_at": "..."
}
```

注意：API 返回中可以隐藏真实宿主机绝对路径，避免暴露敏感信息。可以返回 logical path，例如：

```json
"workspace": "/workspace"
```

### 列出 sandboxes

```http
GET /v1/sandboxes
```

### 查看 sandbox

```http
GET /v1/sandboxes/{id}
```

### 执行命令

```http
POST /v1/sandboxes/{id}/exec
```

请求：

```json
{
  "cmd": "python -c \"print('hello lightsandbox')\"",
  "timeout_seconds": 30
}
```

返回：

```json
{
  "exit_code": 0,
  "stdout": "hello lightsandbox\n",
  "stderr": "",
  "duration_ms": 123,
  "timed_out": false
}
```

### 写文件

```http
PUT /v1/sandboxes/{id}/files
```

请求：

```json
{
  "path": "main.py",
  "content": "print('hello lightsandbox')"
}
```

### 读文件

```http
GET /v1/sandboxes/{id}/files?path=main.py
```

返回：

```json
{
  "path": "main.py",
  "content": "print('hello lightsandbox')"
}
```

### 删除 sandbox

```http
DELETE /v1/sandboxes/{id}
```

返回：

```json
{
  "removed": true
}
```

## 十三、错误返回格式

所有 API 错误统一返回：

```json
{
  "error": {
    "code": "SANDBOX_NOT_FOUND",
    "message": "sandbox not found"
  }
}
```

错误 code 至少包括：

```text
SANDBOX_NOT_FOUND
SANDBOX_EXPIRED
INVALID_PATH
EXEC_TIMEOUT
EXEC_FAILED
FILE_TOO_LARGE
OUTPUT_TOO_LARGE
RUNTIME_ERROR
CONFIG_ERROR
INTERNAL_ERROR
```

不要直接把 Rust panic 或宿主机敏感路径暴露给 API 用户。

## 十四、配置文件

提供 config.example.toml：

```toml
[server]
host = "127.0.0.1"
port = 8080

[runtime]
type = "local"
workspace_root = "./data/workspaces"

[limits]
max_sandboxes = 100
max_concurrent_exec = 20
default_ttl_seconds = 600
default_exec_timeout_seconds = 60
max_stdout_bytes = 1048576
max_stderr_bytes = 1048576
max_file_size_bytes = 10485760
max_read_file_bytes = 10485760

[gc]
enabled = true
interval_seconds = 30
remove_expired = true

[security]
allow_absolute_paths = false
allow_path_traversal = false
hide_host_paths = true
```

## 十五、CLI 设计

实现 CLI：

```bash
lightsandbox server --config config.example.toml

lightsandbox create
lightsandbox list
lightsandbox exec <sandbox_id> "python -V"
lightsandbox write <sandbox_id> ./local.py main.py
lightsandbox read <sandbox_id> main.py
lightsandbox rm <sandbox_id>
```

支持 JSON 输出：

```bash
lightsandbox list --json
lightsandbox create --json
```

CLI 应该通过 HTTP API 调用本地 server，而不是绕过 server 直接操作 runtime。

## 十六、Python SDK

实现 Python SDK。

示例：

```python
from lightsandbox import LightSandboxClient

client = LightSandboxClient("http://127.0.0.1:8080")

with client.create(ttl_seconds=300) as sbx:
    sbx.write_file("main.py", "print('hello lightsandbox')")
    result = sbx.exec("python main.py")
    print(result.stdout)
```

也支持非 context manager：

```python
from lightsandbox import LightSandboxClient

client = LightSandboxClient("http://127.0.0.1:8080")

sbx = client.create()
sbx.write_file("main.py", "print('hello')")
result = sbx.exec("python main.py")
print(result.stdout)
sbx.remove()
```

异常类型：

```python
LightSandboxError
SandboxNotFound
SandboxExpired
SandboxTimeout
SandboxExecError
LightSandboxConnectionError
```

## 十七、Examples

请提供 examples：

### 1. python_agent_demo

展示 AI Agent 如何创建 sandbox、写入 Python 文件、执行、读取结果、删除 sandbox。

### 2. code_execution_demo

展示通用代码执行。

### 3. concurrent_sandboxes

展示并发创建和执行多个 sandbox，例如 20 个 sandbox 同时运行简单命令。

## 十八、测试要求

至少实现以下测试：

1. 创建 sandbox 成功。
2. list 能看到 sandbox。
3. exec echo 成功。
4. exec python 成功。
5. 写文件后读取文件成功。
6. 删除 sandbox 后不能再次 exec。
7. timeout 生效。
8. 非法路径 `../x` 被拒绝。
9. 超大文件写入被拒绝。
10. GC 能清理过期 sandbox。
11. 并发创建多个 sandbox 不崩溃。
12. API 错误格式稳定。

测试不要依赖 Docker。

## 十九、性能与并发

v0.1 不要虚构性能指标。

请提供 benchmark 示例或命令，让用户自己测试：

```bash
cargo run --example concurrent_sandboxes -- --n 100 --concurrency 20
```

目标是：

* 不因为并发请求直接崩溃。
* 状态管理线程安全。
* 超时任务能清理。
* workspace 不产生大量残留。
* GC 能稳定运行。

README 中可以写：

```text
Performance depends on host OS, command type, disk speed, and process startup cost.
Please run examples/concurrent_sandboxes to benchmark your environment.
```

## 二十、README 要求

README.md 必须包括：

1. LightSandbox 是什么。
2. 为什么做 LightSandbox。
3. 核心特性。
4. 与 Docker sandbox、OpenSandbox、E2B、CubeSandbox 的区别。
5. 快速开始。
6. REST API 示例。
7. Python SDK 示例。
8. CLI 示例。
9. LocalProcessRuntime 安全边界。
10. Roadmap。
11. License。

README 风格要简洁、有技术感、有产品感。

推荐 README 开头：

```markdown
# LightSandbox

LightSandbox is a lightweight sandbox runtime for AI agents.

It provides a simple REST API, Python SDK, and CLI for creating isolated workspaces, executing commands, reading/writing files, enforcing timeouts, and cleaning up agent tasks.

LightSandbox v0.1 starts with a zero-Docker LocalProcessRuntime for trusted local workloads. Stronger isolation backends such as Docker, gVisor, Firecracker, and Kubernetes are planned as optional runtimes.
```

## 二十一、docs 要求

请创建：

### docs/quickstart.md

包括：

```bash
cargo run -p lightsandbox-server
curl http://127.0.0.1:8080/health
```

以及完整创建、写文件、执行、删除示例。

### docs/api.md

详细说明 REST API。

### docs/architecture.md

说明：

```text
API Server
Runtime Trait
LocalProcessRuntime
Workspace Manager
Process Executor
GC Task
Python SDK
CLI
```

### docs/security.md

重点说明 LocalProcessRuntime 不是强安全边界。

## 二十二、Roadmap

创建 ROADMAP.md：

```markdown
# Roadmap

## v0.1
- LocalProcessRuntime
- REST API
- Python SDK
- CLI
- Workspace manager
- Exec timeout
- File read/write
- TTL and GC
- Basic concurrency control

## v0.2
- Streaming exec output
- Sandbox pool
- SQLite metadata persistence
- Metrics endpoint
- Better process tree cleanup
- File upload/download multipart

## v0.3
- Optional DockerRuntime
- Rootless Docker guide
- Basic network policy
- Resource limit improvements

## v0.4
- E2B-compatible API subset
- Containerd runtime
- Authentication token

## v0.5
- Kubernetes runtime
- Multi-node scheduling

## v0.6
- Firecracker or microVM runtime
- Strong isolation profile
```

## 二十三、开发顺序

请按以下顺序开发：

1. 检查当前仓库结构。
2. 初始化 Rust workspace。
3. 创建 README、ROADMAP、docs、config.example.toml。
4. 实现 lightsandbox-core。
5. 实现 Runtime trait。
6. 实现 LocalProcessRuntime。
7. 实现 /health。
8. 实现 sandbox create/list/get/remove API。
9. 实现 exec API。
10. 实现 file read/write API。
11. 实现 TTL 和 GC。
12. 实现 CLI。
13. 实现 Python SDK。
14. 实现 examples。
15. 实现测试。
16. 运行 cargo fmt。
17. 运行 cargo test。
18. 修复错误。
19. 最后总结已完成内容和下一步建议。

## 二十四、commit 规范

请使用清晰 commit message：

```text
feat: initialize LightSandbox workspace
feat: add core models and runtime trait
feat: implement local process runtime
feat: add REST API server
feat: add exec and file APIs
feat: add TTL garbage collection
feat: add CLI
feat: add Python SDK
test: add local runtime tests
docs: add quickstart and security notes
```

## 二十五、最终验收目标

完成后，用户应该可以运行：

```bash
cargo run -p lightsandbox-server
```

然后另一个终端运行：

```bash
curl http://127.0.0.1:8080/health
```

返回：

```json
{"status":"ok"}
```

然后 Python demo 可以运行：

```bash
python examples/python_agent_demo/main.py
```

输出类似：

```text
created sandbox: sbx_xxxxx
exec result: hello lightsandbox
removed sandbox: sbx_xxxxx
```

## 二十六、最重要的产品气质

请始终保持 LightSandbox 的产品气质：

* 轻量
* 克制
* 工程化
* 高并发友好
* 本地优先
* AI Agent 原生
* 默认简单
* 后续可扩展
* 不做重平台
* 不引入不必要依赖

不要为了炫技引入复杂架构。先把 v0.1 做扎实。
