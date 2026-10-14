/** 尚未 dispatch 可取消；beforeDispatch 只在真实 writer 调用前同步执行。 */
export interface InputWriteAdmission {
  signal?: AbortSignal
  beforeDispatch?: () => void
}
