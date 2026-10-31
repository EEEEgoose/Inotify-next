# Inotify-next

[![ci](https://github.com/EEEEgoose/Inotify-next/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/EEEEgoose/Inotify-next/actions/workflows/ci.yml)
[![docker](https://github.com/EEEEgoose/Inotify-next/actions/workflows/docker.yml/badge.svg?branch=master)](https://github.com/EEEEgoose/Inotify-next/actions/workflows/docker.yml)

一个简易的自托管消息推送网关，类似 Server 酱：一次调用，把消息发到你配置的所有渠道。

基于 [xpnas/inotify](https://github.com/xpnas/inotify) 用 Rust + Vue 3 重写。

单个约 15MB 的可执行文件（Rust），内置 Web 管理界面和 Bark 服务端，空闲内存约 30MB。

## 通道支持

- 企业微信应用消息
- 钉钉群机器人（支持加签）
- 飞书群机器人（支持签名校验）
- Telegram 机器人
- SMTP 邮件
- 自定义 Webhook（GET / POST，模板占位符）
- Bark（内置 Bark 服务端，iPhone 直接用官方 Bark App 接收）

## 部署

```sh
docker run -d --name inotify -p 8000:8000 -v inotify_data:/data --restart=always ghcr.io/eeeegoose/inotify-next:latest
```

`latest` 为发布版，`master` 为开发版；Docker Hub 上的镜像名为 `inotify-next`。数据（SQLite）保存在 `/data`。

首次启动会创建管理员 `admin` 并在日志中打印随机密码（只打印一次），登录后请在「账号」页修改：

```sh
docker logs inotify 2>&1 | grep administrator
```

建议通过反向代理开启 HTTPS（Bark App 要求 HTTPS）：

```nginx
location / {
    proxy_pass http://127.0.0.1:8000;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

使用反向代理时请设置 `INOTIFY_TRUST_PROXY=1`（`docker run -e INOTIFY_TRUST_PROXY=1 ...`），否则登录失败次数会按代理的 IP 统计，所有访问者共用一个额度。只有服务端口不能被绕过代理直接访问时才应开启该项。

### 环境变量

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `INOTIFY_LISTEN` | `0.0.0.0:8000` | 监听地址 |
| `INOTIFY_DATA_DIR` | `data`（镜像内为 `/data`） | 数据目录 |
| `INOTIFY_SEND_CONCURRENCY` | `8` | 同时处理的发送任务数 |
| `INOTIFY_ALLOW_PRIVATE_TARGETS` | 关闭 | 允许 Webhook / 邮件访问内网地址（如家里的 Home Assistant）。默认禁止，防止 SSRF。开启后任何用户都能让服务器访问内网，**只适合单用户或可信用户** |
| `INOTIFY_TRUST_PROXY` | 关闭 | 位于反向代理之后：客户端 IP 取 `X-Forwarded-For` 的最后一项 |
| `INOTIFY_DISABLE_TURNSTILE` | 关闭 | 临时跳过登录人机验证，用于 Turnstile 密钥填错或无法访问 Cloudflare 时恢复登录 |

出站代理、登录人机验证在后台「系统设置」中配置，修改后立即生效。出站代理用于 Telegram、企业微信、APNs、Turnstile 等固定接口；Webhook、钉钉、飞书等用户填写的地址直连（开启 `INOTIFY_ALLOW_PRIVATE_TARGETS` 时也走代理）。

同一 IP 15 分钟内登录失败 10 次（或同一用户名失败 20 次）后暂停登录 15 分钟；修改或重置密码、禁用用户会让该用户的其他登录会话立即失效。

## 发送消息

在「发送接口」页获取 send token。用 send token 发送会发到所有已启用的渠道；换成某个渠道的 Key 则只发到那个渠道。

```sh
curl -X POST https://你的域名/api/v1/send \
  -H 'Authorization: Bearer <send token>' \
  -H 'Content-Type: application/json' \
  -d '{"title": "标题", "body": "内容"}'
```

兼容 2.x 的 GET 格式（标题和内容需 URL 编码）：

```text
https://你的域名/<send token>.send/标题/内容
https://你的域名/api/send?token=<send token>&title=标题&data=内容
```

## Bark

Inotify 按 [bark-server](https://github.com/Finb/bark-server) 的协议内置了 Bark 服务端，推送直接发往 APNs，无需另外部署 bark-server 或苹果开发者账号（使用 Bark 官方公开的推送密钥）。

1. 在「发送接口」页复制或扫码 Bark 服务器地址：`https://你的域名/bark/<send token>`
2. 在 Bark App 中添加该服务器，App 注册后会自动出现在「消息渠道」中
3. 支持 Bark 推送 API 的工具（Home Assistant、青龙等）可把服务器填成上面的地址、Key 填 send token，消息会分发到你的所有渠道

## 备份渠道

在「消息渠道」页点「导出」，设置密码后下载加密备份（Argon2id + AES-256-GCM，包含渠道密钥和 Bark 设备）；在新服务器或新账号上点「导入」，选择文件并输入同一密码即可恢复。已有相同 Key 的渠道会跳过；Key 被其他用户占用时生成新 Key。

## 登录人机验证（Cloudflare Turnstile）

1. 在 Cloudflare 控制台 → Turnstile 添加站点，域名填 Inotify 的域名，获得 Site Key 和 Secret Key
2. 在「系统设置」填入并启用，登录页随即出现验证组件；服务端会向 Cloudflare 校验每次登录的验证结果
3. 无法登录时，用 `INOTIFY_DISABLE_TURNSTILE=1` 重启后登录，修正设置再去掉该变量

## 从 inotify 2.x 升级

Inotify-next（3.0）是重写版本（.NET → Rust，Vue 2 → Vue 3），**不迁移 2.x 的数据**：

- 重新部署后需重新创建用户和渠道，send token 会变化；旧的 GET 调用格式仍然可用，替换 token 即可
- 容器端口从 80 改为 8000，数据目录从 `/inotify_data` 改为 `/data`
- Bark 服务器地址从 `?act=<token>` 改为 `/bark/<send token>`，需在 App 中重新添加

## 开发

```sh
# 后端（Rust stable）
cd server && cargo run          # http://127.0.0.1:8000
cargo test

# 前端（Node 22+），开发服务器把 /api 和 /bark 代理到后端
cd web && npm install && npm run dev
```

`npm run build` 生成的 `web/dist` 会在编译后端时嵌入可执行文件。
