import { createContext, useContext, useEffect, useState, type ReactNode } from 'react'
import { get, post, setToken, getToken, type User } from './api'

interface AuthCtx {
  user: User | null
  loading: boolean
  login: (username: string, password: string) => Promise<void>
  register: (username: string, password: string, displayName?: string) => Promise<void>
  logout: () => void
}

const Ctx = createContext<AuthCtx>(null as any)
export const useAuth = () => useContext(Ctx)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    if (!getToken()) { setLoading(false); return }
    get<User>('/me').then(setUser).catch(() => setToken('')).finally(() => setLoading(false))
  }, [])

  const login = async (username: string, password: string) => {
    const r = await post<{ token: string; user: User }>('/auth/login', { username, password })
    setToken(r.token)
    setUser(r.user)
  }
  const register = async (username: string, password: string, displayName?: string) => {
    const r = await post<{ token: string; user: User }>('/auth/register', { username, password, displayName })
    setToken(r.token)
    setUser(r.user)
  }
  const logout = () => { setToken(''); setUser(null); location.href = '/login' }

  return <Ctx.Provider value={{ user, loading, login, register, logout }}>{children}</Ctx.Provider>
}