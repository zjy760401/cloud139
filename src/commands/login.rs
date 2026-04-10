use crate::client::ClientError;
use crate::{info, success};
use clap::Parser;

#[derive(Parser, Debug)]
pub struct LoginArgs {
    #[arg(
        short,
        long,
        help = "Authorization Token (从浏览器开发者工具获取)"
    )]
    pub token: Option<String>,

    #[arg(
        short,
        long,
        default_value = "personal_new",
        help = "存储类型: personal_new, family, group"
    )]
    pub storage_type: String,

    #[arg(short, long, help = "云盘ID (家庭云/和家亲时需要)")]
    pub cloud_id: Option<String>,
}

pub async fn execute(args: LoginArgs) -> Result<(), ClientError> {
    // 未提供 token 时，显示当前登录信息或交互输入
    if args.token.is_none() {
        match crate::config::Config::load() {
            Ok(config) => {
                success!("当前已登录");
                info!("账号: {}", config.account);
                info!("存储类型: {}", if config.storage_type.is_empty() { "personal_new" } else { &config.storage_type });
                if let Some(ref cid) = config.cloud_id {
                    info!("云盘ID: {}", cid);
                }
                let expired = config.is_token_expired();
                if expired {
                    info!("Token 状态: \x1b[33m已过期\x1b[0m");
                } else {
                    info!("Token 状态: \x1b[32m有效\x1b[0m");
                }
                if let Ok(path) = crate::config::Config::config_path() {
                    info!("配置文件: {}", path.display());
                }
                return Ok(());
            }
            Err(crate::config::ConfigError::NotFound) => {
                info!("未检测到登录信息，请输入 Authorization Token");
                info!("（从浏览器开发者工具 → Network → 任意请求 → Headers → Authorization 获取）");
                let input: String = dialoguer::Input::new()
                    .with_prompt("Token")
                    .interact_text()
                    .map_err(|e| ClientError::Other(format!("输入错误: {}", e)))?;
                return do_login(input, &args.storage_type, args.cloud_id.as_deref()).await;
            }
            Err(e) => return Err(ClientError::Config(e)),
        }
    }

    do_login(args.token.unwrap(), &args.storage_type, args.cloud_id.as_deref()).await
}

async fn do_login(token: String, storage_type: &str, cloud_id: Option<&str>) -> Result<(), ClientError> {
    info!("正在验证 Token ...");

    let token = token
        .strip_prefix("Basic ")
        .map(|s| s.to_string())
        .unwrap_or(token);

    let config =
        crate::client::auth::login(&token, storage_type, cloud_id).await?;

    config.save()?;

    success!("Token 验证成功!");
    info!("账号: {}", config.account);
    info!("存储类型: {}", storage_type);
    if let Ok(path) = crate::config::Config::config_path() {
        success!("配置文件已保存到: {}", path.display());
    }

    Ok(())
}
