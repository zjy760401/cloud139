use crate::client::ClientError;
use crate::config::Config;
use crate::models::QueryRoutePolicyResp;

pub async fn get_personal_cloud_host(config: &mut Config) -> Result<String, ClientError> {
    if let Some(ref host) = config.personal_cloud_host {
        return Ok(host.clone());
    }

    let url = "https://user-njs.yun.139.com/user/route/qryRoutePolicy";

    let body = serde_json::json!({
        "userInfo": {
            "userType": 1,
            "accountType": 1,
            "accountName": config.username
        },
        "modAddrType": 1
    });

    let client = reqwest::Client::new();

    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let rand_str = generate_rand_str(16);
    let body_str = body.to_string();
    let sign = crate::utils::crypto::calc_sign(&body_str, &ts, &rand_str);

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Accept", "application/json, text/plain, */*".parse().unwrap());
    headers.insert("Authorization", format!("Basic {}", config.authorization).parse().unwrap());
    headers.insert("Content-Type", "application/json;charset=UTF-8".parse().unwrap());
    headers.insert("mcloud-channel", "1000101".parse().unwrap());
    headers.insert("mcloud-client", "10701".parse().unwrap());
    headers.insert("mcloud-sign", format!("{},{},{}", ts, rand_str, sign).parse().unwrap());
    headers.insert("mcloud-version", "7.14.0".parse().unwrap());
    headers.insert("Origin", "https://yun.139.com".parse().unwrap());
    headers.insert("Referer", "https://yun.139.com/w/".parse().unwrap());
    headers.insert("x-DeviceInfo", "||9|7.14.0|chrome|120.0.0.0|||windows 10||zh-CN|||".parse().unwrap());
    headers.insert("x-huawei-channelSrc", "10000034".parse().unwrap());
    headers.insert("x-inner-ntwk", "2".parse().unwrap());
    headers.insert("x-m4c-caller", "PC".parse().unwrap());
    headers.insert("x-m4c-src", "10002".parse().unwrap());
    headers.insert("x-SvcType", "1".parse().unwrap());
    headers.insert("Inner-Hcy-Router-Https", "1".parse().unwrap());

    let resp = client
        .post(url)
        .headers(headers)
        .json(&body)
        .send()
        .await?;

    let route_resp: QueryRoutePolicyResp = resp.json().await?;

    let host = route_resp.data.route_policy_list
        .into_iter()
        .find(|p| p.mod_name == "personal")
        .map(|p| p.https_url)
        .ok_or_else(|| ClientError::Other("Could not find personal cloud host".to_string()))?;

    config.personal_cloud_host = Some(host.clone());
    let _ = config.save();

    Ok(host)
}

pub async fn get_personal_disk_info(config: &Config) -> Result<crate::models::PersonalDiskInfoResp, ClientError> {
    let url = "https://user-njs.yun.139.com/user/disk/getPersonalDiskInfo";

    let body = serde_json::json!({
        "userDomainId": config.user_domain_id
    });

    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let rand_str = generate_rand_str(16);
    let body_str = body.to_string();
    let sign = crate::utils::crypto::calc_sign(&body_str, &ts, &rand_str);

    let client = reqwest::Client::new();

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Accept", "application/json, text/plain, */*".parse().unwrap());
    headers.insert("Authorization", format!("Basic {}", config.authorization).parse().unwrap());
    headers.insert("Content-Type", "application/json;charset=UTF-8".parse().unwrap());
    headers.insert("mcloud-channel", "1000101".parse().unwrap());
    headers.insert("mcloud-client", "10701".parse().unwrap());
    headers.insert("mcloud-sign", format!("{},{},{}", ts, rand_str, sign).parse().unwrap());
    headers.insert("mcloud-version", "7.14.0".parse().unwrap());
    headers.insert("Origin", "https://yun.139.com".parse().unwrap());
    headers.insert("Referer", "https://yun.139.com/w/".parse().unwrap());
    headers.insert("x-DeviceInfo", "||9|7.14.0|chrome|120.0.0.0|||windows 10||zh-CN|||".parse().unwrap());
    headers.insert("x-huawei-channelSrc", "10000034".parse().unwrap());
    headers.insert("x-inner-ntwk", "2".parse().unwrap());
    headers.insert("x-m4c-caller", "PC".parse().unwrap());
    headers.insert("x-m4c-src", "10002".parse().unwrap());
    headers.insert("x-SvcType", "1".parse().unwrap());
    headers.insert("Inner-Hcy-Router-Https", "1".parse().unwrap());

    let resp = client
        .post(url)
        .headers(headers)
        .json(&body)
        .send()
        .await?;

    let result: crate::models::PersonalDiskInfoResp = resp.json().await?;
    Ok(result)
}

pub async fn get_family_disk_info(config: &Config) -> Result<crate::models::FamilyDiskInfoResp, ClientError> {
    let url = "https://user-njs.yun.139.com/user/disk/getFamilyDiskInfo";

    let body = serde_json::json!({
        "userDomainId": config.user_domain_id
    });

    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let rand_str = generate_rand_str(16);
    let body_str = body.to_string();
    let sign = crate::utils::crypto::calc_sign(&body_str, &ts, &rand_str);

    let client = reqwest::Client::new();

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Accept", "application/json, text/plain, */*".parse().unwrap());
    headers.insert("Authorization", format!("Basic {}", config.authorization).parse().unwrap());
    headers.insert("Content-Type", "application/json;charset=UTF-8".parse().unwrap());
    headers.insert("mcloud-channel", "1000101".parse().unwrap());
    headers.insert("mcloud-client", "10701".parse().unwrap());
    headers.insert("mcloud-sign", format!("{},{},{}", ts, rand_str, sign).parse().unwrap());
    headers.insert("mcloud-version", "7.14.0".parse().unwrap());
    headers.insert("Origin", "https://yun.139.com".parse().unwrap());
    headers.insert("Referer", "https://yun.139.com/w/".parse().unwrap());
    headers.insert("x-DeviceInfo", "||9|7.14.0|chrome|120.0.0.0|||windows 10||zh-CN|||".parse().unwrap());
    headers.insert("x-huawei-channelSrc", "10000034".parse().unwrap());
    headers.insert("x-inner-ntwk", "2".parse().unwrap());
    headers.insert("x-m4c-caller", "PC".parse().unwrap());
    headers.insert("x-m4c-src", "10002".parse().unwrap());
    headers.insert("x-SvcType", "2".parse().unwrap());
    headers.insert("Inner-Hcy-Router-Https", "1".parse().unwrap());

    let resp = client
        .post(url)
        .headers(headers)
        .json(&body)
        .send()
        .await?;

    let result: crate::models::FamilyDiskInfoResp = resp.json().await?;
    Ok(result)
}

fn generate_rand_str(len: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..len).map(|_| {
        let idx = rng.gen_range(0..CHARSET.len());
        CHARSET[idx] as char
    }).collect()
}

pub async fn personal_api_request<T: for<'de> serde::Deserialize<'de>>(
    config: &Config,
    url: &str,
    body: serde_json::Value,
) -> Result<T, ClientError> {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let rand_str = generate_rand_str(16);
    let body_str = body.to_string();
    let sign = crate::utils::crypto::calc_sign(&body_str, &ts, &rand_str);

    let client = reqwest::Client::new();

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Accept", "application/json, text/plain, */*".parse().unwrap());
    headers.insert("Authorization", format!("Basic {}", config.authorization).parse().unwrap());
    headers.insert("Caller", "web".parse().unwrap());
    headers.insert("Cms-Device", "default".parse().unwrap());
    headers.insert("Mcloud-Channel", "1000101".parse().unwrap());
    headers.insert("Mcloud-Client", "10701".parse().unwrap());
    headers.insert("Mcloud-Route", "001".parse().unwrap());
    headers.insert("Mcloud-Sign", format!("{},{},{}", ts, rand_str, sign).parse().unwrap());
    headers.insert("Mcloud-Version", "7.14.0".parse().unwrap());
    headers.insert("x-DeviceInfo", "||9|7.14.0|chrome|120.0.0.0|||windows 10||zh-CN|||".parse().unwrap());
    headers.insert("x-huawei-channelSrc", "10000034".parse().unwrap());
    headers.insert("x-inner-ntwk", "2".parse().unwrap());
    headers.insert("x-m4c-caller", "PC".parse().unwrap());
    headers.insert("x-m4c-src", "10002".parse().unwrap());
    headers.insert("x-SvcType", "1".parse().unwrap());
    headers.insert("X-Yun-Api-Version", "v1".parse().unwrap());
    headers.insert("X-Yun-App-Channel", "10000034".parse().unwrap());
    headers.insert("X-Yun-Channel-Source", "10000034".parse().unwrap());
    headers.insert("X-Yun-Client-Info", "||9|7.14.0|chrome|120.0.0.0|||windows 10||zh-CN|||dW5kZWZpbmVk||".parse().unwrap());
    headers.insert("X-Yun-Module-Type", "100".parse().unwrap());
    headers.insert("X-Yun-Svc-Type", "1".parse().unwrap());

    let resp = client
        .post(url)
        .headers(headers)
        .json(&body)
        .send()
        .await?;

    let result: T = resp.json().await?;
    Ok(result)
}
