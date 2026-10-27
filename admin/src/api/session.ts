import { reactive } from 'vue'
import { me, type Action, type Me } from './client'

/** The logged-in user and what they may do; loaded once by the layout. */
export const session = reactive<{ user: Me | null }>({ user: null })

export async function loadSession() {
  session.user = await me()
}

export function can(entity: string, action: Action = 'read'): boolean {
  return session.user?.permissions?.[entity]?.includes(action) ?? false
}
