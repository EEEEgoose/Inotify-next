import 'vue-router'

declare module 'vue-router' {
  interface RouteMeta {
    title?: string
    admin?: boolean
    public?: boolean
  }
}

declare global {
  /** The part of Cloudflare Turnstile's browser API in use (render=explicit). */
  interface Turnstile {
    render(container: HTMLElement, options: Record<string, unknown>): string
    reset(widgetId: string): void
    remove(widgetId: string): void
  }

  interface Window {
    turnstile?: Turnstile
  }
}
