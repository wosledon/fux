import { createTheme } from '@mui/material/styles'

// Fux 设计语言：深空底色 + 靛紫渐变强调 + 分层表面 + 柔和描边
export const theme = createTheme({
  palette: {
    mode: 'dark',
    primary: { main: '#6d8dff', light: '#9db4ff', dark: '#4c6ef5' },
    secondary: { main: '#b197fc' },
    success: { main: '#4ade80' },
    warning: { main: '#fbbf24' },
    error: { main: '#f87171' },
    info: { main: '#60a5fa' },
    background: { default: '#0a0d13', paper: '#10141c' },
    divider: 'rgba(255,255,255,0.07)',
    text: { primary: '#e6eaf2', secondary: '#8b93a7' },
    action: { hover: 'rgba(255,255,255,0.045)', selected: 'rgba(109,141,255,0.14)' },
  },
  shape: { borderRadius: 12 },
  typography: {
    fontFamily: '"Inter","Segoe UI","PingFang SC","Microsoft YaHei",system-ui,sans-serif',
    fontSize: 13,
    h4: { fontWeight: 800, letterSpacing: '-0.02em' },
    h5: { fontWeight: 700, letterSpacing: '-0.01em' },
    h6: { fontWeight: 700 },
    subtitle2: { fontWeight: 700 },
    button: { textTransform: 'none', fontWeight: 600 },
  },
  components: {
    MuiCssBaseline: {
      styleOverrides: {
        body: {
          scrollbarColor: '#2a3140 #0a0d13',
          '&::-webkit-scrollbar, & *::-webkit-scrollbar': { width: 10, height: 10 },
          '&::-webkit-scrollbar-thumb, & *::-webkit-scrollbar-thumb': {
            backgroundColor: '#2a3140', borderRadius: 8, border: '2px solid #0a0d13',
          },
          '&::-webkit-scrollbar-thumb:hover, & *::-webkit-scrollbar-thumb:hover': { backgroundColor: '#3b4356' },
          '&::-webkit-scrollbar-track, & *::-webkit-scrollbar-track': { backgroundColor: 'transparent' },
          '::selection': { backgroundColor: 'rgba(109,141,255,0.35)' },
        },
      },
    },
    MuiPaper: { defaultProps: { elevation: 0 }, styleOverrides: { root: { backgroundImage: 'none' } } },
    MuiCard: {
      styleOverrides: {
        root: { border: '1px solid rgba(255,255,255,0.07)', borderRadius: 14, backgroundImage: 'none' },
      },
    },
    MuiButton: {
      defaultProps: { disableElevation: true },
      styleOverrides: {
        root: { borderRadius: 10 },
        containedPrimary: {
          background: 'linear-gradient(135deg,#6d8dff 0%,#8b5cf6 100%)',
          '&:hover': { background: 'linear-gradient(135deg,#7d9aff 0%,#9a6ff7 100%)' },
        },
      },
    },
    MuiChip: { styleOverrides: { root: { borderRadius: 8, fontWeight: 600 }, sizeSmall: { height: 22 } } },
    MuiDialog: { styleOverrides: { paper: { borderRadius: 16, border: '1px solid rgba(255,255,255,0.09)', backgroundImage: 'none' } } },
    MuiDialogTitle: { styleOverrides: { root: { fontWeight: 700, fontSize: 16 } } },
    MuiOutlinedInput: { styleOverrides: { root: { borderRadius: 10 } } },
    MuiTableCell: {
      styleOverrides: {
        root: { borderBottom: '1px solid rgba(255,255,255,0.06)' },
        head: { color: '#8b93a7', fontWeight: 700, fontSize: 12, backgroundColor: 'rgba(255,255,255,0.02)' },
      },
    },
    MuiDrawer: { styleOverrides: { paper: { backgroundImage: 'none', borderRight: '1px solid rgba(255,255,255,0.07)' } } },
    MuiTabs: { styleOverrides: { indicator: { height: 3, borderRadius: 3 } } },
    MuiTab: { styleOverrides: { root: { textTransform: 'none', fontWeight: 600, minHeight: 44 } } },
    MuiTooltip: { defaultProps: { arrow: true } },
    MuiAlert: { styleOverrides: { root: { borderRadius: 10 } } },
    MuiListItemButton: { styleOverrides: { root: { borderRadius: 10, margin: '2px 8px', width: 'auto' } } },
  },
})

// 泳道 / 状态强调色
export const LANE_ACCENT: Record<string, string> = {
  planning: '#8b93a7',
  ready: '#60a5fa',
  work: '#6d8dff',
  review: '#b197fc',
  blocked: '#f87171',
  done: '#4ade80',
  archived: '#6b7280',
}
export const STATUS_ACCENT: Record<string, string> = {
  planning: '#8b93a7', ready: '#60a5fa', in_progress: '#6d8dff', review: '#b197fc',
  blocked: '#f87171', done: '#4ade80', archived: '#6b7280', cancelled: '#6b7280',
}