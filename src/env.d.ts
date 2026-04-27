/// <reference types="vue/compiler-sfc" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<object, object, unknown>
  export default component
}

// Tauri API 类型声明（由 @tauri-apps/api 提供，此处仅作补充）
declare module '@tauri-apps/api/core' {
  export function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>
}

declare module '@tauri-apps/api/event' {
  export interface Event<T> {
    id: number
    event: string
    payload: T
  }

  export function listen<T>(event: string, handler: (event: Event<T>) => void): Promise<() => void>
  export function once<T>(event: string, handler: (event: Event<T>) => void): Promise<() => void>
  export function emit(event: string, payload?: unknown): Promise<void>
}

declare module '@tauri-apps/plugin-dialog' {
  export interface OpenOptions {
    directory?: boolean
    multiple?: boolean
    title?: string
    filters?: Array<{ name: string; extensions: string[] }>
  }

  export function open(options?: OpenOptions): Promise<string | string[] | null>
}