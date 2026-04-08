use crate::client::api;
use crate::client::{ClientError, StorageType};
use crate::{error, info, step, success};
use clap::Parser;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// CLI Args
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
pub struct SyncArgs {
    #[arg(help = "本地文件夹路径")]
    pub local_path: Option<String>,

    #[arg(help = "远程文件夹路径")]
    pub remote_path: Option<String>,

    #[arg(long, help = "仅上传模式（本地 → 远程）")]
    pub upload_only: bool,

    #[arg(long, help = "仅下载模式（远程 → 本地）")]
    pub download_only: bool,

    #[arg(long, help = "双向完全同步")]
    pub two_way: bool,

    #[arg(long, help = "排除匹配的文件/目录（支持通配符，可多次指定）")]
    pub exclude: Vec<String>,

    #[arg(long, help = "仅显示将要执行的操作，不实际传输")]
    pub dry_run: bool,

    #[arg(
        long,
        short = 'j',
        default_value = "4",
        help = "并行上传/下载数（默认: 4）"
    )]
    pub concurrency: usize,

    #[arg(long, help = "启用多网卡负载均衡（自动探测可用网卡并分流）")]
    pub multi_net: bool,
}

/// 同步模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    UploadOnly,
    DownloadOnly,
    TwoWay,
    Interactive,
}

// ---------------------------------------------------------------------------
// File entry types
// ---------------------------------------------------------------------------

/// 本地文件条目
#[derive(Debug, Clone)]
pub struct LocalFileEntry {
    pub relative_path: String,
    pub size: i64,
    pub modified_epoch_ms: i64,
    pub is_dir: bool,
}

/// 远程文件条目（PersonalNew）
#[derive(Debug, Clone)]
pub struct RemoteFileEntry {
    pub relative_path: String,
    pub name: String,
    pub file_id: String,
    pub size: i64,
    pub modified_time: String,
    pub is_dir: bool,
}

/// 差异类型
#[derive(Debug, Clone)]
pub enum DiffKind {
    /// 仅存在于本地，需要上传
    OnlyLocal,
    /// 仅存在于远程，需要下载
    OnlyRemote,
    /// 两端都存在但内容不同（本地较新）
    LocalNewer,
    /// 两端都存在但内容不同（远程较新）
    RemoteNewer,
}

/// 差异条目
#[derive(Debug, Clone)]
pub struct DiffEntry {
    pub relative_path: String,
    pub kind: DiffKind,
    pub local: Option<LocalFileEntry>,
    pub remote: Option<RemoteFileEntry>,
    pub is_dir: bool,
}

/// 用户对单个文件的决策
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAction {
    Download,
    Upload,
    Skip,
    /// 双向同步：根据差异类型自动决定上传或下载
    Sync,
}

/// 根据差异类型自动选择同步方向
fn auto_sync_action(kind: &DiffKind) -> UserAction {
    match kind {
        DiffKind::OnlyLocal | DiffKind::LocalNewer => UserAction::Upload,
        DiffKind::OnlyRemote | DiffKind::RemoteNewer => UserAction::Download,
    }
}

// ---------------------------------------------------------------------------
// macOS hidden file patterns
// ---------------------------------------------------------------------------

/// macOS 系统自动生成的隐藏文件/目录
const MACOS_EXCLUDE_PATTERNS: &[&str] = &[
    ".DS_Store",
    "._*",
    ".Spotlight-V100",
    ".Trashes",
    ".fseventsd",
    "__MACOSX",
    ".TemporaryItems",
    ".AppleDouble",
    ".LSOverride",
    ".DocumentRevisions-V100",
    ".VolumeIcon.icns",
    ".localized",
    "Icon\r",
];

/// 判断当前是否是 macOS 平台
fn is_macos() -> bool {
    cfg!(target_os = "macos")
}

// ---------------------------------------------------------------------------
// Exclude pattern matching
// ---------------------------------------------------------------------------

/// 检查相对路径是否匹配任一 exclude 模式
pub fn is_excluded(relative_path: &str, patterns: &[String]) -> bool {
    let name = relative_path.rsplit('/').next().unwrap_or(relative_path);
    let components: Vec<&str> = relative_path.split('/').collect();

    // 检查 macOS 系统文件
    if is_macos() {
        for &mac_pattern in MACOS_EXCLUDE_PATTERNS {
            if glob_match::glob_match(mac_pattern, name)
                || glob_match::glob_match(mac_pattern, relative_path)
                || components
                    .iter()
                    .any(|c| glob_match::glob_match(mac_pattern, c))
            {
                return true;
            }
        }
    }

    for pattern in patterns {
        if glob_match::glob_match(pattern, name)
            || glob_match::glob_match(pattern, relative_path)
            || components
                .iter()
                .any(|c| glob_match::glob_match(pattern, c))
        {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Multi-net: 多网卡负载均衡
// ---------------------------------------------------------------------------

/// 探测结果：有效网卡及其绑定的 reqwest::Client
pub struct NetClientPool {
    clients: Vec<(String, reqwest::Client)>, // (描述, client)
    index: std::sync::atomic::AtomicUsize,
}

impl NetClientPool {
    /// 轮询获取下一个 Client
    pub fn next(&self) -> &reqwest::Client {
        let idx = self
            .index
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            % self.clients.len();
        &self.clients[idx].1
    }

    pub fn len(&self) -> usize {
        self.clients.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }
}

/// 枚举所有活跃的非 loopback IPv4 网卡地址
fn detect_local_ipv4_addresses() -> Vec<(String, std::net::Ipv4Addr)> {
    use std::process::Command;

    let mut results = Vec::new();

    // 使用 ifconfig 解析 (macOS / Linux)
    let output = match Command::new("ifconfig").output() {
        Ok(o) => o,
        Err(_) => return results,
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut current_iface = String::new();
    let mut is_up = false;
    let mut is_loopback = false;

    for line in text.lines() {
        // 网卡头行: "en0: flags=8863<UP,...>"
        if !line.starts_with('\t')
            && !line.starts_with(' ')
            && let Some(colon_pos) = line.find(':')
        {
            current_iface = line[..colon_pos].to_string();
            is_up = line.contains("UP");
            is_loopback = line.contains("LOOPBACK");
        }

        // inet 行: "\tinet 192.168.1.100 netmask ..."
        if is_up && !is_loopback && line.contains("inet ") && !line.contains("inet6") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(idx) = parts.iter().position(|&s| s == "inet")
                && let Some(ip_str) = parts.get(idx + 1)
                && let Ok(ip) = ip_str.parse::<std::net::Ipv4Addr>()
                && !ip.is_loopback()
                && !ip.is_link_local()
            {
                results.push((current_iface.clone(), ip));
            }
        }
    }

    results
}

/// 探测网卡连通性并按出口 IP 去重，返回有效的 (网卡名, 本地IP, 出口IP)
async fn probe_and_dedup(
    interfaces: Vec<(String, std::net::Ipv4Addr)>,
) -> Vec<(String, std::net::Ipv4Addr)> {
    use std::collections::HashMap;

    let mut results: Vec<(String, std::net::Ipv4Addr, String)> = Vec::new();

    for (iface, local_ip) in &interfaces {
        info!("探测网卡 {} ({})...", iface, local_ip);

        let client = match reqwest::Client::builder()
            .local_address(std::net::IpAddr::V4(*local_ip))
            .timeout(std::time::Duration::from_secs(3))
            .build()
        {
            Ok(c) => c,
            Err(_) => continue,
        };

        match client.get("https://ifconfig.me/ip").send().await {
            Ok(resp) => {
                if let Ok(ext_ip) = resp.text().await {
                    let ext_ip = ext_ip.trim().to_string();
                    info!("  {} → 出口 IP: {}", iface, ext_ip);
                    results.push((iface.clone(), *local_ip, ext_ip));
                }
            }
            Err(_) => {
                info!("  {} → 不可达，跳过", iface);
            }
        }
    }

    // 按出口 IP 去重，每个出口只保留第一个网卡
    let mut seen_exits: HashMap<String, usize> = HashMap::new();
    let mut deduped = Vec::new();

    for (iface, local_ip, ext_ip) in results {
        if let Some(&existing_idx) = seen_exits.get(&ext_ip) {
            let (ref existing_iface, _, _) = deduped[existing_idx];
            info!(
                "  {} 与 {} 出口相同 ({})，跳过",
                iface, existing_iface, ext_ip
            );
        } else {
            seen_exits.insert(ext_ip, deduped.len());
            deduped.push((iface, local_ip, ()));
        }
    }

    deduped
        .into_iter()
        .map(|(iface, ip, _)| (iface, ip))
        .collect()
}

/// 构建多网卡 Client 池；如果只有 0-1 个有效网卡，返回 None
async fn build_multi_net_pool() -> Option<Arc<NetClientPool>> {
    step!("探测可用网卡...");
    let interfaces = detect_local_ipv4_addresses();

    if interfaces.len() <= 1 {
        info!("仅检测到 {} 个网卡，无需多网卡模式", interfaces.len());
        return None;
    }

    info!("检测到 {} 个网卡，开始连通性探测...", interfaces.len());

    let valid = probe_and_dedup(interfaces).await;

    if valid.len() <= 1 {
        info!("去重后仅 {} 个有效出口，无需多网卡模式", valid.len());
        return None;
    }

    let mut clients = Vec::new();
    for (iface, local_ip) in &valid {
        let client = reqwest::Client::builder()
            .local_address(std::net::IpAddr::V4(*local_ip))
            .build()
            .ok()?;
        clients.push((format!("{} ({})", iface, local_ip), client));
    }

    success!("多网卡模式就绪: {} 个出口", clients.len());
    for (desc, _) in &clients {
        info!("  {}", desc);
    }

    Some(Arc::new(NetClientPool {
        clients,
        index: std::sync::atomic::AtomicUsize::new(0),
    }))
}

// ---------------------------------------------------------------------------
// Local directory tree scanner
// ---------------------------------------------------------------------------

/// 递归扫描本地目录，返回扁平化的文件条目列表
pub fn scan_local_tree(
    root: &Path,
    exclude_patterns: &[String],
) -> Result<Vec<LocalFileEntry>, ClientError> {
    let root = root
        .canonicalize()
        .map_err(|e| ClientError::Other(format!("无法解析本地路径 {}: {}", root.display(), e)))?;
    if !root.is_dir() {
        return Err(ClientError::Other(format!(
            "本地路径不是目录: {}",
            root.display()
        )));
    }

    let mut entries = Vec::new();
    scan_local_recursive(&root, &root, exclude_patterns, &mut entries)?;
    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(entries)
}

fn scan_local_recursive(
    base: &Path,
    current: &Path,
    exclude_patterns: &[String],
    entries: &mut Vec<LocalFileEntry>,
) -> Result<(), ClientError> {
    let read_dir = std::fs::read_dir(current)?;

    for entry in read_dir {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(base)
            .map_err(|e| ClientError::Other(e.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");

        if is_excluded(&relative, exclude_patterns) {
            continue;
        }

        let metadata = entry.metadata()?;
        let modified_epoch_ms = metadata
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH)
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        if metadata.is_dir() {
            entries.push(LocalFileEntry {
                relative_path: relative.clone(),
                size: 0,
                modified_epoch_ms,
                is_dir: true,
            });
            scan_local_recursive(base, &path, exclude_patterns, entries)?;
        } else if metadata.is_file() {
            entries.push(LocalFileEntry {
                relative_path: relative,
                size: metadata.len() as i64,
                modified_epoch_ms,
                is_dir: false,
            });
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Remote directory tree scanner (PersonalNew)
// ---------------------------------------------------------------------------

/// 递归扫描远程目录树（PersonalNew），返回扁平化的文件条目列表
pub async fn scan_remote_tree_personal(
    config: &crate::config::Config,
    remote_path: &str,
    exclude_patterns: &[String],
) -> Result<Vec<RemoteFileEntry>, ClientError> {
    let mut config = config.clone();
    let host = api::get_personal_cloud_host(&mut config).await?;

    let parent_file_id = if remote_path == "/" || remote_path.is_empty() {
        "/".to_string()
    } else {
        api::get_file_id_by_path(&config, remote_path).await?
    };

    let mut entries = Vec::new();
    scan_remote_recursive_personal(
        &config,
        &host,
        &parent_file_id,
        "",
        exclude_patterns,
        &mut entries,
    )
    .await?;

    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(entries)
}

#[async_recursion::async_recursion]
async fn scan_remote_recursive_personal(
    config: &crate::config::Config,
    host: &str,
    parent_id: &str,
    prefix: &str,
    exclude_patterns: &[String],
    entries: &mut Vec<RemoteFileEntry>,
) -> Result<(), ClientError> {
    let url = format!("{}/file/list", host);
    let mut next_cursor = String::new();

    loop {
        let body = serde_json::json!({
            "parentFileId": parent_id,
            "pageInfo": {
                "pageCursor": next_cursor,
                "pageSize": 100
            },
            "orderBy": "updated_at",
            "orderDirection": "DESC"
        });

        let resp: crate::models::PersonalListResp =
            api::personal_api_request(config, &url, body, StorageType::PersonalNew).await?;

        if !resp.base.success {
            let msg = resp.base.message.as_deref().unwrap_or("未知错误");
            return Err(ClientError::Api(format!("列出远程目录失败: {}", msg)));
        }

        let data = match resp.data {
            Some(d) => d,
            None => break,
        };

        for item in &data.items {
            let name = item.name.as_deref().unwrap_or("");
            if name.is_empty() {
                continue;
            }

            let relative_path = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{}/{}", prefix, name)
            };

            if is_excluded(&relative_path, exclude_patterns) {
                continue;
            }

            let is_dir = item.file_type.as_deref() == Some("folder");
            let file_id = item.file_id.clone().unwrap_or_default();
            let size = item.size.unwrap_or(0);
            let modified_time = item
                .updated_at
                .as_deref()
                .or(item.update_date.as_deref())
                .or(item.last_modified.as_deref())
                .unwrap_or("")
                .to_string();

            entries.push(RemoteFileEntry {
                relative_path: relative_path.clone(),
                name: name.to_string(),
                file_id: file_id.clone(),
                size,
                modified_time,
                is_dir,
            });

            if is_dir {
                scan_remote_recursive_personal(
                    config,
                    host,
                    &file_id,
                    &relative_path,
                    exclude_patterns,
                    entries,
                )
                .await?;
            }
        }

        next_cursor = data.next_page_cursor.unwrap_or_default();
        if next_cursor.is_empty() {
            break;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Diff engine
// ---------------------------------------------------------------------------

/// 比较本地和远程文件树，产出差异列表
pub fn compute_diff(
    local_entries: &[LocalFileEntry],
    remote_entries: &[RemoteFileEntry],
) -> Vec<DiffEntry> {
    let local_map: HashMap<&str, &LocalFileEntry> = local_entries
        .iter()
        .map(|e| (e.relative_path.as_str(), e))
        .collect();

    let remote_map: HashMap<&str, &RemoteFileEntry> = remote_entries
        .iter()
        .map(|e| (e.relative_path.as_str(), e))
        .collect();

    let mut diffs = Vec::new();

    // Files only in local
    for local in local_entries {
        if local.is_dir {
            continue;
        }
        if !remote_map.contains_key(local.relative_path.as_str()) {
            diffs.push(DiffEntry {
                relative_path: local.relative_path.clone(),
                kind: DiffKind::OnlyLocal,
                local: Some(local.clone()),
                remote: None,
                is_dir: false,
            });
        }
    }

    // Files only in remote
    for remote in remote_entries {
        if remote.is_dir {
            continue;
        }
        if !local_map.contains_key(remote.relative_path.as_str()) {
            diffs.push(DiffEntry {
                relative_path: remote.relative_path.clone(),
                kind: DiffKind::OnlyRemote,
                local: None,
                remote: Some(remote.clone()),
                is_dir: false,
            });
        }
    }

    // Files in both — check for modifications
    for local in local_entries {
        if local.is_dir {
            continue;
        }
        if let Some(remote) = remote_map.get(local.relative_path.as_str()) {
            if remote.is_dir {
                continue;
            }
            // Size differs → modified
            if local.size != remote.size {
                let kind = determine_newer(local, remote);
                diffs.push(DiffEntry {
                    relative_path: local.relative_path.clone(),
                    kind,
                    local: Some(local.clone()),
                    remote: Some((*remote).clone()),
                    is_dir: false,
                });
            }
        }
    }

    diffs.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    diffs
}

/// 根据修改时间判断哪一端较新
fn determine_newer(local: &LocalFileEntry, remote: &RemoteFileEntry) -> DiffKind {
    let remote_ms = parse_remote_time_to_epoch_ms(&remote.modified_time);
    if local.modified_epoch_ms > remote_ms {
        DiffKind::LocalNewer
    } else {
        DiffKind::RemoteNewer
    }
}

/// 解析远程时间字符串为毫秒时间戳
fn parse_remote_time_to_epoch_ms(time_str: &str) -> i64 {
    if time_str.is_empty() {
        return 0;
    }
    // Try RFC3339
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(time_str) {
        return dt.timestamp_millis();
    }
    // Try "2024-01-01T12:00:00.000"
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M:%S%.f") {
        return dt.and_utc().timestamp_millis();
    }
    // Try "2024-01-01 12:00:00"
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(time_str, "%Y-%m-%d %H:%M:%S") {
        return dt.and_utc().timestamp_millis();
    }
    0
}

// ---------------------------------------------------------------------------
// BFS scan-and-diff engine
// ---------------------------------------------------------------------------

/// 本地目录索引，按目录分组
struct LocalDirIndex {
    /// 每个目录下直接包含的文件 (key: 相对目录路径, "" = 根目录)
    files: HashMap<String, Vec<LocalFileEntry>>,
    /// 每个目录下直接包含的子目录名
    subdirs: HashMap<String, HashSet<String>>,
}

fn build_local_dir_index(entries: &[LocalFileEntry]) -> LocalDirIndex {
    let mut files: HashMap<String, Vec<LocalFileEntry>> = HashMap::new();
    let mut subdirs: HashMap<String, HashSet<String>> = HashMap::new();

    for entry in entries {
        let parent = Path::new(&entry.relative_path)
            .parent()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        if entry.is_dir {
            let dir_name = Path::new(&entry.relative_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            subdirs.entry(parent).or_default().insert(dir_name);
        } else {
            files.entry(parent).or_default().push(entry.clone());
        }
    }

    LocalDirIndex { files, subdirs }
}

/// 收集某个本地目录下的所有文件（递归，用于本地独有目录）
fn collect_local_files_under(index: &LocalDirIndex, dir_path: &str) -> Vec<LocalFileEntry> {
    let mut result = Vec::new();
    let mut queue = vec![dir_path.to_string()];

    while let Some(current) = queue.pop() {
        if let Some(files) = index.files.get(&current) {
            result.extend(files.iter().cloned());
        }
        if let Some(subs) = index.subdirs.get(&current) {
            for sub in subs {
                let child = if current.is_empty() {
                    sub.clone()
                } else {
                    format!("{}/{}", current, sub)
                };
                queue.push(child);
            }
        }
    }

    result
}

/// 列出远程单个目录中的文件（不递归，处理分页）
async fn list_remote_dir_personal(
    config: &crate::config::Config,
    host: &str,
    parent_id: &str,
) -> Result<Vec<crate::models::PersonalFileItem>, ClientError> {
    let url = format!("{}/file/list", host);
    let mut all_items = Vec::new();
    let mut next_cursor = String::new();

    loop {
        let body = serde_json::json!({
            "parentFileId": parent_id,
            "pageInfo": {
                "pageCursor": next_cursor,
                "pageSize": 100
            },
            "orderBy": "updated_at",
            "orderDirection": "DESC"
        });

        let resp: crate::models::PersonalListResp =
            api::personal_api_request(config, &url, body, StorageType::PersonalNew).await?;

        if !resp.base.success {
            let msg = resp.base.message.as_deref().unwrap_or("未知错误");
            return Err(ClientError::Api(format!("列出远程目录失败: {}", msg)));
        }

        let data = match resp.data {
            Some(d) => d,
            None => break,
        };

        all_items.extend(data.items);

        next_cursor = data.next_page_cursor.unwrap_or_default();
        if next_cursor.is_empty() {
            break;
        }
    }

    Ok(all_items)
}

/// 将远程 API 返回的 item 转换为 RemoteFileEntry
fn item_to_remote_entry(
    item: &crate::models::PersonalFileItem,
    rel_dir: &str,
) -> Option<(RemoteFileEntry, bool)> {
    let name = item.name.as_deref().unwrap_or("");
    if name.is_empty() {
        return None;
    }

    let relative_path = if rel_dir.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", rel_dir, name)
    };

    let is_dir = item.file_type.as_deref() == Some("folder");
    let file_id = item.file_id.clone().unwrap_or_default();
    let size = item.size.unwrap_or(0);
    let modified_time = item
        .updated_at
        .as_deref()
        .or(item.update_date.as_deref())
        .or(item.last_modified.as_deref())
        .unwrap_or("")
        .to_string();

    Some((
        RemoteFileEntry {
            relative_path,
            name: name.to_string(),
            file_id,
            size,
            modified_time,
            is_dir,
        },
        is_dir,
    ))
}

/// BFS 广度优先扫描远程目录并同时计算差异
///
/// 相比先全量扫描远程再 diff 的方式，这里按目录逐层比较：
///   - 本地独有的子目录直接标记为 OnlyLocal，不发起远程请求
///   - 两端都有的子目录排入 BFS 队列继续比较
///   - 远程独有的子目录递归扫描并标记为 OnlyRemote
///
/// 返回 (差异列表, 远程文件条目, 远程文件数, 远程目录数)
async fn scan_and_diff_bfs_personal(
    config: &crate::config::Config,
    remote_path: &str,
    local_entries: &[LocalFileEntry],
    exclude_patterns: &[String],
) -> Result<(Vec<DiffEntry>, Vec<RemoteFileEntry>, usize, usize), ClientError> {
    let mut config = config.clone();
    let host = api::get_personal_cloud_host(&mut config).await?;

    let index = build_local_dir_index(local_entries);

    let remote_root_id = ensure_remote_root_personal(&config, &host, remote_path).await?;

    let mut diffs = Vec::new();
    let mut remote_entries = Vec::new();
    let mut remote_file_count = 0usize;
    let mut remote_dir_count = 0usize;

    // BFS 队列: (相对目录路径, 远程目录 ID)
    let mut queue: VecDeque<(String, String)> = VecDeque::new();
    queue.push_back(("".to_string(), remote_root_id));

    while let Some((rel_dir, remote_dir_id)) = queue.pop_front() {
        // 获取当前远程目录的文件列表（单层）
        let remote_items = list_remote_dir_personal(&config, &host, &remote_dir_id).await?;

        // 分类远程条目：文件 / 子目录
        let mut remote_files_here: HashMap<String, RemoteFileEntry> = HashMap::new();
        let mut remote_subdirs_here: HashMap<String, String> = HashMap::new(); // name -> file_id

        for item in &remote_items {
            let Some((entry, is_dir)) = item_to_remote_entry(item, &rel_dir) else {
                continue;
            };

            if is_excluded(&entry.relative_path, exclude_patterns) {
                continue;
            }

            if is_dir {
                remote_dir_count += 1;
                remote_subdirs_here.insert(entry.name.clone(), entry.file_id.clone());
                remote_entries.push(entry);
            } else {
                remote_file_count += 1;
                remote_files_here.insert(entry.name.clone(), entry.clone());
                remote_entries.push(entry);
            }
        }

        // 获取本地此目录下的文件和子目录
        let empty_files = Vec::new();
        let local_files = index.files.get(&rel_dir).unwrap_or(&empty_files);
        let empty_subdirs = HashSet::new();
        let local_subs = index.subdirs.get(&rel_dir).unwrap_or(&empty_subdirs);

        // ---- 比较文件 ----

        // 仅本地存在的文件
        for lf in local_files {
            let file_name = Path::new(&lf.relative_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if !remote_files_here.contains_key(&file_name) {
                diffs.push(DiffEntry {
                    relative_path: lf.relative_path.clone(),
                    kind: DiffKind::OnlyLocal,
                    local: Some(lf.clone()),
                    remote: None,
                    is_dir: false,
                });
            }
        }

        // 仅远程存在 + 两端都有
        for (name, rf) in &remote_files_here {
            if let Some(lf) = local_files.iter().find(|f| {
                Path::new(&f.relative_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
                    == *name
            }) {
                // 两端都有 — 检查差异
                if lf.size != rf.size {
                    let kind = determine_newer(lf, rf);
                    diffs.push(DiffEntry {
                        relative_path: lf.relative_path.clone(),
                        kind,
                        local: Some(lf.clone()),
                        remote: Some(rf.clone()),
                        is_dir: false,
                    });
                }
            } else {
                // 仅远程存在
                diffs.push(DiffEntry {
                    relative_path: rf.relative_path.clone(),
                    kind: DiffKind::OnlyRemote,
                    local: None,
                    remote: Some(rf.clone()),
                    is_dir: false,
                });
            }
        }

        // ---- 比较子目录 ----

        for local_sub in local_subs {
            let child_rel = if rel_dir.is_empty() {
                local_sub.clone()
            } else {
                format!("{}/{}", rel_dir, local_sub)
            };

            if let Some(remote_id) = remote_subdirs_here.get(local_sub) {
                // 两端都有 — 入队继续 BFS
                queue.push_back((child_rel, remote_id.clone()));
            } else {
                // 仅本地 — 所有子文件标记为 OnlyLocal（无需远程请求）
                let local_only_files = collect_local_files_under(&index, &child_rel);
                for lf in local_only_files {
                    diffs.push(DiffEntry {
                        relative_path: lf.relative_path.clone(),
                        kind: DiffKind::OnlyLocal,
                        local: Some(lf),
                        remote: None,
                        is_dir: false,
                    });
                }
            }
        }

        // 仅远程存在的子目录 — 递归扫描
        for (remote_sub, remote_id) in &remote_subdirs_here {
            if !local_subs.contains(remote_sub) {
                let child_rel = if rel_dir.is_empty() {
                    remote_sub.clone()
                } else {
                    format!("{}/{}", rel_dir, remote_sub)
                };

                let mut sub_entries = Vec::new();
                scan_remote_recursive_personal(
                    &config,
                    &host,
                    remote_id,
                    &child_rel,
                    exclude_patterns,
                    &mut sub_entries,
                )
                .await?;

                for rf in sub_entries {
                    if !rf.is_dir {
                        remote_file_count += 1;
                        diffs.push(DiffEntry {
                            relative_path: rf.relative_path.clone(),
                            kind: DiffKind::OnlyRemote,
                            local: None,
                            remote: Some(rf.clone()),
                            is_dir: false,
                        });
                    } else {
                        remote_dir_count += 1;
                    }
                    remote_entries.push(rf);
                }
            }
        }
    }

    diffs.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    remote_entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    Ok((diffs, remote_entries, remote_file_count, remote_dir_count))
}

// ---------------------------------------------------------------------------
// Interactive prompt
// ---------------------------------------------------------------------------

/// 交互式询问用户对差异文件的操作决策
pub fn prompt_interactive(diffs: &[DiffEntry]) -> Result<Vec<(usize, UserAction)>, ClientError> {
    use dialoguer::Select;

    let mut decisions = Vec::new();
    let mut global_action: Option<UserAction> = None;

    for (i, diff) in diffs.iter().enumerate() {
        if let Some(action) = global_action {
            let resolved = match action {
                UserAction::Sync => auto_sync_action(&diff.kind),
                UserAction::Upload => match diff.kind {
                    DiffKind::OnlyLocal | DiffKind::LocalNewer => UserAction::Upload,
                    DiffKind::OnlyRemote | DiffKind::RemoteNewer => UserAction::Skip,
                },
                UserAction::Download => match diff.kind {
                    DiffKind::OnlyRemote | DiffKind::RemoteNewer => UserAction::Download,
                    DiffKind::OnlyLocal | DiffKind::LocalNewer => UserAction::Skip,
                },
                _ => action,
            };
            decisions.push((i, resolved));
            continue;
        }

        let kind_desc = match &diff.kind {
            DiffKind::OnlyLocal => "仅本地存在",
            DiffKind::OnlyRemote => "仅远程存在",
            DiffKind::LocalNewer => "本地较新",
            DiffKind::RemoteNewer => "远程较新",
        };

        let size_info = if let Some(ref local) = diff.local {
            format!("本地: {} bytes", local.size)
        } else if let Some(ref remote) = diff.remote {
            format!("远程: {} bytes", remote.size)
        } else {
            String::new()
        };

        println!("\n  {} [{}] ({})", diff.relative_path, kind_desc, size_info);

        let items = vec![
            "始终只下载（本地 ← 远程）",
            "始终只上传（本地 → 远程）",
            "始终同步（本地 ↔ 远程）",
            "下载（本地 ← 远程）",
            "上传（本地 → 远程）",
            "跳过此文件",
        ];

        let default = match &diff.kind {
            DiffKind::OnlyLocal | DiffKind::LocalNewer => 4,
            DiffKind::OnlyRemote | DiffKind::RemoteNewer => 3,
        };

        let selection = Select::new()
            .with_prompt("请选择操作")
            .items(&items)
            .default(default)
            .interact()
            .map_err(|e| ClientError::Other(format!("交互输入错误: {}", e)))?;

        match selection {
            0 => {
                global_action = Some(UserAction::Download);
                let resolved = match diff.kind {
                    DiffKind::OnlyRemote | DiffKind::RemoteNewer => UserAction::Download,
                    DiffKind::OnlyLocal | DiffKind::LocalNewer => UserAction::Skip,
                };
                decisions.push((i, resolved));
            }
            1 => {
                global_action = Some(UserAction::Upload);
                let resolved = match diff.kind {
                    DiffKind::OnlyLocal | DiffKind::LocalNewer => UserAction::Upload,
                    DiffKind::OnlyRemote | DiffKind::RemoteNewer => UserAction::Skip,
                };
                decisions.push((i, resolved));
            }
            2 => {
                global_action = Some(UserAction::Sync);
                decisions.push((i, auto_sync_action(&diff.kind)));
            }
            3 => decisions.push((i, UserAction::Download)),
            4 => decisions.push((i, UserAction::Upload)),
            5 => decisions.push((i, UserAction::Skip)),
            _ => decisions.push((i, UserAction::Skip)),
        }
    }

    Ok(decisions)
}

// ---------------------------------------------------------------------------
// Sync action executors
// ---------------------------------------------------------------------------

/// 确保远程根路径存在（PersonalNew），如不存在则逐级递归创建，返回最终目录的 file_id
async fn ensure_remote_root_personal(
    config: &crate::config::Config,
    host: &str,
    remote_path: &str,
) -> Result<String, ClientError> {
    if remote_path == "/" || remote_path.is_empty() {
        return Ok("/".to_string());
    }

    let parts: Vec<&str> = remote_path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    let mut current_parent_id = "/".to_string();

    for part in &parts {
        let files = api::list_personal_files(config, &current_parent_id).await?;
        let existing = files
            .iter()
            .find(|f| f.name.as_deref() == Some(part) && f.file_type.as_deref() == Some("folder"));

        if let Some(dir) = existing {
            current_parent_id = dir.file_id.clone().unwrap_or_default();
        } else {
            let url = format!("{}/file/create", host);
            let body = serde_json::json!({
                "parentFileId": current_parent_id,
                "name": part,
                "description": "",
                "type": "folder",
                "fileRenameMode": "force_rename"
            });

            let resp: crate::models::PersonalUploadResp =
                api::personal_api_request(config, &url, body, StorageType::PersonalNew).await?;

            if !resp.base.success {
                return Err(ClientError::Api(format!(
                    "创建远程目录失败 {}: {}",
                    part,
                    resp.base.message.as_deref().unwrap_or("未知错误")
                )));
            }

            current_parent_id = resp.data.and_then(|d| d.file_id).unwrap_or_default();

            info!("已创建远程目录: {}", part);
        }
    }

    Ok(current_parent_id)
}

/// 确保远程目录存在（PersonalNew），带中间路径缓存
/// `created_dirs` 记录本次会话中新创建的目录，其子目录无需 list 直接创建
async fn ensure_remote_dir_personal_cached(
    config: &crate::config::Config,
    host: &str,
    remote_base_id: &str,
    relative_dir: &str,
    cache: &mut HashMap<String, String>,
) -> Result<String, ClientError> {
    if relative_dir.is_empty() {
        return Ok(remote_base_id.to_string());
    }

    // 如果完整路径已缓存，直接返回
    if let Some(id) = cache.get(relative_dir) {
        return Ok(id.clone());
    }

    let parts: Vec<&str> = relative_dir.split('/').filter(|s| !s.is_empty()).collect();
    let mut current_parent_id = remote_base_id.to_string();
    let mut path_so_far = String::new();
    // 一旦某级目录是新创建的，后续子目录必定不存在，跳过 list
    let mut parent_is_new = false;

    for part in &parts {
        if !path_so_far.is_empty() {
            path_so_far.push('/');
        }
        path_so_far.push_str(part);

        // 检查中间路径缓存
        if let Some(id) = cache.get(&path_so_far) {
            current_parent_id = id.clone();
            continue;
        }

        let mut found = false;

        // 如果父目录是本次新创建的，子目录必定不存在，直接创建
        if !parent_is_new {
            let files = api::list_personal_files(config, &current_parent_id).await?;
            let existing = files.iter().find(|f| {
                f.name.as_deref() == Some(part) && f.file_type.as_deref() == Some("folder")
            });

            if let Some(dir) = existing {
                current_parent_id = dir.file_id.clone().unwrap_or_default();
                found = true;
            }
        }

        if !found {
            let url = format!("{}/file/create", host);
            let body = serde_json::json!({
                "parentFileId": current_parent_id,
                "name": part,
                "description": "",
                "type": "folder",
                "fileRenameMode": "force_rename"
            });

            let resp: crate::models::PersonalUploadResp =
                api::personal_api_request(config, &url, body, StorageType::PersonalNew).await?;

            if !resp.base.success {
                return Err(ClientError::Api(format!(
                    "创建远程目录失败 {}: {}",
                    part,
                    resp.base.message.as_deref().unwrap_or("未知错误")
                )));
            }

            current_parent_id = resp.data.and_then(|d| d.file_id).unwrap_or_default();
            parent_is_new = true;
        }

        cache.insert(path_so_far.clone(), current_parent_id.clone());
    }

    Ok(current_parent_id)
}

/// 上传单个文件到远程（PersonalNew），支持进度回调
async fn upload_file_personal(
    config: &crate::config::Config,
    host: &str,
    local_file: &Path,
    parent_file_id: &str,
    file_name: &str,
    progress_bar: Option<&indicatif::ProgressBar>,
    http_client: &reqwest::Client,
) -> Result<(), ClientError> {
    let metadata = std::fs::metadata(local_file)?;
    let file_size = metadata.len() as i64;

    let content_hash = crate::utils::crypto::calc_file_sha256(local_file.to_str().unwrap_or(""))?;

    let part_size =
        crate::commands::upload::get_part_size(file_size, config.custom_upload_part_size);
    let mut part_count = (file_size + part_size - 1) / part_size;
    if part_count == 0 {
        part_count = 1;
    }

    let first_part_infos: Vec<serde_json::Value> = (0..part_count.min(100))
        .map(|i| {
            let start = i * part_size;
            let byte_size = if file_size - start > part_size {
                part_size
            } else {
                file_size - start
            };
            serde_json::json!({
                "partNumber": (i + 1) as i32,
                "partSize": byte_size,
                "parallelHashCtx": {
                    "partOffset": start
                }
            })
        })
        .collect();

    let url = format!("{}/file/create", host);
    let body = serde_json::json!({
        "contentHash": content_hash,
        "contentHashAlgorithm": "SHA256",
        "contentType": "application/octet-stream",
        "parallelUpload": false,
        "partInfos": first_part_infos,
        "size": file_size,
        "parentFileId": parent_file_id,
        "name": file_name,
        "type": "file",
        "fileRenameMode": "auto_rename"
    });

    let resp: crate::models::PersonalUploadResp =
        api::personal_api_request(config, &url, body, StorageType::PersonalNew).await?;

    if !resp.base.success {
        return Err(ClientError::Api(format!(
            "创建上传任务失败: {}",
            resp.base.message.as_deref().unwrap_or("未知错误")
        )));
    }

    let data = match resp.data {
        Some(d) => d,
        None => return Ok(()),
    };

    if data.exist.unwrap_or(false) {
        return Ok(());
    }

    if let Some(part_infos_response) = data.part_infos {
        if part_infos_response.is_empty() {
            return Ok(());
        }

        let file_id_val = data.file_id.clone().unwrap_or_default();
        upload_parts_personal(
            config,
            host,
            local_file,
            &data.upload_id.unwrap_or_default(),
            &file_id_val,
            file_size,
            &content_hash,
            part_size,
            progress_bar,
            http_client,
        )
        .await?;
    }

    Ok(())
}

/// 分片上传（PersonalNew）— 使用 reqwest 异步流式上传，支持实时进度
#[allow(clippy::too_many_arguments)]
async fn upload_parts_personal(
    config: &crate::config::Config,
    host: &str,
    local_path: &Path,
    upload_id: &str,
    file_id: &str,
    file_size: i64,
    content_hash: &str,
    part_size: i64,
    progress_bar: Option<&indicatif::ProgressBar>,
    http_client: &reqwest::Client,
) -> Result<(), ClientError> {
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};

    let mut file = File::open(local_path)?;
    let part_count = (file_size + part_size - 1) / part_size;
    let mut upload_urls: HashMap<i32, String> = HashMap::new();

    for batch_start in (0..part_count as usize).step_by(100) {
        let batch_end = std::cmp::min(batch_start + 100, part_count as usize);
        let url = format!("{}/file/getUploadUrl", host);

        let part_infos: Vec<serde_json::Value> = (batch_start..batch_end)
            .map(|i| {
                let start = i as i64 * part_size;
                let byte_size = if file_size - start > part_size {
                    part_size
                } else {
                    file_size - start
                };
                serde_json::json!({
                    "partNumber": (i + 1) as i32,
                    "partSize": byte_size
                })
            })
            .collect();

        let body = serde_json::json!({
            "fileId": file_id,
            "uploadId": upload_id,
            "partInfos": part_infos,
            "commonAccountInfo": {
                "account": config.account,
                "accountType": 1
            }
        });

        let resp_json: serde_json::Value =
            api::personal_api_request(config, &url, body, StorageType::PersonalNew).await?;

        if let Some(part_infos) = resp_json
            .get("data")
            .and_then(|d| d.get("partInfos"))
            .and_then(|p| p.as_array())
        {
            for info in part_infos {
                if let (Some(part_num), Some(url)) = (
                    info.get("partNumber").and_then(|n| n.as_i64()),
                    info.get("uploadUrl").and_then(|u| u.as_str()),
                ) {
                    upload_urls.insert(part_num as i32, url.to_string());
                }
            }
        }
    }

    for i in 0..part_count {
        file.seek(SeekFrom::Start(i as u64 * part_size as u64))?;
        let read_size = if (i + 1) * part_size > file_size {
            file_size - i * part_size
        } else {
            part_size
        };

        let mut buffer = vec![0u8; read_size as usize];
        file.read_exact(&mut buffer)?;

        let part_number = (i + 1) as i32;
        let upload_url = upload_urls
            .get(&part_number)
            .cloned()
            .ok_or_else(|| ClientError::Api(format!("找不到分片 {} 的上传URL", part_number)))?;

        let content_len = buffer.len();
        let pb_clone = progress_bar.cloned();

        // Stream the buffer in 256KB chunks with real-time progress
        let stream =
            futures_util::stream::unfold((0usize, buffer, pb_clone), |(pos, buf, pb)| async move {
                if pos >= buf.len() {
                    return None;
                }
                let end = std::cmp::min(pos + 256 * 1024, buf.len());
                let chunk = buf[pos..end].to_vec();
                if let Some(ref pb) = pb {
                    pb.inc((end - pos) as u64);
                }
                Some((Ok::<_, std::io::Error>(chunk), (end, buf, pb)))
            });

        let resp = http_client
            .put(&upload_url)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", content_len.to_string())
            .header("Origin", "https://yun.139.com")
            .header("Referer", "https://yun.139.com/")
            .body(reqwest::Body::wrap_stream(stream))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().as_u16() == 200 => {}
            Ok(r) => {
                return Err(ClientError::Api(format!(
                    "分片 {} 上传失败: HTTP {}",
                    part_number,
                    r.status().as_u16()
                )));
            }
            Err(e) => {
                return Err(ClientError::Api(format!(
                    "分片 {} 上传失败: {}",
                    part_number, e
                )));
            }
        }
    }

    // Complete upload
    let complete_url = format!("{}/file/complete", host);
    let body = serde_json::json!({
        "contentHash": content_hash,
        "contentHashAlgorithm": "SHA256",
        "uploadId": upload_id,
        "fileId": file_id,
    });

    let _: serde_json::Value =
        api::personal_api_request(config, &complete_url, body, StorageType::PersonalNew).await?;

    Ok(())
}

/// 下载单个文件（PersonalNew），支持进度回调
async fn download_file_personal(
    config: &crate::config::Config,
    file_id: &str,
    local_path: &Path,
    progress_bar: Option<&indicatif::ProgressBar>,
    http_client: &reqwest::Client,
) -> Result<(), ClientError> {
    let download_url = api::get_personal_download_link(config, file_id).await?;
    if download_url.is_empty() {
        return Err(ClientError::Api("获取下载链接失败: URL为空".to_string()));
    }

    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let response = http_client.get(&download_url).send().await?;
    let total_size = response.content_length().unwrap_or(0);

    if let Some(pb) = progress_bar {
        pb.set_length(total_size);
        pb.set_position(0);
    }

    let mut file = std::fs::File::create(local_path)?;
    let mut stream = response.bytes_stream();

    use futures_util::StreamExt;
    use std::io::Write;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        if let Some(pb) = progress_bar {
            pb.inc(chunk.len() as u64);
        }
    }

    if let Some(pb) = progress_bar {
        pb.set_position(total_size);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Main execute
// ---------------------------------------------------------------------------

pub async fn execute(args: SyncArgs) -> Result<(), ClientError> {
    // 未提供路径参数时显示帮助信息
    if args.local_path.is_none() || args.remote_path.is_none() {
        use clap::CommandFactory;
        SyncArgs::command().name("cloud139 sync").print_help().ok();
        println!();
        return Ok(());
    }

    let local_path = args.local_path.as_ref().unwrap().clone();
    let remote_path = args.remote_path.as_ref().unwrap().clone();

    // Validate mutually exclusive flags
    let mode_count = args.upload_only as u8 + args.download_only as u8 + args.two_way as u8;
    if mode_count > 1 {
        error!("--upload-only, --download-only, --two-way 三个参数互斥，只能指定一个");
        return Err(ClientError::Other("同步模式参数互斥".to_string()));
    }

    if args.concurrency == 0 {
        error!("并行数不能为 0");
        return Err(ClientError::Other("并行数不能为 0".to_string()));
    }

    let sync_mode = if args.upload_only {
        SyncMode::UploadOnly
    } else if args.download_only {
        SyncMode::DownloadOnly
    } else if args.two_way {
        SyncMode::TwoWay
    } else {
        SyncMode::Interactive
    };

    // macOS 自动排除系统隐藏文件
    if is_macos() {
        info!("检测到 macOS 环境，将自动排除系统隐藏文件 (.DS_Store, ._* 等)");
    }

    info!("并行数: {}", args.concurrency);

    // 多网卡探测
    let net_pool = if args.multi_net {
        build_multi_net_pool().await
    } else {
        None
    };

    let config = crate::config::Config::load().map_err(ClientError::Config)?;
    let storage_type = config.storage_type();

    match storage_type {
        StorageType::PersonalNew => {
            execute_personal(
                &config,
                &args,
                sync_mode,
                &local_path,
                &remote_path,
                net_pool,
            )
            .await?;
        }
        _ => {
            error!("暂不支持该存储类型的同步功能，目前仅支持个人云 (PersonalNew)");
            return Err(ClientError::Other("暂不支持该存储类型的同步".to_string()));
        }
    }

    Ok(())
}

async fn execute_personal(
    config: &crate::config::Config,
    args: &SyncArgs,
    sync_mode: SyncMode,
    local_path: &str,
    remote_path: &str,
    net_pool: Option<Arc<NetClientPool>>,
) -> Result<(), ClientError> {
    let local_root = Path::new(local_path);

    // Ensure local directory exists
    if !local_root.exists() {
        if sync_mode == SyncMode::UploadOnly {
            error!("本地目录不存在: {}", local_path);
            return Err(ClientError::FileNotFound);
        }
        std::fs::create_dir_all(local_root)?;
        info!("已创建本地目录: {}", local_path);
    }

    // Step 1: Scan local
    step!("扫描本地目录: {}", local_path);
    let local_entries = scan_local_tree(local_root, &args.exclude)?;
    let local_file_count = local_entries.iter().filter(|e| !e.is_dir).count();
    let local_dir_count = local_entries.iter().filter(|e| e.is_dir).count();
    info!(
        "本地: {} 个文件, {} 个目录",
        local_file_count, local_dir_count
    );

    // Step 2: BFS 扫描远程并同时计算差异
    step!("扫描远程目录并计算差异: {}", remote_path);
    let (diffs, remote_entries, remote_file_count, remote_dir_count) =
        scan_and_diff_bfs_personal(config, remote_path, &local_entries, &args.exclude).await?;
    info!(
        "远程: {} 个文件, {} 个目录",
        remote_file_count, remote_dir_count
    );

    if diffs.is_empty() {
        success!("本地与远程目录完全一致，无需同步");
        return Ok(());
    }

    let only_local_count = diffs
        .iter()
        .filter(|d| matches!(d.kind, DiffKind::OnlyLocal))
        .count();
    let only_remote_count = diffs
        .iter()
        .filter(|d| matches!(d.kind, DiffKind::OnlyRemote))
        .count();
    let modified_count = diffs
        .iter()
        .filter(|d| matches!(d.kind, DiffKind::LocalNewer | DiffKind::RemoteNewer))
        .count();

    info!(
        "差异: {} 仅本地, {} 仅远程, {} 已修改",
        only_local_count, only_remote_count, modified_count
    );

    // Step 4: Determine actions
    let actions: Vec<(usize, UserAction)> = match sync_mode {
        SyncMode::UploadOnly => diffs
            .iter()
            .enumerate()
            .filter_map(|(i, d)| match d.kind {
                DiffKind::OnlyLocal | DiffKind::LocalNewer => Some((i, UserAction::Upload)),
                _ => None,
            })
            .collect(),
        SyncMode::DownloadOnly => diffs
            .iter()
            .enumerate()
            .filter_map(|(i, d)| match d.kind {
                DiffKind::OnlyRemote | DiffKind::RemoteNewer => Some((i, UserAction::Download)),
                _ => None,
            })
            .collect(),
        SyncMode::TwoWay => diffs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let action = match d.kind {
                    DiffKind::OnlyLocal | DiffKind::LocalNewer => UserAction::Upload,
                    DiffKind::OnlyRemote | DiffKind::RemoteNewer => UserAction::Download,
                };
                (i, action)
            })
            .collect(),
        SyncMode::Interactive => {
            if args.dry_run {
                // In dry-run + interactive, just show the diff
                diffs
                    .iter()
                    .enumerate()
                    .map(|(i, d)| {
                        let action = match d.kind {
                            DiffKind::OnlyLocal | DiffKind::LocalNewer => UserAction::Upload,
                            DiffKind::OnlyRemote | DiffKind::RemoteNewer => UserAction::Download,
                        };
                        (i, action)
                    })
                    .collect()
            } else {
                prompt_interactive(&diffs)?
            }
        }
    };

    // Step 5: Dry run or execute
    if args.dry_run {
        println!("\n--- 预览模式 (dry-run) ---");
        for (i, action) in &actions {
            let diff = &diffs[*i];
            let action_str = match action {
                UserAction::Upload => "↑ 上传",
                UserAction::Download => "↓ 下载",
                UserAction::Skip => "- 跳过",
                UserAction::Sync => "↔ 同步",
            };
            println!("  {} {}", action_str, diff.relative_path);
        }
        let upload_count = actions
            .iter()
            .filter(|(_, a)| *a == UserAction::Upload)
            .count();
        let download_count = actions
            .iter()
            .filter(|(_, a)| *a == UserAction::Download)
            .count();
        info!(
            "预览: 将上传 {} 个文件, 下载 {} 个文件",
            upload_count, download_count
        );
        return Ok(());
    }

    // Execute actions with parallel transfers and progress bar
    let mut config_mut = config.clone();
    let host = api::get_personal_cloud_host(&mut config_mut).await?;

    let remote_base_id = ensure_remote_root_personal(config, &host, remote_path).await?;

    // Build remote file_id lookup (owned)
    let remote_id_map: HashMap<String, String> = remote_entries
        .iter()
        .map(|e| (e.relative_path.clone(), e.file_id.clone()))
        .collect();

    // Separate upload and download actions; skip immediately counted
    let mut upload_tasks: Vec<(usize, &DiffEntry)> = Vec::new();
    let mut download_tasks: Vec<(usize, &DiffEntry)> = Vec::new();
    let mut skipped = 0u32;

    for (i, action) in &actions {
        let diff = &diffs[*i];
        match action {
            UserAction::Upload => upload_tasks.push((*i, diff)),
            UserAction::Download => download_tasks.push((*i, diff)),
            UserAction::Skip => skipped += 1,
            UserAction::Sync => {
                // Sync 在交互阶段已解析为 Upload/Download，此处做兜底
                match diff.kind {
                    DiffKind::OnlyLocal | DiffKind::LocalNewer => upload_tasks.push((*i, diff)),
                    DiffKind::OnlyRemote | DiffKind::RemoteNewer => download_tasks.push((*i, diff)),
                }
            }
        }
    }

    let total_transfer = upload_tasks.len() + download_tasks.len();
    if total_transfer == 0 {
        success!("没有需要传输的文件 ({} 已跳过)", skipped);
        return Ok(());
    }

    // Ensure remote directories exist BEFORE parallel uploads (sequential, deduped)
    let mut dir_id_cache: HashMap<String, String> = HashMap::new();
    let mut upload_task_data: Vec<(String, String, String, i64)> = Vec::new(); // (relative_path, parent_id, file_name, size)

    if !upload_tasks.is_empty() {
        // 先收集所有需要的唯一父目录
        let mut unique_dirs: Vec<String> = upload_tasks
            .iter()
            .map(|(_i, diff)| {
                Path::new(&diff.relative_path)
                    .parent()
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default()
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        unique_dirs.sort();

        let dirs_to_create: Vec<&String> = unique_dirs
            .iter()
            .filter(|d| !d.is_empty() && !dir_id_cache.contains_key(d.as_str()))
            .collect();
        if !dirs_to_create.is_empty() {
            step!("创建远程目录 ({} 个)...", dirs_to_create.len());
        }
        for dir in &unique_dirs {
            if !dir_id_cache.contains_key(dir.as_str()) {
                let id = ensure_remote_dir_personal_cached(
                    config,
                    &host,
                    &remote_base_id,
                    dir,
                    &mut dir_id_cache,
                )
                .await?;
                dir_id_cache.insert(dir.clone(), id);
            }
        }

        step!("准备上传任务...");
        for (_i, diff) in &upload_tasks {
            let parent_rel = Path::new(&diff.relative_path)
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();

            let parent_id = dir_id_cache
                .get(&parent_rel)
                .cloned()
                .unwrap_or_else(|| remote_base_id.clone());

            let file_name = Path::new(&diff.relative_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let local_file = local_root.join(&diff.relative_path);
            let size = std::fs::metadata(&local_file)
                .map(|m| m.len() as i64)
                .unwrap_or(0);

            upload_task_data.push((diff.relative_path.clone(), parent_id, file_name, size));
        }
    }

    // Prepare download items (before progress bars start)
    let download_items: Vec<(String, String)> = download_tasks
        .iter()
        .filter_map(|(_i, diff)| {
            remote_id_map
                .get(&diff.relative_path)
                .map(|fid| (diff.relative_path.clone(), fid.clone()))
        })
        .collect();

    let missing_ids = download_tasks.len() - download_items.len();

    // Build a size lookup from remote_entries for progress bar length
    let remote_size_map: HashMap<String, i64> = remote_entries
        .iter()
        .map(|e| (e.relative_path.clone(), e.size))
        .collect();

    // Log summary before progress bars take over the terminal
    if !upload_task_data.is_empty() {
        step!(
            "开始并行上传 ({} 个文件, 并行数: {})",
            upload_task_data.len(),
            args.concurrency
        );
    }
    if !download_items.is_empty() {
        step!(
            "开始并行下载 ({} 个文件, 并行数: {})",
            download_items.len(),
            args.concurrency
        );
    }

    // --- Progress bar setup (all println! after this point must go through mp) ---
    use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
    use std::sync::atomic::{AtomicU32, Ordering};

    let config = Arc::new(config.clone());
    let host = Arc::new(host);
    let local_root = Arc::new(local_root.to_path_buf());
    let concurrency = args.concurrency;

    // 构建 HTTP Client 池（多网卡或默认单 Client）
    let default_client = reqwest::Client::new();
    let client_pool: Arc<Vec<reqwest::Client>> = Arc::new(match &net_pool {
        Some(pool) => pool.clients.iter().map(|(_, c)| c.clone()).collect(),
        None => vec![default_client],
    });
    let client_index = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let mp = if crate::utils::logger::is_quiet() {
        MultiProgress::with_draw_target(ProgressDrawTarget::hidden())
    } else {
        MultiProgress::new()
    };

    // Overall progress bar
    let overall_style =
        ProgressStyle::with_template("{prefix} [{bar:30.cyan/dim}] {pos}/{len} ({percent}%) {msg}")
            .unwrap()
            .progress_chars("█▓░");

    let overall_pb = mp.add(ProgressBar::new(total_transfer as u64));
    overall_pb.set_style(overall_style);
    overall_pb.set_prefix("\x1b[34msync\x1b[0m");
    overall_pb.set_message("");

    // Per-task progress bar style (with speed)
    let task_style = ProgressStyle::with_template(
        "     {prefix} [{bar:25.green/dim}] {bytes}/{total_bytes} {bytes_per_sec} {msg}",
    )
    .unwrap()
    .progress_chars("━╸─");

    let uploaded = Arc::new(AtomicU32::new(0));
    let downloaded = Arc::new(AtomicU32::new(0));
    let error_count = Arc::new(AtomicU32::new(0));
    let failed_files: Arc<std::sync::Mutex<Vec<(String, String)>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    // Parallel uploads
    if !upload_task_data.is_empty() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
        let mut join_set = tokio::task::JoinSet::new();

        for (relative_path, parent_id, file_name, file_size) in upload_task_data {
            let config = Arc::clone(&config);
            let host = Arc::clone(&host);
            let local_root = Arc::clone(&local_root);
            let uploaded = Arc::clone(&uploaded);
            let error_count = Arc::clone(&error_count);
            let failed_files = Arc::clone(&failed_files);
            let semaphore = Arc::clone(&semaphore);
            let task_style = task_style.clone();
            let mp = mp.clone();
            let overall_pb = overall_pb.clone();

            // 文件级 round-robin 选择 HTTP Client
            let idx = client_index.fetch_add(1, Ordering::Relaxed) % client_pool.len();
            let http_client = client_pool[idx].clone();

            join_set.spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();

                // Create progress bar only when task becomes active
                let pb = mp.insert_before(&overall_pb, ProgressBar::new(file_size as u64));
                pb.set_style(task_style);
                let display_name = truncate_filename(&relative_path, 30);
                pb.set_prefix(format!("↑ {}", display_name));
                pb.set_position(0);

                let local_file = local_root.join(&relative_path);

                let result = upload_file_personal(
                    &config,
                    &host,
                    &local_file,
                    &parent_id,
                    &file_name,
                    Some(&pb),
                    &http_client,
                )
                .await;

                match result {
                    Ok(()) => {
                        pb.set_position(file_size as u64);
                        pb.finish_and_clear();
                        uploaded.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        pb.abandon_with_message(format!("失败: {}", err_msg));
                        error_count.fetch_add(1, Ordering::Relaxed);
                        failed_files
                            .lock()
                            .unwrap()
                            .push((format!("↑ {}", relative_path), err_msg));
                    }
                }
                overall_pb.inc(1);
            });
        }

        while join_set.join_next().await.is_some() {}
    }

    // Count missing remote IDs as errors
    if missing_ids > 0 {
        error_count.fetch_add(missing_ids as u32, Ordering::Relaxed);
        overall_pb.inc(missing_ids as u64);
    }

    // Parallel downloads
    if !download_items.is_empty() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
        let mut join_set = tokio::task::JoinSet::new();

        for (relative_path, file_id) in download_items {
            let config = Arc::clone(&config);
            let local_root = Arc::clone(&local_root);
            let downloaded = Arc::clone(&downloaded);
            let error_count = Arc::clone(&error_count);
            let failed_files = Arc::clone(&failed_files);
            let semaphore = Arc::clone(&semaphore);
            let task_style = task_style.clone();
            let mp = mp.clone();
            let overall_pb = overall_pb.clone();

            let est_size = remote_size_map.get(&relative_path).copied().unwrap_or(0) as u64;

            // 文件级 round-robin 选择 HTTP Client
            let idx = client_index.fetch_add(1, Ordering::Relaxed) % client_pool.len();
            let http_client = client_pool[idx].clone();

            join_set.spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();

                // Create progress bar only when task becomes active
                let pb = mp.insert_before(&overall_pb, ProgressBar::new(est_size));
                pb.set_style(task_style);
                let display_name = truncate_filename(&relative_path, 30);
                pb.set_prefix(format!("↓ {}", display_name));
                pb.set_position(0);

                let local_file = local_root.join(&relative_path);

                let result =
                    download_file_personal(&config, &file_id, &local_file, Some(&pb), &http_client)
                        .await;

                match result {
                    Ok(()) => {
                        pb.finish_and_clear();
                        downloaded.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        pb.abandon_with_message(format!("失败: {}", err_msg));
                        error_count.fetch_add(1, Ordering::Relaxed);
                        failed_files
                            .lock()
                            .unwrap()
                            .push((format!("↓ {}", relative_path), err_msg));
                    }
                }
                overall_pb.inc(1);
            });
        }

        while join_set.join_next().await.is_some() {}
    }

    overall_pb.finish_and_clear();

    let uploaded = uploaded.load(Ordering::Relaxed);
    let downloaded = downloaded.load(Ordering::Relaxed);
    let errors = error_count.load(Ordering::Relaxed);

    // Summary
    println!();
    success!(
        "同步完成: ↑ {} 已上传, ↓ {} 已下载, {} 已跳过, {} 错误",
        uploaded,
        downloaded,
        skipped,
        errors
    );

    // Failed files report
    let failures = failed_files.lock().unwrap();
    if !failures.is_empty() {
        println!();
        error!("以下文件传输失败:");
        for (path, reason) in failures.iter() {
            error!("  {} — {}", path, reason);
        }
    }

    Ok(())
}

/// 截断文件名用于进度条显示
fn truncate_filename(name: &str, max_len: usize) -> String {
    let char_count = name.chars().count();
    if char_count <= max_len {
        name.to_string()
    } else if max_len <= 3 {
        name.chars().take(max_len).collect()
    } else {
        let tail_len = max_len - 3;
        let start = char_count - tail_len;
        let tail: String = name.chars().skip(start).collect();
        format!("...{}", tail)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_excluded_simple_glob() {
        let patterns = vec![".*".to_string()];
        assert!(is_excluded(".git", &patterns));
        assert!(is_excluded(".DS_Store", &patterns));
        assert!(is_excluded("sub/.hidden", &patterns));
        assert!(!is_excluded("readme.md", &patterns));
    }

    #[test]
    fn test_is_excluded_wildcard() {
        let patterns = vec!["*.tmp".to_string()];
        assert!(is_excluded("file.tmp", &patterns));
        assert!(is_excluded("dir/file.tmp", &patterns));
        assert!(!is_excluded("file.txt", &patterns));
    }

    #[test]
    fn test_is_excluded_multiple_patterns() {
        let patterns = vec![
            ".*".to_string(),
            "*.log".to_string(),
            "node_modules".to_string(),
        ];
        assert!(is_excluded(".gitignore", &patterns));
        assert!(is_excluded("app.log", &patterns));
        assert!(is_excluded("node_modules", &patterns));
        assert!(!is_excluded("main.rs", &patterns));
    }

    #[test]
    fn test_is_excluded_dir_children() {
        let patterns = vec![".obsidian".to_string()];
        assert!(is_excluded(".obsidian", &patterns));
        assert!(is_excluded(".obsidian/config.json", &patterns));
        assert!(is_excluded(".obsidian/plugins/test.js", &patterns));
        assert!(!is_excluded("notes/readme.md", &patterns));

        let patterns2 = vec!["node_modules".to_string()];
        assert!(is_excluded("node_modules/pkg/index.js", &patterns2));
        assert!(!is_excluded("src/index.js", &patterns2));
    }

    #[test]
    fn test_compute_diff_only_local() {
        let local = vec![LocalFileEntry {
            relative_path: "file1.txt".to_string(),
            size: 100,
            modified_epoch_ms: 1000,
            is_dir: false,
        }];
        let remote: Vec<RemoteFileEntry> = vec![];

        let diffs = compute_diff(&local, &remote);
        assert_eq!(diffs.len(), 1);
        assert!(matches!(diffs[0].kind, DiffKind::OnlyLocal));
    }

    #[test]
    fn test_compute_diff_only_remote() {
        let local: Vec<LocalFileEntry> = vec![];
        let remote = vec![RemoteFileEntry {
            relative_path: "file1.txt".to_string(),
            name: "file1.txt".to_string(),
            file_id: "id1".to_string(),
            size: 200,
            modified_time: "2024-01-01T12:00:00.000".to_string(),
            is_dir: false,
        }];

        let diffs = compute_diff(&local, &remote);
        assert_eq!(diffs.len(), 1);
        assert!(matches!(diffs[0].kind, DiffKind::OnlyRemote));
    }

    #[test]
    fn test_compute_diff_same_file() {
        let local = vec![LocalFileEntry {
            relative_path: "file1.txt".to_string(),
            size: 100,
            modified_epoch_ms: 1000,
            is_dir: false,
        }];
        let remote = vec![RemoteFileEntry {
            relative_path: "file1.txt".to_string(),
            name: "file1.txt".to_string(),
            file_id: "id1".to_string(),
            size: 100,
            modified_time: "".to_string(),
            is_dir: false,
        }];

        let diffs = compute_diff(&local, &remote);
        assert_eq!(diffs.len(), 0);
    }

    #[test]
    fn test_compute_diff_modified() {
        let local = vec![LocalFileEntry {
            relative_path: "file1.txt".to_string(),
            size: 200,
            modified_epoch_ms: 2000000,
            is_dir: false,
        }];
        let remote = vec![RemoteFileEntry {
            relative_path: "file1.txt".to_string(),
            name: "file1.txt".to_string(),
            file_id: "id1".to_string(),
            size: 100,
            modified_time: "2024-01-01T00:00:00.000".to_string(),
            is_dir: false,
        }];

        let diffs = compute_diff(&local, &remote);
        assert_eq!(diffs.len(), 1);
        assert!(matches!(
            diffs[0].kind,
            DiffKind::LocalNewer | DiffKind::RemoteNewer
        ));
    }

    #[test]
    fn test_compute_diff_skips_dirs() {
        let local = vec![LocalFileEntry {
            relative_path: "subdir".to_string(),
            size: 0,
            modified_epoch_ms: 0,
            is_dir: true,
        }];
        let remote: Vec<RemoteFileEntry> = vec![];

        let diffs = compute_diff(&local, &remote);
        assert_eq!(diffs.len(), 0);
    }

    #[test]
    fn test_scan_local_tree_with_excludes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        std::fs::write(root.join("file1.txt"), "hello").unwrap();
        std::fs::write(root.join(".hidden"), "secret").unwrap();
        std::fs::create_dir(root.join("subdir")).unwrap();
        std::fs::write(root.join("subdir/file2.txt"), "world").unwrap();
        std::fs::write(root.join("subdir/.gitignore"), "").unwrap();

        let entries = scan_local_tree(root, &[".*".to_string()]).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.relative_path.as_str()).collect();

        assert!(names.contains(&"file1.txt"));
        assert!(names.contains(&"subdir"));
        assert!(names.contains(&"subdir/file2.txt"));
        assert!(!names.contains(&".hidden"));
        assert!(!names.contains(&"subdir/.gitignore"));
    }

    #[test]
    fn test_parse_remote_time() {
        assert!(parse_remote_time_to_epoch_ms("2024-01-01T00:00:00.000") > 0);
        assert!(parse_remote_time_to_epoch_ms("2024-01-01T00:00:00+08:00") > 0);
        assert_eq!(parse_remote_time_to_epoch_ms(""), 0);
        assert_eq!(parse_remote_time_to_epoch_ms("invalid"), 0);
    }
}
