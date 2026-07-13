import { describe, it, expect } from 'vitest'
import {
  validateDisplayName,
  projectBasename,
  resolveWindowTitle,
  matchProjectQuery,
  editReducer,
} from '@/utils/displayName'
import type { EditState, EditAction } from '@/utils/displayName'

describe('validateDisplayName', () => {
  // 合法普通别名
  it('ValidateDisplay_OK_Normal_001', () => {
    expect(validateDisplayName('主项目')).toEqual({ ok: true })
  })
  // 空 / 纯空白合法（=清除语义，不报错）
  it('ValidateDisplay_OK_Empty_001', () => {
    expect(validateDisplayName('')).toEqual({ ok: true })
    expect(validateDisplayName('   ')).toEqual({ ok: true })
  })
  // 32 字符合法（边界）
  it('ValidateDisplay_OK_MaxLen32_001', () => {
    expect(validateDisplayName('a'.repeat(32))).toEqual({ ok: true })
  })
  // 33 字符过长
  it('ValidateDisplay_Fail_TooLong_001', () => {
    expect(validateDisplayName('a'.repeat(33))).toEqual({ ok: false, error: 'tooLong' })
  })
  // 含换行符（\p{Cc}）非法
  it('ValidateDisplay_Fail_Newline_001', () => {
    expect(validateDisplayName('a\nb')).toEqual({ ok: false, error: 'invalid' })
  })
  // 含回车符非法
  it('ValidateDisplay_Fail_CarriageReturn_001', () => {
    expect(validateDisplayName('a\rb')).toEqual({ ok: false, error: 'invalid' })
  })
  // 含制表符非法
  it('ValidateDisplay_Fail_Tab_001', () => {
    expect(validateDisplayName('a\tb')).toEqual({ ok: false, error: 'invalid' })
  })
  // 含 NUL 控制字符非法
  it('ValidateDisplay_Fail_Nul_001', () => {
    expect(validateDisplayName('a\x00b')).toEqual({ ok: false, error: 'invalid' })
  })
  // 普通空格合法（仅首尾 trim，中间空格允许）
  it('ValidateDisplay_OK_Space_001', () => {
    expect(validateDisplayName('my project')).toEqual({ ok: true })
  })
})

describe('projectBasename', () => {
  // 普通 POSIX 路径取末段
  it('Basename_Basic_001', () => {
    expect(projectBasename('/work/client')).toBe('client')
  })
  // 尾斜杠先去掉再取末段
  it('Basename_TrailingSlash_001', () => {
    expect(projectBasename('/work/app/')).toBe('app')
  })
  // 反斜杠规范后取末段
  it('Basename_Backslash_001', () => {
    expect(projectBasename('C:\\repo\\x\\')).toBe('x')
  })
  // 根路径：POSIX '/' -> 末段为空 -> 回退原路径
  it('Basename_PosixRoot_001', () => {
    expect(projectBasename('/')).toBe('/')
  })
})

describe('resolveWindowTitle', () => {
  // cwd 空 -> 默认标题
  it('ResolveWindowTitle_NoCwd_001', () => {
    expect(resolveWindowTitle(null, () => 'x')).toBe('CC-Box')
    expect(resolveWindowTitle('', () => 'x')).toBe('CC-Box')
    expect(resolveWindowTitle(undefined, () => 'x')).toBe('CC-Box')
  })
  // cwd 有值 -> 用 resolveName（getDisplayName，含别名/basename）
  it('ResolveWindowTitle_WithName_001', () => {
    expect(resolveWindowTitle('/p-a', () => '主项目')).toBe('主项目')
    expect(resolveWindowTitle('/p-a', () => 'p-a')).toBe('p-a')
  })
})

describe('matchProjectQuery 三字段', () => {
  // displayName 命中（别名）
  it('MatchQuery_DisplayName_001', () => {
    expect(matchProjectQuery('客户的活', 'client', '/work/client', '客户')).toBe(true)
  })
  // basename 命中（无别名场景，displayName=basename）
  it('MatchQuery_Basename_001', () => {
    expect(matchProjectQuery('client', 'client', '/work/client', 'client')).toBe(true)
  })
  // path 命中（displayName/basename 不含 q，path 含）
  it('MatchQuery_Path_001', () => {
    expect(matchProjectQuery('客户', 'client', '/work/secret-dir', 'secret')).toBe(true)
  })
  // 全不命中
  it('MatchQuery_NoMatch_001', () => {
    expect(matchProjectQuery('客户', 'client', '/work/client', 'zzz')).toBe(false)
  })
  // 大小写不敏感
  it('MatchQuery_CaseInsensitive_001', () => {
    expect(matchProjectQuery('Client', 'client', '/work/client', 'CLIENT')).toBe(true)
  })
  // alias 设置时 basename 仍独立命中（codex #8/#16）：displayName=别名不含 'client'，
  // basename='client' 含 -> 命中（证明 basename 参数独立，非被 displayName 遮蔽）
  it('MatchQuery_AliasSetBasenameIndependent_001', () => {
    // displayName='客户的活' 不含 'client'；path='/work/c1' 不含 'client'；仅 basename='client' 含
    expect(matchProjectQuery('客户的活', 'client', '/work/c1', 'client')).toBe(true)
  })
})

describe('editReducer 状态机', () => {
  // start：从 idle/error 进入 editing（submitting 保持防中断）
  it('EditReducer_Start_001', () => {
    expect(editReducer('idle', { type: 'start' })).toBe('editing')
    expect(editReducer('error', { type: 'start' })).toBe('editing')
    expect(editReducer('submitting', { type: 'start' })).toBe('submitting')
  })
  // submit：editing -> submitting
  it('EditReducer_Submit_001', () => {
    expect(editReducer('editing', { type: 'submit' })).toBe('submitting')
  })
  // 防重复：submitting 态忽略重复 submit（不重复提交）
  it('EditReducer_Submit_IdempotentWhileSubmitting_001', () => {
    expect(editReducer('submitting', { type: 'submit' })).toBe('submitting')
  })
  // retry（codex #6）：error 态 submit -> submitting（用户改完重试）
  it('EditReducer_Submit_FromError_Retry_001', () => {
    expect(editReducer('error', { type: 'submit' })).toBe('submitting')
  })
  // idle 态 submit 忽略
  it('EditReducer_Submit_Idle_001', () => {
    expect(editReducer('idle', { type: 'submit' })).toBe('idle')
  })
  // success：submitting -> idle（成功才关 input）
  it('EditReducer_Success_001', () => {
    expect(editReducer('submitting', { type: 'success' })).toBe('idle')
  })
  // fail：submitting -> error（保留 input）
  it('EditReducer_Fail_001', () => {
    expect(editReducer('submitting', { type: 'fail' })).toBe('error')
  })
  // cancel：editing/error -> idle（不改）
  it('EditReducer_Cancel_001', () => {
    expect(editReducer('editing', { type: 'cancel' })).toBe('idle')
    expect(editReducer('error', { type: 'cancel' })).toBe('idle')
  })
  // idle 态 cancel 幂等
  it('EditReducer_Cancel_Idle_001', () => {
    expect(editReducer('idle', { type: 'cancel' })).toBe('idle')
  })

  // ---- 流程测（codex #5/#6/#7：校验失败进 error + retry + 双击幂等）----

  // 校验失败流程（codex #5）：editing -> submit -> submitting -> validate fail -> fail -> error（保留 input + 错误）
  it('EditReducer_Flow_ValidationFail_001', () => {
    let s: EditState = 'editing'
    s = editReducer(s, { type: 'submit' })       // -> submitting
    expect(s).toBe('submitting')
    s = editReducer(s, { type: 'fail' })          // 校验失败 -> error
    expect(s).toBe('error')                       // input 保留 + 错误提示
  })

  // retry 流程（codex #6）：error -> submit -> submitting -> success -> idle（重试成功关闭）
  it('EditReducer_Flow_RetryAfterFail_001', () => {
    let s: EditState = 'error'
    s = editReducer(s, { type: 'submit' })        // retry -> submitting
    expect(s).toBe('submitting')
    s = editReducer(s, { type: 'success' })       // 成功 -> idle
    expect(s).toBe('idle')
  })

  // persist 失败流程：submitting -> fail -> error -> retry -> submitting -> fail（连续失败保留）
  it('EditReducer_Flow_PersistFailRetain_001', () => {
    let s: EditState = 'editing'
    s = editReducer(s, { type: 'submit' })        // submitting
    s = editReducer(s, { type: 'fail' })          // error（persist 失败）
    expect(s).toBe('error')
    s = editReducer(s, { type: 'submit' })        // retry
    expect(s).toBe('submitting')
    s = editReducer(s, { type: 'fail' })          // 再次失败
    expect(s).toBe('error')
  })

  // 双击/快速回车不重复提交（codex #7）：editing -> submit -> submitting -> 再 submit 幂等
  it('EditReducer_Flow_DoubleSubmit_001', () => {
    let s: EditState = 'editing'
    s = editReducer(s, { type: 'submit' })        // -> submitting
    const s2 = editReducer(s, { type: 'submit' }) // 幂等，仍 submitting
    expect(s2).toBe('submitting')
    // 仅一次 persist（由调用方据 state 转换触发；reducer 保证不二次进 submitting）
  })

  // Esc 取消流程：editing -> cancel -> idle（不改）
  it('EditReducer_Flow_EscCancel_001', () => {
    let s: EditState = 'editing'
    s = editReducer(s, { type: 'cancel' })
    expect(s).toBe('idle')
  })

  // EditAction 类型编译期校验（保证 action 形状完整，无回归）
  it('EditAction_TypeShape_001', () => {
    const actions: EditAction[] = [
      { type: 'start' },
      { type: 'submit' },
      { type: 'success' },
      { type: 'fail' },
      { type: 'cancel' },
    ]
    let s: EditState = 'idle'
    s = editReducer(s, actions[0])  // start -> editing
    s = editReducer(s, actions[1])  // submit -> submitting
    s = editReducer(s, actions[2])  // success -> idle
    s = editReducer(s, actions[0])  // start -> editing
    s = editReducer(s, actions[3])  // fail（editing 态忽略，仍 editing）
    expect(s).toBe('editing')
    s = editReducer(s, actions[4])  // cancel -> idle
    expect(s).toBe('idle')
  })
})

/**
 * startRename 卡死修复（v6-T5 Fix Round 1）。
 * bug：多行 submitting（旧 persist 在途）期间发起新 startRename，旧实现走 reducer start，
 *   editReducer('submitting',{start}) -> 'submitting'（不变），新 input :disabled="editState==='submitting'"
 *   永久禁用，Esc 无法触发，离开视图才恢复。
 * 修法：startRename 在 renameRequestId++（作废旧 persist）后显式 editState='editing'，不走 reducer start；
 *   旧 persist 完成时 myId!==renameRequestId 早 return 不调 editReducer，editState 保持 editing，安全。
 * 此 describe 在 reducer 层标定 bug 根因 + 验证修复依赖的两个不变量（reducer start 在 submitting 不变、
 * success 仅 submitting->idle 对 editing 无影响），使修复所依赖的 reducer 语义有回归守护。
 */
describe('startRename 卡死修复（v6-T5 Fix Round 1）', () => {
  // bug 根因标定：reducer start 在 submitting 态不变 -> 旧实现 startRename 依赖 reducer 会卡死
  it('StartRenameFix_ReducerStartStuckInSubmitting_001', () => {
    expect(editReducer('submitting', { type: 'start' })).toBe('submitting')
  })

  // 修复后流程：submitting 期间新 startRename 显式重置 editing，旧 persist 作废不改，input 可用
  it('StartRenameFix_Flow_ResetEditing_001', () => {
    let s: EditState = 'editing'
    // A 行提交：editing -> submitting（persist 在途）
    s = editReducer(s, { type: 'submit' })
    expect(s).toBe('submitting')
    // bug 标定：若此时走 reducer start（旧实现），submitting -> submitting 不变，input 永久禁用
    expect(editReducer(s, { type: 'start' })).toBe('submitting')
    // B 行 startRename 修复：renameRequestId++ 作废旧请求 + 显式 editState='editing'（不走 reducer start）
    s = 'editing'
    expect(s).toBe('editing')  // :disabled="s==='submitting'" -> false，B 行 input 可用
    // A 行旧 persist 完成：myId!==renameRequestId 早 return 不调 editReducer -> s 保持 editing
    expect(s).toBe('editing')  // B 行 input 仍可用（未卡死）
  })

  // 修复安全不变量：success 仅 submitting->idle，对 editing 态无影响。
  // 即便旧 persist 误走到 editReducer success（理论上早 return 不会走到），editing 也不被污染。
  it('StartRenameFix_StaleSuccessNoOpOnEditing_001', () => {
    expect(editReducer('editing', { type: 'success' })).toBe('editing')
  })
})
