use clap::Parser;
use crate::client::ClientError;

#[derive(Parser, Debug)]
pub struct LoginArgs {
    #[arg(short, long, required = true, help = "Authorization Token (从alist或手动获取)")]
    pub token: String,

    #[arg(short, long, default_value = "personal_new", help = "存储类型: personal_new, family, group")]
    pub storage_type: String,

    #[arg(short, long, help = "云盘ID (家庭云/和家亲时需要)")]
    pub cloud_id: Option<String>,
}

pub async fn execute(args: LoginArgs) -> Result<(), ClientError> {
    println!("正在验证 Token ...");

    let config = crate::client::auth::login(
        &args.token,
        &args.storage_type,
        args.cloud_id.as_deref(),
    ).await?;

    config.save()?;

    println!("Token 验证成功!");
    println!("存储类型: {}", args.storage_type);
    println!("配置文件已保存到: ./config/config.json");

    Ok(())
}
