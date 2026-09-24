export type PlayerjsOptions = {
  id: string
  file?: string
  poster?: string
  title?: string
  subtitle?: string
  default_subtitle?: string
  autoplay?: 0 | 1
  ready?: string
}

export type PlayerjsInstance = {
  /**
   * PlayerJS takes a command plus optional arguments. Some commands
   * (`getCurrentTime`, `getDuration`) take a callback instead of a value.
   */
  api: (command: string, ...args: unknown[]) => unknown
}

declare global {
  interface Window {
    Playerjs: new (options: PlayerjsOptions) => PlayerjsInstance
    PlayerjsEvents?: (event: string, id: string, data?: unknown) => void
  }
}

export {}
