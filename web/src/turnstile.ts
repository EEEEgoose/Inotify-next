// Cloudflare Turnstile, loaded on demand (explicit rendering) the first time a widget is shown.

const SCRIPT = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit'

let loading: Promise<Turnstile> | undefined

export function loadTurnstile(): Promise<Turnstile> {
  loading ??= new Promise<Turnstile>((resolve, reject) => {
    const script = document.createElement('script')
    script.src = SCRIPT
    script.async = true
    script.onload = () => (window.turnstile ? resolve(window.turnstile) : reject(new Error('人机验证组件加载失败')))
    script.onerror = () => {
      loading = undefined
      script.remove()
      reject(new Error('人机验证组件加载失败，请检查网络后刷新'))
    }
    document.head.appendChild(script)
  })
  return loading
}
