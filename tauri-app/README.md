# LiteLLM Suite — Tauri 桌面应用

## 概述

基于 Tauri 2.0 的原生桌面管理应用，提供：

- **系统托盘**：后台运行，快速查看服务状态
- **仪表盘**：Docker 服务状态、Docker/Ollama 环境检查、快捷操作
- **智能对话**：复杂任务自动分解、SSE 实时推送、成本可视化
- **服务管理**：启动/停止/重启 Docker 服务
- **配置向导**：环境检查、CLI 命令调用
- **模型管理**：查看模型列表、编排复杂任务

## 前置要求

| 依赖 | 版本 | 说明 |
|------|------|------|
| Node.js | v18+ | 前端构建 |
| Rust | stable | Tauri 后端编译 |
| Docker | latest | 服务运行 |
| Python 3 | 3.10+ | CLI 工具 |

## 快速开始

```bash
# 一键构建
./build-tauri.sh

# 开发模式（热重载）
./build-tauri.sh --dev
```

## 项目结构

```
tauri-app/
├── package.json              # Node.js 依赖
├── build-tauri.sh            # 构建脚本
├── README.md                 # 本文件
└── src-tauri/
    ├── Cargo.toml            # Rust 依赖
    ├── tauri.conf.json       # Tauri 配置（窗口/图标/打包）
    ├── build.rs              # Tauri 构建脚本
    ├── ui/
    │   └── index.html        # 嵌入式前端界面
    └── src/
        └── main.rs           # Rust 后端（Docker 管理 + 系统托盘）
```

## Tauri Commands（后端 API）

| 命令 | 功能 |
|------|------|
| `get_services_status` | 获取所有 Docker 服务状态 |
| `start_services` | 启动所有 Docker 服务 |
| `stop_services` | 停止所有 Docker 服务 |
| `restart_service` | 重启指定服务 |
| `open_web_interface` | 在浏览器中打开 Web 界面 |
| `run_cli` | 运行 litellm_cli.py 命令 |
| `check_docker` | 检查 Docker 是否安装 |
| `check_ollama` | 检查 Ollama 是否运行 |

## 构建产物

- **macOS**: `.app` (应用包) + `.dmg` (安装包)
- **Windows**: `.exe` (安装包)
- **Linux**: `.deb` / `.AppImage`

## 打包资源配置

Tauri 打包时会将以下资源文件嵌入应用：

- docker-compose.yml + 所有配置文件
- litellm_cli.py + task_orchestrator.py + custom_callbacks.py
- webapp/ 目录
- grafana/ 目录
- install.sh + webui-init.sh + quota_setup.sh

用户安装桌面应用后，无需单独下载配置文件，应用会自动管理 Docker 服务生命周期。
