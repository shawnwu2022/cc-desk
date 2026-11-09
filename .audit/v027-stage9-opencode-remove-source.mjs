/*
MIT License

Copyright (c) 2025 opencode

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

Source: https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/session.ts#L391
该固定版本官方原始 generator 函数体，仅用于注入服务实验，没有启动 CLI 或原生服务。
*/
export const upstreamRemoveSource = "function* (sessionID) {\n        yield* result.get(sessionID)\n        yield* execution.interrupt(sessionID)\n        yield* execution.awaitIdle(sessionID)\n        yield* jobs.cancelSession(sessionID)\n        yield* transport.close(sessionID)\n        const children = yield* store.list({ parentID: sessionID })\n        yield* Effect.forEach(children, (child) => result.remove(child.id), { concurrency: 1, discard: true })\n        yield* environments.clear(sessionID)\n        yield* bus.publish(SessionEvent.Deleted, { sessionID })\n        yield* bus.remove(sessionID)\n      }"
