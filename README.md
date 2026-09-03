# 有赞订单同步

基于 React 18 + TypeScript + Vite 6、Rust 2021、SQLite 和 Tauri 2 的 Windows 桌面应用。

## 本地开发

```bash
npm install
npm run tauri dev
```

不要直接用 RustRover 的 `cargo run` 启动；它不会自动启动 Vite，因此窗口会显示空白。如果必须分别启动，请先在一个 Terminal 运行 `npm run dev`，再运行 `cargo run --manifest-path src-tauri/Cargo.toml`。

RustRover 已提供共享运行配置：右上角运行配置选择 `Tauri Dev`，点击绿色运行按钮即可启动。该配置执行 `npm run tauri dev`，会自动启动 Vite 和 Tauri。

应用已内置有赞 `client_id`、`client_secret` 和店铺（`grant_id`）。首次使用可在“店铺与授权”中粘贴 `access_token`，或直接点击“刷新 Token”；应用通过 `youzan.trades.sold.get/4.0.4` 查询订单，并将结果保存到本机 SQLite。

## GitHub Actions

推送 `v*` 标签或手动运行 `Build Windows` 工作流即可构建 Windows 安装包。首次使用请在 GitHub 仓库启用 Actions；工作流使用 `GITHUB_TOKEN` 自动创建草稿 Release。

应用密钥已按需求编码在 Rust 程序中。发布前请注意：桌面程序中的密钥可被逆向提取，建议仅用于受控的自用环境。
