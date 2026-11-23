[English](README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | **简体中文**

# telegram

Telegram 聊天提供方 `bmo-telegram`。它通过基础库 `bmo-chat` 的聊天协议，在聊天接收端面前充当 Telegram:声明自己
支持的读取模式、能做的事以及房间所读的方言(dialect)，通过机器人账号读取房间，发送十二种动词，并把房间里的发言
交给接收端。机器人看不到或取不到的内容，以及所有者本人的发言，由只读的所有者本人账号在背后负责，该账号由一个
独立进程，即读取器(reader)保持。

## 目录结构

| 文件夹 | 内容 |
|---|---|
| `src/` | 服务自身的包:提供方、Bot API 连接及其路由 |
| `dialect/` | `telegram-dialect`，即 `telegram-html` 方言:把 Markdown 转为 Telegram 的 HTML、结构检查、折叠以及拆分为多条消息 |
| `picture/` | `telegram-picture`，图片的读取与重绘:类型、透明度、Telegram 作为照片接收的上传，以及控制在大小限制内的图片 |
| `reader/` | 读取器，一个通过 MTProto 保持所有者账号的 bun 包，测试位于 `reader/tests/` |
| `tools/` | 部署时安装的托管工具:仅凭所有者的指令，通过服务获取某个聊天的近期消息 |
| `policy/` | 提供方的固定数值和英文文案:限额、拒绝和占位符 |
| `tests/` | 覆盖整个仓库的检查，以及服务的测试 |
| `deploy/` | 部署时链接的两个服务单元:Rust 提供方和 bun 读取器 |
| `docs/` | 描述提供方行为的权威文档 |

根目录下有同时作为服务包的 Cargo 工作区清单、`VERSION` 中的版本、`testbed.toml`(测试平台关于如何测试、检查、
构建和部署本仓库的声明)、`rustfmt.toml` 和 `CLAUDE.md`。

## 文档

| 文档 | 内容 |
|---|---|
| `docs/api.md` | 提供方固定的公开 API:聊天协议的路由、线上格式和页面;以及以机器可读形式描述同一契约的 `docs/openapi.json` |
| `docs/boundary-api.md` | 边界:提供的路由、对接收端的调用、服务与读取器之间的接缝，以及配置 |
| `docs/telegram.md` | 提供方:声明、读取房间的方式、每种发送的行为、所有者的声音，以及 `telegram-html` 方言 |
| `docs/reader.md` | 读取器:所有者账号、连接、历史遍历、头像，以及为何使用 TypeScript |
| `docs/tools.md` | 托管工具:入口、参数、所有者的指令，以及到达提供方的位置 |
| `docs/checks.md` | 仓库检查:依赖、清单、目录结构、版本、韩文字符、个人数据和名称 |
| `docs/PERSONAL-DATA.md` | 仓库的个人数据禁令:被跟踪的文件不得包含什么、用什么代替，以及保障这一点的检查 |

## 版本

- **整个仓库只有一个版本，** 即工作区的版本。`VERSION` 保存该版本，工作区的版本与之相同，读取器的
  `package.json` 也使用同一版本。
- **版本只在所有者宣布发布时变动，** 规则见 bmo-chat 的 `docs/conventions.md`「Versions」。提交、部署、修复或
  契约变更都不会改变版本，部署通过提交来区分。

## 构建与测试

Cargo 通过测试平台的 Cargo 动词运行，在唯一的共享构建目录中构建;若已安装的测试平台没有自己的 Cargo 动词，则由
`bmo cargo` 提供同样的动词:

```sh
bmo cargo check --workspace --all-targets
bmo cargo test --workspace
bmo cargo clippy --workspace --all-targets -- -D warnings
bmo cargo fmt --all --check
```

读取器的测试在 `reader/` 中运行:

```sh
bun install
bun test
```
