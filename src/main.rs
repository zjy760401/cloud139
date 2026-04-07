use clap::{Parser, Subcommand};

use cloud139::commands::{cp, delete, download, list, login, mkdir, mv, rename, sync, upload};
use cloud139::utils::logger;

#[derive(Parser)]
#[command(name = "cloud139")]
#[command(about = "139 Yun CLI - 移动云盘命令行工具", long_about = None)]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(
        short,
        long,
        default_value = "info",
        help = "日志级别 (trace, debug, info, warn, error, off)"
    )]
    verbose: String,
}

#[derive(Subcommand)]
enum Commands {
    /// 登录账号
    Login(login::LoginArgs),
    /// 列出文件
    Ls(list::ListArgs),
    /// 上传文件
    Upload(upload::UploadArgs),
    /// 下载文件
    Download(download::DownloadArgs),
    /// 删除文件
    Rm(delete::DeleteArgs),
    /// 创建目录
    Mkdir(mkdir::MkdirArgs),
    /// 移动文件
    Mv(mv::MvArgs),
    /// 复制文件
    Cp(cp::CpArgs),
    /// 重命名文件
    Rename(rename::RenameArgs),
    /// 同步文件夹
    Sync(sync::SyncArgs),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if cli.verbose.eq_ignore_ascii_case("off") {
        logger::set_quiet(true);
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&cli.verbose))
        .init();

    let result = match cli.command {
        Commands::Login(args) => login::execute(args).await,
        Commands::Ls(args) => list::execute(args).await,
        Commands::Upload(args) => upload::execute(args).await,
        Commands::Download(args) => download::execute(args).await,
        Commands::Rm(args) => delete::execute(args).await,
        Commands::Mkdir(args) => mkdir::execute(args).await,
        Commands::Mv(args) => mv::execute(args).await,
        Commands::Cp(args) => cp::execute(args).await,
        Commands::Rename(args) => rename::execute(args).await,
        Commands::Sync(args) => sync::execute(args).await,
    };

    if let Err(e) = result {
        if !logger::is_quiet() {
            eprintln!("\x1b[31merror\x1b[0m {}", e);
        }
        std::process::exit(1);
    }

    Ok(())
}
