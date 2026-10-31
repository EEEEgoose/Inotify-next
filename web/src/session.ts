import { reactive } from 'vue'
import { api, getSession, setSession, type User } from './api'

export const session = reactive<{ user: User | null }>({ user: null })

/** Loads the current user if there is a session; false when there is none (or it expired). */
export async function loadUser(): Promise<boolean> {
  if (session.user) return true
  if (!getSession()) return false
  try {
    session.user = await api.me()
    return true
  } catch {
    logout()
    return false
  }
}

export async function startSession(token: string) {
  setSession(token)
  session.user = null
  return loadUser()
}

export function logout() {
  setSession(null)
  session.user = null
}

export async function copy(text: string) {
  await navigator.clipboard.writeText(text)
}
