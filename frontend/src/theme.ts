import { createTheme } from '@mui/material/styles'

// Fux 设计语言：克制的石墨色层级、靛蓝强调与清晰的内容边界
export const theme = createTheme({
  palette: {
    mode: 'dark',
    primary: { main: '#8b9dff', light: '#b8c3ff', dark: '#6879e8' },
    secondary: { main: '#b8a9f7' },
    success: { main: '#5ed6a0' },
    warning: { main: '#f4bf62' },
    error: { main: '#fb8490' },
    info: { main: '#74b9ff' },
    background: { default: '#0b0e14', paper: '#121722' },
    divider: 'rgba(148,163,184,0.12)',
    text: { primary: '#edf1f7', secondary: '#96a0b5' },
    action: { hover: 'rgba(148,163,184,0.075)', selected: 'rgba(139,157,255,0.13)' },
  },
  shape: { borderRadius: 12 },
  typography: {
    fontFamily: '"Inter","Segoe UI","PingFang SC","Microsoft YaHei",system-ui,sans-serif',
    fontSize: 14,
    h4: { fontWeight: 750, fontSize: 30, letterSpacing: '-0.035em' },
    h5: { fontWeight: 750, fontSize: 24, letterSpacing: '-0.025em' },
    h6: { fontWeight: 700, fontSize: 20, letterSpacing: '-0.015em' },
    subtitle1: { fontWeight: 650 },
    subtitle2: { fontWeight: 700 },
    body2: { lineHeight: 1.65 },
    button: { textTransform: 'none', fontWeight: 650 },
  },
  components: {
    MuiCssBaseline: {
      styleOverrides: {
        body: {
          scrollbarColor: '#30394a #0b0e14',
          '&::-webkit-scrollbar, & *::-webkit-scrollbar': { width: 9, height: 9 },
          '&::-webkit-scrollbar-thumb, & *::-webkit-scrollbar-thumb': {
            backgroundColor: '#30394a', borderRadius: 8, border: '2px solid transparent', backgroundClip: 'padding-box',
          },
          '&::-webkit-scrollbar-thumb:hover, & *::-webkit-scrollbar-thumb:hover': { backgroundColor: '#465168' },
          '&::-webkit-scrollbar-track, & *::-webkit-scrollbar-track': { backgroundColor: 'transparent' },
          '::selection': { backgroundColor: 'rgba(139,157,255,0.3)' },
        },
      },
    },
    MuiPaper: { defaultProps: { elevation: 0 }, styleOverrides: { root: { backgroundImage: 'none' } } },
    MuiCard: {
      styleOverrides: {
        root: { border: '1px solid rgba(148,163,184,0.13)', borderRadius: 16, backgroundImage: 'none' },
      },
    },
    MuiButton: {
      defaultProps: { disableElevation: true },
      styleOverrides: {
        root: { borderRadius: 10, minHeight: 38, paddingInline: 16 },
        sizeSmall: { minHeight: 32, paddingInline: 12 },
        containedPrimary: {
          backgroundColor: '#7f91f5', color: '#0b1020',
          '&:hover': { backgroundColor: '#93a4ff' },
        },
        outlined: { borderColor: 'rgba(148,163,184,0.22)' },
      },
    },
    MuiIconButton: { styleOverrides: { root: { borderRadius: 9 } } },
    MuiChip: { styleOverrides: { root: { borderRadius: 8, fontWeight: 600 }, sizeSmall: { height: 24 } } },
    MuiDialog: { styleOverrides: { paper: { borderRadius: 18, border: '1px solid rgba(148,163,184,0.16)', backgroundImage: 'none' } } },
    MuiDialogTitle: { styleOverrides: { root: { fontWeight: 700, fontSize: 18 } } },
    MuiOutlinedInput: {
      styleOverrides: {
        root: { borderRadius: 10, backgroundColor: 'rgba(255,255,255,0.018)' },
        notchedOutline: { borderColor: 'rgba(148,163,184,0.18)' },
      },
    },
    MuiTable: {
      styleOverrides: {
        root: { backgroundColor: '#121722', border: '1px solid rgba(148,163,184,0.12)', borderRadius: 16, borderCollapse: 'separate', overflow: 'hidden' },
      },
    },
    MuiTableCell: {
      styleOverrides: {
        root: { borderBottom: '1px solid rgba(148,163,184,0.09)', padding: '13px 16px' },
        sizeSmall: { padding: '11px 14px' },
        head: { color: '#96a0b5', fontWeight: 700, fontSize: 12, backgroundColor: 'rgba(148,163,184,0.035)', letterSpacing: '0.02em' },
      },
    },
    MuiTableRow: { styleOverrides: { root: { '&.MuiTableRow-hover:hover': { backgroundColor: 'rgba(148,163,184,0.035)' } } } },
    MuiTabs: { styleOverrides: { indicator: { height: 2, borderRadius: 2 } } },
    MuiTab: { styleOverrides: { root: { textTransform: 'none', fontWeight: 650, minHeight: 46 } } },
    MuiTooltip: { defaultProps: { arrow: true } },
    MuiAlert: { styleOverrides: { root: { borderRadius: 12, border: '1px solid rgba(148,163,184,0.1)' } } },
    MuiListItemButton: { styleOverrides: { root: { borderRadius: 10, margin: '3px 12px', width: 'auto' } } },
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