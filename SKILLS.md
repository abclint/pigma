# Pigma – 网易云音乐客户端（CLI 控制）

## Overview
**Pigma** 是一个网易云音乐 TUI 客户端（ratatui）。不带子命令直接运行即进入交互界面；同时提供
`pigma status` / `pigma list` / `pigma msg` 三个子命令，通过 IPC（Linux/macOS 为 Unix socket
`~/.cache/pigma/pigma.sock`，Windows 为命名管道 `\\.\pipe\pigma`）查询或控制**正在运行**的实例
（交互界面或 `-d` 守护进程均可）。适合脚本、状态栏（Waybar）、远程控制。

---

## 安装与启动

```bash
cargo build --release
# 将 target/release/pigma 加入 PATH
```

- **无子命令** → 启动交互式 TUI。
- **守护进程**（无头后台）：
  ```bash
  pigma -d                # 等价于 pigma -d __liked__
  pigma -d toplist        # 加载指定端点
  pigma -d toplist --playlist 3   # 歌单/榜单端点选第 3 个（1 起始）
  ```
  守护进程只加载队列，**不自动播放**，需显式 `pigma msg play`。
  `SIGINT`/`SIGTERM` 会保存会话并干净退出。

---

## 全局选项（`pigma --help`）

| 选项 | 说明 |
|---|---|
| `-v, --version` | 打印版本号并退出 |
| `-d, --daemon [<ENDPOINT>]` | 无头守护进程模式；省略 ENDPOINT 默认 `__liked__`（可与子命令冲突，见下） |
| `--playlist <INDEX>` | 歌单/榜单端点选第 N 个歌单（1 起始），配合 `-d` 使用 |
| `--socket <SOCKET>` | 自定义 IPC socket/管道路径（Unix 或 Windows 管道名） |
| `-h, --help` | 显示帮助 |

> `args_conflicts_with_subcommands = true`：`-d`/`--playlist` 等全局选项不能与子命令混用；
> 但 `status`/`msg` 子命令内部自带 `--socket`，可指定目标实例。

---

## `pigma status` – 查询状态

```bash
pigma status [OPTIONS]
```

| 选项 | 说明 |
|---|---|
| `--template <TEMPLATE>` | 自定义 plain 输出。占位符：`{name}` `{artist}` `{album}` `{current}`/`{position}` `{duration}` `{volume}` `{status}` `{mode}` `{id}` `{liked}`。覆盖配置 `cli_status_template` |
| `--json` | JSON 输出（优先于配置 `cli_status_format`） |
| `-L, --list` | 列出当前播放队列（`>` 标记当前曲目）；配合 `--json` 输出原始 `QueueSnapshot` |
| `--socket <SOCKET>` | 指定实例的 socket（覆盖全局/默认） |

`{status}` 取值：`playing` / `paused` / `stopped`；`{mode}` 取值：`sequential` / `repeat_one` /
`repeat_all` / `shuffle` / `heartbeat`。

```bash
pigma status                                   # plain（默认模板来自配置）
pigma status --json | jq '.name'
pigma status -L                                # 队列列表
pigma status -L --json                         # 队列原始 JSON
pigma status --template "{artist} – {name} [{status}] vol {volume}%"
```

---

## `pigma list <endpoint>` – 列出端点内容

无需运行实例，直接解析端点并打印其歌单/歌曲，带 1 起始序号（即 `--playlist N` 用的下标）：

```bash
pigma list toplist          # 打印榜单歌单列表
pigma list __liked__        # 打印歌曲列表
pigma list user_song_list
```

输出：
- 歌单类端点 → `  1. 歌单名`
- 歌曲类端点 → `  1. 歌曲名 - 歌手`

---

## `pigma msg <ACTION> [VALUE]` – 控制播放

```bash
pigma msg [OPTIONS] <ACTION> [VALUE]
```

| 动作 | 别名 | 说明 |
|---|---|---|
| `play` | | 播放 |
| `pause` | | 暂停 |
| `next` | | 下一首 |
| `previous` | `prev` | 上一首 |
| `switch-list <ENDPOINT>` | `list` `switch` | 动态切换队列到指定端点；歌单端点用 `--playlist N` 选第 N 个（1 起始） |
| `volume <VALUE>` | | `75`=绝对音量 0-100；`+5`/`-10`=相对 ±% |
| `mode` | | 切换播放模式 |
| `like` | | 喜欢当前曲目 |
| `dislike` | | 不喜欢当前曲目 |
| `toggle_like` | `unlike` `toggle` | 喜欢/取消喜欢（切换） |

选项：`--playlist <INDEX>`（switch-list 用）、`--socket <SOCKET>`。

```bash
pigma msg play
pigma msg next
pigma msg volume 75
pigma msg volume +5
pigma msg switch-list toplist --playlist 2
pigma msg toggle_like
```

---

## IPC 协议（直接走 socket/管道）

子命令底层就是往 socket/管道发一行 JSON、收一行 JSON（需换行结尾）。
`msg` 成功回 `{"ok":true}`。可用 socat / PowerShell / 脚本直接控制。

> `action` 是内部标签对象，必须写成 `{"cmd":"msg","action":{"action":...}}` 的嵌套形式
> （即 `pigma msg` 实际发送的 JSON）；写成 `{"action":"play"}` 会被服务端丢弃。

```bash
# 查询状态（回一行 JSON）
printf '{"cmd":"status"}\n' | socat - "$HOME/.cache/pigma/pigma.sock"
# 列出播放队列
printf '{"cmd":"list"}\n' | socat - "$HOME/.cache/pigma/pigma.sock"
# 播放控制（注意嵌套的 action 对象）
printf '{"cmd":"msg","action":{"action":"next"}}\n' | socat - "$HOME/.cache/pigma/pigma.sock"
printf '{"cmd":"msg","action":{"action":"play"}}\n' | socat - "$HOME/.cache/pigma/pigma.sock"
# 音量：绝对（0.0-1.0）或相对增量
printf '{"cmd":"msg","action":{"action":"volume","absolute":0.75}}\n' | socat - "$HOME/.cache/pigma/pigma.sock"
printf '{"cmd":"msg","action":{"action":"volume","delta":0.05}}\n'    | socat - "$HOME/.cache/pigma/pigma.sock"
# 切换队列（歌单端点可用 "playlist" 选第 N 个，1 起始）
printf '{"cmd":"msg","action":{"action":"switch_list","endpoint":"toplist","playlist":2}}\n' | socat - "$HOME/.cache/pigma/pigma.sock"
```

Windows 命名管道协议相同，socket 路径可用 `--socket <pipe-name>` 自定义。

---

## 端点参考（与导航项一致）

| 端点 | 类型 | 说明 |
|---|---|---|
| `__liked__` | 歌曲（默认） | 我喜欢的音乐，需登录 |
| `recommend_songs` | 歌曲 | 每日推荐歌曲，需登录 |
| `user_cloud_disk` | 歌曲 | 我的云盘，需登录 |
| `__download__` | 歌曲 | 本地下载 |
| `__local_music__` | 歌曲 | 本地音乐（扫描 `~/Music`） |
| `__recent__` | 歌曲 | 最近播放，需登录 |
| `recommend_resource` | 歌单 | 每日推荐歌单，需登录 |
| `toplist` | 歌单 | 排行榜 |
| `top_song_list` | 歌单 | 热门歌单 |
| `user_radio_sublist` | 歌单 | 我的电台，需登录 |
| `user_song_list` | 歌单 | 用户歌单，需登录 |
| `user_created_song_list` | 歌单 | 创建的歌单，需登录 |
| `user_subscribed_song_list` | 歌单 | 订阅的歌单，需登录 |
| `album_sublist` | 歌单 | 收藏的专辑，需登录 |
| `search` | 其他 | 搜索热榜（无可播队列） |
| `top_singers` | 其他 | 热门歌手（无可播队列） |

歌单类端点解析为「一组歌单」，默认加载第一个，可用 `--playlist N` 选第 N 个（1 起始）；
先用 `pigma list <endpoint>` 查看序号。需登录的端点未登录时返回 `未登录`。

---

## 配置（简要）

配置文件默认 `~/.config/pigma/config.toml`（Linux）。与 CLI 相关的项：
- `cli_status_template`：`pigma status` 默认 plain 模板。
- `cli_status_format`：`plain` 或 `json`；命令行 `--json` 优先。
- 其他项见 `config.example.toml`。

---

## 典型用法

```bash
# 1. 后台守护 + 开始播放
pigma -d
pigma msg play

# 2. Waybar 状态模块（参考 waybar/pigma 脚本）
pigma status --template "{artist} – {name} [{status}]"

# 3. 音量快捷键
pigma msg volume +5
pigma msg volume -5

# 4. 动态换队列
pigma msg switch-list recommend_songs

# 5. 多实例（自定义 socket）
pigma -d --socket /tmp/music.sock
pigma status --socket /tmp/music.sock
pigma msg --socket /tmp/music.sock play
```

---

## 故障排查

| 问题 | 解决 |
|---|---|
| `pigma status` 连接失败 | 确认实例在运行（TUI 或 `pigma -d`）；检查 `--socket` 路径 |
| `-d` 后无播放 | 必须显式 `pigma msg play` |
| 提示未登录 | 需登录端点（`__liked__` 等）要求已登录，先在 TUI 登录（`L` 键） |
| 队列为空 | 端点可能不是歌曲类；用 `pigma list <endpoint>` 确认 |
| `--playlist N` 越界 | 先用 `pigma list <endpoint>` 查看可用序号 |
| 未知动作 | `pigma msg --help` 查看合法动作 |

---

## Notes
- 所有子命令都是即发即走（非阻塞），走本地 socket/管道，无需网络。
- 交互界面与守护进程都暴露同一 IPC；守护进程可挂 systemd / Waybar。
- 完整帮助：`pigma --help`、`pigma status --help`、`pigma msg --help`。