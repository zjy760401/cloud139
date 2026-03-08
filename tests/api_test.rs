use cloud139::models::*;

#[test]
fn test_query_file_resp_with_defaults() {
    let json = r#"{"success": true, "data": {"fileId": "123", "name": "test", "type": "file"}}"#;
    let resp: QueryFileResp = serde_json::from_str(json).unwrap();
    assert!(resp.base.success);
}

#[test]
fn test_personal_file_item_with_all_fields() {
    let json = r#"{
        "fileId": "123",
        "name": "test.txt",
        "size": 1024,
        "type": "file",
        "createdAt": "2024-01-01T00:00:00Z",
        "updatedAt": "2024-01-02T00:00:00Z",
        "createDate": "2024-01-01",
        "updateDate": "2024-01-02",
        "lastModified": "2024-01-03"
    }"#;
    let item: PersonalFileItem = serde_json::from_str(json).unwrap();
    assert_eq!(item.file_id, Some("123".to_string()));
    assert_eq!(item.name, Some("test.txt".to_string()));
    assert_eq!(item.size, Some(1024));
    assert_eq!(item.file_type, Some("file".to_string()));
}

#[test]
fn test_personal_upload_data_defaults() {
    let json = r#"{}"#;
    let data: PersonalUploadData = serde_json::from_str(json).unwrap();
    assert_eq!(data.file_id, None);
    assert_eq!(data.file_name, None);
    assert!(data.part_infos.is_none());
    assert_eq!(data.exist, None);
}

#[test]
fn test_personal_part_info_deserialize() {
    let json = r#"{
        "partNumber": 1,
        "uploadUrl": "http://example.com/upload"
    }"#;
    let part: PersonalPartInfo = serde_json::from_str(json).unwrap();
    assert_eq!(part.part_number, 1);
    assert_eq!(part.upload_url, "http://example.com/upload");
}

#[test]
fn test_download_url_data_defaults() {
    let json = r#"{}"#;
    let data: DownloadUrlData = serde_json::from_str(json).unwrap();
    assert_eq!(data.url, None);
    assert_eq!(data.cdn_url, None);
    assert_eq!(data.file_name, None);
}

#[test]
fn test_query_content_list_data_defaults() {
    let json = r#"{
        "result": {"resultCode": "0"},
        "path": "/",
        "cloudContentList": [],
        "cloudCatalogList": [],
        "totalCount": 0
    }"#;
    let data: QueryContentListData = serde_json::from_str(json).unwrap();
    assert_eq!(data.path, "/");
    assert_eq!(data.total_count, 0);
}

#[test]
fn test_get_group_content_result_defaults() {
    let json = r#"{
        "parentCatalogID": "0",
        "catalogList": [],
        "contentList": [],
        "nodeCount": 0,
        "ctlgCnt": 0,
        "contCnt": 0
    }"#;
    let result: GetGroupContentResult = serde_json::from_str(json).unwrap();
    assert_eq!(result.parent_catalog_id, "0");
    assert_eq!(result.node_count, 0);
}

#[test]
fn test_group_content_with_digest() {
    let json = r#"{
        "contentID": "123",
        "contentName": "test.txt",
        "contentSize": 1024,
        "createTime": "2024-01-01",
        "updateTime": "2024-01-02",
        "thumbnailURL": "http://example.com/thumb.jpg",
        "digest": "abc123"
    }"#;
    let content: GroupContent = serde_json::from_str(json).unwrap();
    assert_eq!(content.digest, Some("abc123".to_string()));
}

#[test]
fn test_route_policy_full_deserialize() {
    let json = r#"{
        "siteID": "site1",
        "siteCode": "code1",
        "modName": "personal",
        "httpUrl": "http://example.com",
        "httpsUrl": "https://example.com"
    }"#;
    let policy: RoutePolicy = serde_json::from_str(json).unwrap();
    assert_eq!(policy.site_id, Some("site1".to_string()));
    assert_eq!(policy.mod_name, Some("personal".to_string()));
    assert_eq!(policy.https_url, Some("https://example.com".to_string()));
}

#[test]
fn test_refresh_token_resp_with_desc() {
    let xml = r#"<root><return>1</return><token></token><expiretime>3600</expiretime><desc>error</desc></root>"#;
    let resp: RefreshTokenResp = serde_xml_rs::from_str(xml).unwrap();
    assert_eq!(resp.return_code, Some("1".to_string()));
    assert_eq!(resp.expiretime, Some(3600));
    assert_eq!(resp.desc, Some("error".to_string()));
}

#[test]
fn test_personal_list_data_without_next_cursor() {
    let json = r#"{
        "success": true,
        "data": {
            "items": []
        }
    }"#;
    let resp: PersonalListResp = serde_json::from_str(json).unwrap();
    let data = resp.data.unwrap();
    assert_eq!(data.next_page_cursor, None);
}

#[test]
fn test_batch_responses_with_error() {
    let json = r#"{"success": false, "code": "500", "message": "error"}"#;
    let resp: BatchMoveResp = serde_json::from_str(json).unwrap();
    assert!(!resp.base.success);
    assert_eq!(resp.base.code, Some("500".to_string()));
}

#[test]
fn test_common_account_info_default() {
    let info = CommonAccountInfo {
        account: "".to_string(),
        account_type: 0,
    };
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"account\":\"\""));
    assert!(json.contains("\"accountType\":0"));
}

#[test]
fn test_list_request_with_none_options() {
    let req = ListRequest {
        parent_file_id: "parent123".to_string(),
        page_num: 1,
        page_size: 100,
        order_by: None,
        descending: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"parentFileId\":\"parent123\""));
}

#[test]
fn test_page_info_serialize() {
    let info = PageInfo {
        page_num: 1,
        page_size: 50,
    };
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"pageNum\":1"));
    assert!(json.contains("\"pageSize\":50"));
}

#[test]
fn test_family_create_folder_request_serialize() {
    let req = FamilyCreateFolderRequest {
        catalog_name: "new_folder".to_string(),
        parent_catalog_id: "parent123".to_string(),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"catalogName\":\"new_folder\""));
    assert!(json.contains("\"parentCatalogID\":\"parent123\""));
}

#[test]
fn test_cloud_content_with_optional_fields() {
    let json = r#"{
        "contentID": "123",
        "contentName": "test.txt",
        "contentSize": 1024,
        "createTime": "2024-01-01",
        "lastUpdateTime": "2024-01-02"
    }"#;
    let content: CloudContent = serde_json::from_str(json).unwrap();
    assert_eq!(content.thumbnail_url, None);
}

#[test]
fn test_cloud_catalog_with_all_fields() {
    let json = r#"{
        "catalogID": "123",
        "catalogName": "folder",
        "createTime": "2024-01-01",
        "lastUpdateTime": "2024-01-02"
    }"#;
    let catalog: CloudCatalog = serde_json::from_str(json).unwrap();
    assert_eq!(catalog.catalog_id, "123");
    assert_eq!(catalog.catalog_name, "folder");
}

#[test]
fn test_group_catalog_defaults() {
    let json = r#"{
        "catalogID": "123",
        "catalogName": "folder",
        "createTime": "2024-01-01",
        "updateTime": "2024-01-02",
        "path": "/folder"
    }"#;
    let catalog: GroupCatalog = serde_json::from_str(json).unwrap();
    assert_eq!(catalog.path, "/folder");
}

#[test]
fn test_api_result_defaults() {
    let json = r#"{"resultCode": "0"}"#;
    let result: ApiResult = serde_json::from_str(json).unwrap();
    assert_eq!(result.result_desc, None);
}

#[test]
fn test_part_info_defaults() {
    let json = r#"{"partNumber": 1, "partSize": 1024, "parallelHashCtx": {"partOffset": 0}}"#;
    let part: PartInfo = serde_json::from_str(json).unwrap();
    assert_eq!(part.parallel_hash_ctx.part_offset, 0);
}
