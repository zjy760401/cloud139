# AGENTS.md - cloud139 开发指南

本文档为 AI Agent 提供开发指导。

## 快速开始

### 构建与测试

```bash
# Debug 构建
cargo build

# 运行所有测试
cargo test
```

更多命令见 [docs/commands.md](docs/commands.md)

### 项目结构

```
src/
├── main.rs           # 程序入口
├── lib.rs           # 库入口
├── commands/        # CLI 命令实现
├── client/         # API 客户端
├── models/         # 数据模型
├── config/         # 配置管理
└── utils/          # 工具函数
```

详细结构见 [docs/structure.md](docs/structure.md)

### 代码风格

参考现有代码风格，见 [docs/style.md](docs/style.md)

### 常用依赖

clap, reqwest, tokio, serde, thiserror 等

详细依赖见 [docs/dependencies.md](docs/dependencies.md)

## 开发注意事项

- 139 云盘 API 区分三种存储类型: PersonalNew, Family, Group
- 某些操作在不同存储类型下行为不同

详细注意事项见 [docs/notes.md](docs/notes.md)

## 相关文档

- [README.md](../README.md): 项目说明
- [.agents/skills/cloud139-e2e-test](../.agents/skills/cloud139-e2e-test/SKILL.md): E2E 测试流程
