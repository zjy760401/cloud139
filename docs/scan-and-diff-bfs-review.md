# scan_and_diff_bfs_personal 详细流程分析

> 源文件: `src/commands/sync.rs`  
> 函数: `scan_and_diff_bfs_personal` (L829–L1016)  
> 调用位置: `execute_personal` → Step 2 (L1674–L1677)

---

## 1. 概述

`scan_and_diff_bfs_personal` 是 sync 命令的核心引擎，采用 **BFS（广度优先搜索）** 策略同时完成两件事：

1. **扫描远程目录树**（PersonalNew 存储类型）
2. **逐目录计算本地 vs 远程的差异**

与传统 "先全量扫描远程、再整体 diff" 的两趟扫描不同，这里在扫描的同时就完成了差异计算，并且能跳过本地独有目录的远程请求，减少 API 调用次数。

### 签名

```rust
async fn scan_and_diff_bfs_personal(
    config: &Config,
    remote_path: &str,           // 远程同步根路径, e.g. "/backup"
    local_entries: &[LocalFileEntry],  // Step 1 扫描得到的本地文件列表（已扁平化）
    exclude_patterns: &[String],       // 排除模式
) -> Result<(
    Vec<DiffEntry>,       // 差异列表
    Vec<RemoteFileEntry>, // 远程文件/目录完整列表
    usize,                // 远程文件数
    usize,                // 远程目录数
), ClientError>
```

### 返回值用途

| 返回值 | 下游消费者 |
|--------|-----------|
| `diffs` | 决定上传/下载/跳过的操作列表 |
| `remote_entries` | 构建 `remote_id_map` (relative_path → file_id)，供下载任务查找 file_id |
| `remote_file_count` / `remote_dir_count` | 日志输出 |

---

## 2. 前置准备

### 2.1 创建共享 HTTP 客户端 (L835)

```rust
let http_client = api::HttpClientWrapper::new();
```

整个 BFS 过程共享**一个** `HttpClientWrapper`（底层是 `reqwest::Client`），复用 TCP/TLS 连接池。  
对比旧代码每次 API 调用都 `new()` 一个 Client，253 个目录 = 253 次 TLS 握手 → 现在只需要少量连接。

### 2.2 获取 Personal Cloud Host (L837)

```rust
let host = api::get_personal_cloud_host_with_client(&mut config, &http_client).await?;
```

查询路由策略 API (`qryRoutePolicy`)，获取个人云的实际 API Host（如 `https://personal-njs.yun.139.com`）。结果会被缓存到 config 中，后续不再请求。

### 2.3 构建本地目录索引 (L839)

```rust
let index = build_local_dir_index(local_entries);
```

将扁平化的 `local_entries` 列表重组为按目录分组的索引结构：

```rust
struct LocalDirIndex {
    files: HashMap<String, Vec<LocalFileEntry>>,   // 目录路径 → 该目录下的文件
    subdirs: HashMap<String, HashSet<String>>,       // 目录路径 → 该目录下的子目录名
}
```

- Key `""` (空字符串) 表示根目录
- Key `"photos"` 表示 `photos/` 目录
- Key `"photos/2024"` 表示 `photos/2024/` 目录

**示例**: 本地结构如下：
```
readme.md
photos/
  img1.jpg
  2024/
    img2.jpg
docs/
  note.md
```

索引结果：
```
files:
  "" → [readme.md]
  "photos" → [photos/img1.jpg]
  "photos/2024" → [photos/2024/img2.jpg]
  "docs" → [docs/note.md]

subdirs:
  "" → {"photos", "docs"}
  "photos" → {"2024"}
```

### 2.4 确保远程根路径存在 (L841)

```rust
let remote_root_id = ensure_remote_root_personal(&config, &host, remote_path, &http_client).await?;
```

如果用户指定的远程路径 (如 `/backup/mydata`) 不存在，会逐级创建。  
返回该路径对应的 `file_id`，作为 BFS 的起点。

---

## 3. BFS 主循环

### 3.1 初始化

```
queue: [("", remote_root_id)]   // BFS 队列: (相对目录路径, 远程目录 file_id)
diffs: []                        // 差异结果
remote_entries: []                // 远程文件/目录条目
```

### 3.2 循环结构

```
while queue 非空:
    取出 (rel_dir, remote_dir_id)
    ├── 输出实时进度
    ├── 调用 API 列出远程目录内容 (单层)
    ├── 分类远程条目为 文件 / 子目录
    ├── 从本地索引取出对应目录的 文件 / 子目录
    ├── 比较文件 (3 种情况)
    ├── 比较子目录 (3 种情况)
    └── 处理远程独有子目录
```

### 3.3 详细步骤

#### Step A: 实时进度输出 (L854–L856)

```rust
scanned_dir_count += 1;
eprint!("\r\x1b[36minfo\x1b[0m 已扫描 {} 个目录, 当前: {}  \x1b[K",
    scanned_dir_count, display_dir);
```

使用 `\r` 覆写同一行，`\x1b[K` 清除行尾残余字符。用户可以实时看到扫描进度。

#### Step B: 列出远程目录内容 (L858)

```rust
let remote_items = list_remote_dir_personal(&config, &host, &remote_dir_id, &http_client).await?;
```

- 调用 `{host}/file/list` API
- 分页处理：每页 100 条，通过 `pageCursor` 翻页直到没有更多
- 返回当前目录下的**直接子条目**（不递归）
- 使用共享 `http_client` 复用连接

#### Step C: 分类远程条目 (L860–L882)

通过 `item_to_remote_entry()` 将 API 返回的 `PersonalFileItem` 转换为 `RemoteFileEntry`：

```
对每个远程条目:
    ├── 跳过空名称
    ├── 计算 relative_path (rel_dir + "/" + name)
    ├── 检查是否被 exclude 模式排除
    ├── 如果是目录 → 记入 remote_subdirs_here (name → file_id)
    └── 如果是文件 → 记入 remote_files_here (name → RemoteFileEntry)
```

所有条目同时追加到 `remote_entries` 总表中。

#### Step D: 取出本地对应目录 (L884–L888)

```rust
let local_files = index.files.get(&rel_dir).unwrap_or(&empty_files);
let local_subs = index.subdirs.get(&rel_dir).unwrap_or(&empty_subdirs);
```

#### Step E: 比较文件 — 三种情况 (L890–L940)

```
                     远程有该文件?
                    /            \
                  否              是
                  │               │
          DiffKind::OnlyLocal    size 相同?
           (需要上传)           /       \
                              是        否
                              │         │
                           无差异    determine_newer()
                          (跳过)     /         \
                                LocalNewer  RemoteNewer
```

**判断哪端较新的逻辑** (`determine_newer`):
1. 将远程的 `modified_time` 字符串解析为毫秒时间戳
2. 与本地的 `modified_epoch_ms` 比较
3. 本地时间戳更大 → `LocalNewer`，否则 → `RemoteNewer`

> ⚠️ **注意**: 当前只比较 **size** 是否不同来触发 diff。size 相同的文件即使内容不同也会被认为一致。这是一个有意的性能取舍——避免对每个文件计算 hash。

然后反向遍历远程文件，找出仅远程存在的：

```
对每个远程文件:
    ├── 在本地文件列表中找到同名 → 已在上面处理
    └── 找不到 → DiffKind::OnlyRemote (需要下载)
```

#### Step F: 比较子目录 — 三种情况 (L942–L1006)

```
             远程有该子目录?
            /              \
          否                是
          │                 │
    collect_local_files_   入队 BFS 继续比较
    under() 递归收集       queue.push_back(
    所有子文件并标记          (child_rel, remote_id))
    OnlyLocal
```

**仅本地存在的子目录** (L954–L966):
- 调用 `collect_local_files_under()` 从本地索引递归收集该目录下的所有文件
- 全部标记为 `OnlyLocal`
- **关键优化**: 不发起任何远程 API 请求，因为已知远程没有这个目录

**两端都存在的子目录** (L951–L953):
- 推入 BFS 队列，下一轮循环处理
- 这确保了广度优先的遍历顺序

**仅远程存在的子目录** (L969–L1006):
- 调用 `scan_remote_recursive_personal()` **深度优先**递归扫描远程子树
- 所有扫描到的文件标记为 `OnlyRemote`
- 这里用 DFS 而非继续 BFS，因为本地已确认没有这个目录，无需逐层比较

---

## 4. 收尾 (L1009–L1015)

```rust
eprint!("\r\x1b[K");  // 清除进度行
diffs.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
remote_entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
```

结果按路径字典序排序后返回。

---

## 5. 完整流程图

```
scan_and_diff_bfs_personal
│
├── 1. 创建共享 HTTP Client (连接池复用)
├── 2. 获取 Personal Cloud Host (路由策略)
├── 3. build_local_dir_index (本地文件按目录分组)
├── 4. ensure_remote_root_personal (确保远程根目录存在)
│
├── 5. BFS 主循环
│   │
│   │  对队列中的每个 (rel_dir, remote_dir_id):
│   │
│   ├── 5a. 输出扫描进度 (覆写式)
│   ├── 5b. list_remote_dir_personal (API, 单层, 分页)
│   ├── 5c. 分类: remote_files_here / remote_subdirs_here
│   ├── 5d. 取出 local_files / local_subs
│   │
│   ├── 5e. 比较文件
│   │   ├── 本地有, 远程无 → OnlyLocal
│   │   ├── 远程有, 本地无 → OnlyRemote
│   │   └── 两端都有, size 不同 → LocalNewer 或 RemoteNewer
│   │
│   └── 5f. 比较子目录
│       ├── 本地有, 远程无 → 递归收集本地文件, 全标 OnlyLocal (零API调用)
│       ├── 两端都有 → 入队 BFS
│       └── 远程有, 本地无 → DFS 递归扫描远程, 全标 OnlyRemote
│
├── 6. 清除进度行
├── 7. 排序 diffs + remote_entries
└── 8. 返回 (diffs, remote_entries, file_count, dir_count)
```

---

## 6. 性能优化分析

| 优化点 | 说明 |
|--------|------|
| **共享 HTTP Client** | 所有 API 请求复用同一个 `reqwest::Client`，底层 TCP/TLS 连接池避免重复握手 |
| **BFS + 本地索引** | 不做全量远程扫描后再 diff，而是逐目录按需请求 |
| **本地独有目录零请求** | 如果某目录仅本地存在，直接从索引收集文件，不调用任何 API |
| **单层 API 调用** | 每个目录只调一次 `file/list`（分页），不做递归 API 调用 |
| **实时进度** | 用 `eprint!("\r...")` 覆写同一行，不产生大量日志输出 |

### API 调用次数估算

设远程有 D 个目录（与本地共有）、R 个仅远程目录、L 个仅本地目录：

- BFS 主循环: **D** 次 `file/list` 调用
- 仅远程子树 DFS: 约 **R** 次 `file/list` 调用
- 仅本地子树: **0** 次 API 调用

**总计 ≈ D + R 次 API 调用**（加上分页可能翻倍，但单目录超过 100 个文件才需要翻页）

---

## 7. 数据结构关系

```
LocalFileEntry                    RemoteFileEntry
├── relative_path: String         ├── relative_path: String
├── size: i64                     ├── name: String
├── modified_epoch_ms: i64        ├── file_id: String
└── is_dir: bool                  ├── size: i64
                                  ├── modified_time: String
                                  └── is_dir: bool
        ↓                                   ↓
        └──────────── DiffEntry ────────────┘
                      ├── relative_path
                      ├── kind: DiffKind
                      ├── local: Option<LocalFileEntry>
                      ├── remote: Option<RemoteFileEntry>
                      └── is_dir: bool

DiffKind:
  OnlyLocal   → 需要上传
  OnlyRemote  → 需要下载
  LocalNewer  → 本地较新 (可上传覆盖)
  RemoteNewer → 远程较新 (可下载覆盖)
```

---

## 8. 下游处理

`scan_and_diff_bfs_personal` 返回后，`execute_personal` 继续：

1. **显示差异统计** — 仅本地/仅远程/已修改各多少个
2. **决策阶段** — 根据 sync_mode 决定每个 diff 的动作:
   - `UploadOnly` → OnlyLocal/LocalNewer 上传，其余跳过
   - `DownloadOnly` → OnlyRemote/RemoteNewer 下载，其余跳过
   - `TwoWay` → 全部自动决策
   - `Interactive` → 逐个询问用户
3. **执行阶段** — 流水线执行:
   - 上传流水线: 创建目录 → mpsc channel → 并行上传 (与下载并行)
   - 下载流水线: 直接并行下载 (与上传并行)
