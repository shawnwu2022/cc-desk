#!/bin/bash
# CC Desk Hook Reporter — 跨平台兼容，任何异常均 exit 0
#
# 作用：将 Claude Code 的 hook 事件通过 HTTP POST 发送给 CC Desk
# 触发条件：由 cc-desk-monitor plugin 的 hooks.json 注册
#
# 环境变量（由 CC Desk spawn PTY 时注入）：
# - CC_BOX_HOOK_PORT   CC Desk HTTP 服务器端口（未设置 = 非 CC Desk 会话）
# - CC_BOX_SESSION_ID  当前 PTY 的唯一标识（用于区分多终端）
#
# 安全保障：
# - CC_BOX_HOOK_PORT 未设置时静默退出，不影响 Claude
# - curl 不可用时跳过
# - 所有错误重定向到 /dev/null
# - 超时 3 秒，hook timeout 5 秒作为二次保险
#
# UTF-8 修复（v1.0.2）：
# - 使用 curl -d @- 直接从 stdin 读取数据
# - 绕过 bash 变量处理，避免多字节 UTF-8 序列被截断

[ -z "$CC_BOX_HOOK_PORT" ] && exit 0

command -v curl >/dev/null 2>&1 || exit 0

# 直接从 stdin 读取数据发送，不经过 bash 变量处理
# 这避免了 Windows Git Bash 对 UTF-8 多字节序列的截断问题
curl -s --max-time 3 -X POST "http://127.0.0.1:$CC_BOX_HOOK_PORT/hook" \
  -H "Content-Type: application/json" \
  -H "X-CC-Box-Session: ${CC_BOX_SESSION_ID:-}" \
  -d @- >/dev/null 2>&1

exit 0
