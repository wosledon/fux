import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { CssBaseline, ThemeProvider } from '@mui/material'
import type { PaletteMode } from '@mui/material/styles'
import { createAppTheme } from './theme'

const STORAGE_KEY = 'fux-color-mode'
const ColorModeCtx = createContext<{ mode: PaletteMode; toggleMode: () => void }>({ mode: 'light', toggleMode: () => {} })

function initialMode(): PaletteMode {
  const saved = window.localStorage.getItem(STORAGE_KEY)
  if (saved === 'light' || saved === 'dark') return saved
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

export function AppThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setMode] = useState<PaletteMode>(initialMode)
  const theme = useMemo(() => createAppTheme(mode), [mode])
  const value = useMemo(() => ({ mode, toggleMode: () => setMode(m => m === 'dark' ? 'light' : 'dark') }), [mode])

  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, mode)
    document.documentElement.style.colorScheme = mode
  }, [mode])

  return (
    <ColorModeCtx.Provider value={value}>
      <ThemeProvider theme={theme}>
        <CssBaseline />
        {children}
      </ThemeProvider>
    </ColorModeCtx.Provider>
  )
}

export const useColorMode = () => useContext(ColorModeCtx)
