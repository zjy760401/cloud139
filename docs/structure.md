# 项目结构

```
src/
├── main.rs           # 程序入口
├── lib.rs           # 库入口
├── commands/        # CLI 命令实现 (cp, delete, download, list, login, mkdir, mv, rename, upload)
├── client/         # API 客户端 (Client, ClientError, StorageType, api, api_trait, auth)
├── models/         # 数据模型
├── config/         # 配置管理
└── utils/          # 工具函数 (crypto, width, logger)
```
