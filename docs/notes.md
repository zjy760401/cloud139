# 开发注意事项

## 强制要求

- 所有公共函数必须添加文档注释
- 错误类型需要实现 `Error` trait
- async 函数使用 `#[tokio::main]`
- 配置文件使用 toml 格式

## 注意事项

- 139 云盘 API 区分三种存储类型: PersonalNew, Family, Group
- 某些操作在不同存储类型下行为不同（如重命名、批量移动）
- 登录 Token 需要从浏览器开发者工具获取
- 测试可能需要真实 API 调用或 mock
