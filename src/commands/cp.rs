use clap::Parser;
use crate::client::{Client, ClientError, StorageType};
use crate::models::BatchCopyResp;

#[derive(Parser, Debug)]
pub struct CpArgs {
    #[arg(help = "源文件路径")]
    pub source: String,

    #[arg(help = "目标目录")]
    pub target: String,

    #[arg(short, long, help = "合并复制（覆盖目标中的同名文件）")]
    pub merge: bool,
}

pub async fn execute(args: CpArgs) -> Result<(), ClientError> {
    let config = crate::config::Config::load().map_err(|e| ClientError::Config(e))?;
    let storage_type = config.storage_type();

    match storage_type {
        StorageType::PersonalNew => {
            cp_personal(&config, &args.source, &args.target, args.merge).await?;
        }
        StorageType::Family => {
            cp_family(&config, &args.source, &args.target).await?;
        }
        StorageType::Group => {
            cp_group(&config, &args.source, &args.target).await?;
        }
    }

    Ok(())
}

async fn cp_personal(config: &crate::config::Config, source: &str, target: &str, merge: bool) -> Result<(), ClientError> {
    let source_id = crate::client::api::get_file_id_by_path(config, source).await?;
    if source_id.is_empty() {
        println!("错误: 无效的源文件路径");
        return Ok(());
    }

    let target_id = if target == "/" || target.is_empty() {
        "".to_string()
    } else {
        crate::client::api::get_file_id_by_path(config, target).await?
    };
    
    let mut config = config.clone();
    let host = crate::client::api::get_personal_cloud_host(&mut config).await?;
    let url = format!("{}/file/batchCopy", host);

    let body = serde_json::json!({
        "fileIds": [source_id],
        "toParentFileId": target_id,
        "merge": merge
    });

    let resp: BatchCopyResp = crate::client::api::personal_api_request(&config, &url, body, StorageType::PersonalNew).await?;

    if resp.base.success {
        if merge {
            println!("合并复制成功");
        } else {
            println!("复制成功");
        }
    } else {
        println!("复制失败: {}", resp.base.message);
    }

    Ok(())
}

async fn cp_family(config: &crate::config::Config, source: &str, target: &str) -> Result<(), ClientError> {
    let client = Client::new(config.clone());

    let body = serde_json::json!({
        "commonAccountInfo": {
            "accountType": "1",
            "accountUserId": config.user_domain_id.as_deref().unwrap_or("")
        },
        "destCatalogID": target,
        "destCloudID": config.cloud_id,
        "sourceCatalogIDs": [],
        "sourceCloudID": config.cloud_id,
        "sourceContentIDs": [source]
    });

    let resp: serde_json::Value = client.and_album_request("/copyContentCatalog", body).await?;

    println!("复制响应: {:?}", resp);
    Ok(())
}

async fn cp_group(config: &crate::config::Config, source: &str, target: &str) -> Result<(), ClientError> {
    let client = Client::new(config.clone());

    let source_file_id = crate::client::api::get_file_id_by_path(config, source).await?;
    let target_file_id = if target == "/" || target.is_empty() {
        "0".to_string()
    } else {
        crate::client::api::get_file_id_by_path(config, target).await?
    };

    let source_path = format!("root:/{}:{}", target, source_file_id);
    let dest_path = format!("root:/{}:{}", target, target_file_id);

    let body = serde_json::json!({
        "commonAccountInfo": {
            "accountType": "1",
            "accountUserId": config.user_domain_id.as_deref().unwrap_or("")
        },
        "destCatalogID": dest_path,
        "destCloudID": config.cloud_id,
        "sourceCatalogIDs": [source_path],
        "sourceCloudID": config.cloud_id,
        "sourceContentIDs": []
    });

    let resp: serde_json::Value = client.and_album_request("/copyContentCatalog", body).await?;

    if resp.get("result").and_then(|r| r.get("resultCode")).and_then(|c| c.as_str()) == Some("0") {
        println!("复制成功");
    } else {
        println!("复制失败: {:?}", resp);
    }

    Ok(())
}
