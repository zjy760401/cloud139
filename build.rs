use std::process::Command;

fn main() {
    // 获取 git 提交数作为 patch 版本号
    let commit_count = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "0".to_string());

    println!("cargo:rustc-env=GIT_COMMIT_COUNT={}", commit_count);

    // 提交数变化时重新运行
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/");
}
