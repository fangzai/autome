# Autome 启动与运行指南

本项目分为两部分：
- **后端内核 (`automed`)**：Rust 实现，负责调度 AI 角色、Git 状态机管理、模型探测与会话协议分发。
- **前端桌面端 (`apps/desktop`)**：Electron + 原生 Web（Vanilla JS/CSS）实现，负责用户交互、路由图配置与仪表盘渲染。

---

## 一、环境准备 (Prerequisites)

在启动前，请确保本机已安装以下工具：

1. **Rust 与 Cargo**
   - 检查命令：`rustc --version`、`cargo --version`
2. **Node.js 与 npm**
   - 推荐 Node.js v20+
   - 检查命令：`node -v`、`npm -v`
3. **Git & 终端工具**
   - Windows 环境下请确保已安装 Git for Windows（可在终端执行 `git --version`）。
4. **AI CLI 运行时（建议至少安装并登录其中之一）**
   - **Antigravity CLI (`agy`)**：`agy --version`，测试模型命令：`agy models`
   - **Codex CLI (`codex`)**：`codex --version`，配置与登录：`codex login`
   - **Claude Code CLI (`claude`)**：`claude --version`

---

## 二、开发模式快速启动 (Quick Start)

### 步骤 1：编译 Rust 后端内核

在项目根目录（`d:\AgentCode\autome`）下执行编译：

```bash
cargo build -p automed
```

> **说明**：该命令会在 `target/debug/` 下生成内核可执行程序 `automed.exe`（macOS/Linux 为 `automed`）。Electron 桌面端在开发模式下会自动从此路径拉起 sidecar 守护进程。

### 步骤 2：安装桌面端依赖（首次运行时）

进入 `apps/desktop` 目录安装前端依赖：

```bash
cd apps/desktop
npm install
```

### 步骤 3：启动桌面应用

在 `apps/desktop` 目录下运行：

```bash
npm start
```

或者直接在项目根目录下通过一条命令启动：

```bash
npm start --prefix apps/desktop
```

启动后会弹出 Autome 桌面主窗口，自动连接编译好的 `automed` 内核。

---

## 三、常用测试与诊断命令

- **Rust 内核测试**（在项目根目录执行）：
  ```bash
  cargo test -p automed --lib
  ```
- **桌面端前端与端到端 DOM 测试**：
  ```bash
  cd apps/desktop
  npm test
  ```
- **手动指定内核二进制路径（可选）**：
  如果内核生成在自定义路径或发布目录，可通过 `AUTOMED_BIN` 环境变量指定：
  - Windows PowerShell:
    ```powershell
    $env:AUTOMED_BIN = "D:\AgentCode\autome\target\debug\automed.exe"
    npm start --prefix apps/desktop
    ```
  - Bash:
    ```bash
    AUTOMED_BIN="../../target/debug/automed" npm start --prefix apps/desktop
    ```

---

## 四、生产发布与安装包打包 (Packaging)

- **Windows 安装包打包**：
  在项目根目录下运行打包脚本：
  ```powershell
  powershell -ExecutionPolicy Bypass -File scripts/package.ps1
  ```
- **macOS 安装包打包**：
  ```bash
  sh scripts/package.sh
  ```
