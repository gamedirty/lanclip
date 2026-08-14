# LanClip · 局域网剪切板收件箱

一个**无账号、无服务器、端到端加密**的局域网剪切板同步应用（Windows / macOS）。

在任意一台已配对的电脑上复制内容，局域网内其他电脑会在**右上角弹出通知**；
点击「接收」把内容复制进本机剪切板，点击「忽略」或超时则保留在**历史记录**中，
随时可以重新接收。

## 核心特性

- **常驻后台**：系统托盘（Windows）/ 菜单栏（macOS），关闭窗口即隐藏到托盘，支持开机自启
- **局域网自动发现**：mDNS/DNS-SD（`_lanclip._udp.local.`），无需输入 IP
- **端到端加密**：QUIC（TLS 1.3）传输 + 证书公钥固定，未配对设备拿不到任何剪切板数据
- **设备配对**：6 位数字验证码（SAS），肉眼比对防中间人
- **确认接收**：接收方点击后才写入本机剪切板，不会被远程覆盖
- **历史记录**：内容本地加密存储（ChaCha20-Poly1305），支持搜索 / 状态筛选 / 重新接收
- **防死循环**：BLAKE3 内容哈希 + 写入抑制窗口 + 消息去重，接收的内容不会被再次广播
- **首版支持**：纯文本 / URL / HTML（同时保留纯文本版本）；图片、文件属于二期

## 技术栈

| 模块 | 技术 |
|---|---|
| 桌面框架 | Tauri 2（多窗口 + 托盘） |
| 后端核心 | Rust + Tokio |
| UI | React + TypeScript + Vite + Tailwind CSS |
| 局域网发现 | mDNS（mdns-sd） |
| 设备通信 | QUIC（quinn + rustls，自签名证书 + 公钥固定） |
| 设备身份 | Ed25519（挑战-应答认证） |
| 内容哈希 | BLAKE3 |
| 本地库 | SQLite（rusqlite） |
| 密钥存储 | Windows Credential Manager / macOS Keychain（keyring） |
| 日志 | tracing（stderr + 按天滚动文件） |

## 快速开始

```bash
# 前置：Node.js 18+、Rust 1.75+（Windows 需 MSVC 工具链 & WebView2）
npm install
npm run tauri dev      # 开发模式
npm run tauri build    # 打包（Windows: NSIS 安装包；macOS: DMG）
```

首次启动会自动生成设备身份（Ed25519 + TLS 证书）并创建本地数据库，
数据目录：`%APPDATA%/app.lanclip.desktop`（Windows）或 `~/Library/Application Support/app.lanclip.desktop`（macOS）。

## 两台设备如何配对

1. 两台电脑都启动 LanClip，处于同一局域网
2. 在任一台的「设备」页，会看到对方出现在「发现的设备」
3. 点击「请求配对」→ **两台设备同时显示 6 位验证码**
4. 肉眼核对两码一致后，被请求方点击「确认配对」
5. 之后任意一方复制内容，另一方右上角弹通知

## 工作流程

```
电脑 A 复制内容
  → 剪切板监听（Windows: WM_CLIPBOARDUPDATE 事件；macOS: 轮询）
  → BLAKE3 哈希去重（防回环、防重复）
  → QUIC 加密发送给所有已配对在线设备
电脑 B 收到
  → 内容加密落库（状态: 待处理）
  → 右上角弹出自定义通知窗口（8 秒自动收起）
     ├─ 点击「接收」→ 写入本机剪切板（状态: 已接收）
     ├─ 点击「忽略」→ 状态: 已忽略
     └─ 超时收起   → 状态保持: 待处理
  → 三种状态都可从历史记录重新接收
```

## 安全设计

- mDNS 只广播设备 ID / 名称 / 端口 / 协议版本，**不含任何剪切板内容**
- 内容只通过已建立的 QUIC（TLS 1.3）连接发送给**已配对**设备
- 客户端固定配对时保存的证书（DER 精确匹配），换证书 = 需要重新配对
- 服务端通过 Ed25519 挑战-应答认证连接方身份
- 历史内容用本机密钥（存系统凭据库）加密后落库
- 通知预览可关闭（敏感环境只显示来源设备）

## 本机自测

```bash
npm run tauri -- dev -- --selftest   # 或直接运行编译产物
./src-tauri/target/debug/lanclip.exe --selftest
```

自测覆盖：密钥生成、本地加解密、配对码算法、Ed25519 签名、mDNS 注册、
QUIC 回环连接（证书固定 + 挑战应答）、剪切板消息收发、历史落库与解密还原。

## 与设计稿的已知差异（v0.1）

- macOS 剪切板监听采用 500ms 轮询（哈希比较），后续可切换原生 `NSPasteboard.changeCount`
- 自动更新（tauri-plugin-updater）未启用，需要签名密钥后再开
- 图片 / 文件传输、多显示器通知归位、iOS 端均为二期
- macOS 的 `.icns` 图标与 DMG 签名需在 macOS 构建机上补齐（`icons/` 目前含 ico/png）

## 项目结构

```
lanclip/
├── index.html / popup.html       # 主窗口 / 接收弹窗入口
├── src/                          # React 前端
│   ├── pages/                    # History / Devices / Settings
│   ├── popup/                    # 右上角接收弹窗
│   ├── stores/                   # zustand 全局状态
│   └── lib/                      # 类型定义 + IPC 封装
└── src-tauri/
    ├── src/
    │   ├── clipboard/            # windows.rs 事件监听 / macos.rs 轮询 / 共享逻辑
    │   ├── network/              # discovery.rs (mDNS) / transport.rs (QUIC) / protocol.rs (MessagePack)
    │   ├── security/             # identity / pairing / key_store / cipher
    │   ├── storage.rs            # SQLite 设备 + 历史 + kv
    │   ├── commands.rs           # Tauri IPC 命令
    │   ├── notification.rs       # 弹窗定位与系统通知
    │   ├── tray.rs               # 托盘菜单
    │   └── selftest.rs           # --selftest 回环自测
    └── tauri.conf.json
```
