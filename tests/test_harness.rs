/// 测试生命周期：备份和还原登录配置
///
/// - 测试进程启动时：备份 config.toml → config.toml.test_bak
/// - 测试进程退出时：还原 config.toml.test_bak → config.toml
use ctor::{ctor, dtor};

#[ctor]
fn backup_config() {
    let config_path = dirs_config_path();
    let backup_path = format!("{}.test_bak", config_path);

    if std::path::Path::new(&config_path).exists() {
        if let Err(e) = std::fs::copy(&config_path, &backup_path) {
            eprintln!("[test-harness] 备份配置失败: {}", e);
        } else {
            eprintln!("[test-harness] 已备份配置: {} → {}", config_path, backup_path);
        }
    } else {
        eprintln!("[test-harness] 配置文件不存在，跳过备份");
    }
}

#[dtor]
fn restore_config() {
    let config_path = dirs_config_path();
    let backup_path = format!("{}.test_bak", config_path);

    if std::path::Path::new(&backup_path).exists() {
        if let Err(e) = std::fs::copy(&backup_path, &config_path) {
            eprintln!("[test-harness] 还原配置失败: {}", e);
        } else {
            let _ = std::fs::remove_file(&backup_path);
            eprintln!("[test-harness] 已还原配置: {} → {}", backup_path, config_path);
        }
    }
}

fn dirs_config_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    format!("{}/.config/cloud139/config.toml", home)
}

/// 占位测试，确保该文件被 cargo test 加载
#[test]
fn test_harness_loaded() {
    assert!(true);
}
