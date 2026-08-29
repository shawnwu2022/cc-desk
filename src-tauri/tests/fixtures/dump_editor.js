// 外部编辑器转储助手：claude 的 Ctrl+G 会以「待编辑文件路径」为参数调用 $EDITOR。
// 本助手把该文件内容复制到 CC_DUMP_OUT 指定路径后立即退出（内容原样返回编辑器），
// 使 e2e 探针能读到 Claude 输入编辑器的真实缓冲——绕开 VT 渲染流的视口/批处理失真。
const fs = require('node:fs');
const path = process.argv[process.argv.length - 1];
const out = process.env.CC_DUMP_OUT;
try {
  if (out && path && fs.existsSync(path)) {
    fs.mkdirSync(require('node:path').dirname(out), { recursive: true });
    fs.copyFileSync(path, out);
  }
} catch (e) {
  /* 转储失败不影响编辑器退出，测试按超时判定 */
}
process.exit(0);
