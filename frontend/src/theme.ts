import { alpha, createTheme, type PaletteMode } from '@mui/material/styles'

// Fux 设计语言：安静的中性色画布、明确边界与克制的品牌蓝
export const createAppTheme = (mode: PaletteMode) => {
  const dark = mode === 'dark'
  return createTheme({
  palette: {
    mode,
    primary: dark
      ? { main: '#7aa7ff', light: '#a9c5ff', dark: '#5287ef', contrastText: '#101722' }
      : { main: '#2563eb', light: '#60a5fa', dark: '#1d4ed8', contrastText: '#ffffff' },
    secondary: { main: dark ? '#b49aff' : '#7c3aed' },
    success: { main: dark ? '#4ade80' : '#15803d' },
    warning: { main: dark ? '#fbbf24' : '#b45309' },
    error: { main: dark ? '#f87171' : '#dc2626' },
    info: { main: dark ? '#38bdf8' : '#0284c7' },
    background: dark ? { default: '#0f141d', paper: '#171d28' } : { default: '#f6f7f9', paper: '#ffffff' },
    divider: dark ? '#2b3442' : '#e5e7eb',
    text: dark ? { primary: '#e7ebf2', secondary: '#9aa7b8' } : { primary: '#182230', secondary: '#667085' },
    action: dark
      ? { hover: 'rgba(255,255,255,0.055)', selected: 'rgba(122,167,255,0.14)' }
      : { hover: 'rgba(24,34,48,0.045)', selected: 'rgba(37,99,235,0.08)' },
  },
  shape: { borderRadius: 10 },
  typography: {
    fontFamily: '"Inter","Segoe UI","PingFang SC","Microsoft YaHei",system-ui,sans-serif',
    fontSize: 14,
    h4: { fontWeight: 700, fontSize: 32, letterSpacing: '-0.035em', lineHeight: 1.25 },
    h5: { fontWeight: 700, fontSize: 25, letterSpacing: '-0.025em', lineHeight: 1.3 },
    h6: { fontWeight: 700, fontSize: 18, letterSpacing: '-0.015em', lineHeight: 1.4 },
    subtitle1: { fontWeight: 650 },
    subtitle2: { fontWeight: 650, fontSize: 14 },
    body2: { lineHeight: 1.65 },
    button: { textTransform: 'none', fontWeight: 600 },
  },
  components: {
    MuiCssBaseline: {
      styleOverrides: theme => ({
        body: {
          colorScheme: theme.palette.mode,
          scrollbarColor: `${alpha(theme.palette.text.secondary, 0.38)} ${theme.palette.background.default}`,
          '&::-webkit-scrollbar, & *::-webkit-scrollbar': { width: 9, height: 9 },
          '&::-webkit-scrollbar-thumb, & *::-webkit-scrollbar-thumb': {
            backgroundColor: alpha(theme.palette.text.secondary, 0.32), borderRadius: 8, border: '2px solid transparent', backgroundClip: 'padding-box',
          },
          '&::-webkit-scrollbar-thumb:hover, & *::-webkit-scrollbar-thumb:hover': { backgroundColor: alpha(theme.palette.text.secondary, 0.5) },
          '&::-webkit-scrollbar-track, & *::-webkit-scrollbar-track': { backgroundColor: 'transparent' },
          '::selection': { backgroundColor: alpha(theme.palette.primary.main, 0.22) },
          '@keyframes fuxSpin': {
            from: { transform: 'rotate(0deg)' },
            to: { transform: 'rotate(360deg)' },
          },
          '@keyframes fuxNodePulse': {
            '0%, 100%': { boxShadow: `0 0 0 0 ${alpha(theme.palette.primary.main, 0.05)}, 0 2px 8px ${alpha(theme.palette.primary.main, 0.08)}` },
            '50%': { boxShadow: `0 0 0 4px ${alpha(theme.palette.primary.main, 0.12)}, 0 8px 24px ${alpha(theme.palette.primary.main, 0.18)}` },
          },
          '@keyframes fuxDash': {
            from: { strokeDashoffset: 24 },
            to: { strokeDashoffset: 0 },
          },
        },
      }),
    },
    MuiPaper: { defaultProps: { elevation: 0 }, styleOverrides: { root: { backgroundImage: 'none' } } },
    MuiCard: {
      styleOverrides: {
        root: ({ theme }) => ({ border: `1px solid ${theme.palette.divider}`, borderRadius: 12, backgroundImage: 'none', boxShadow: 'none' }),
      },
    },
    MuiButton: {
      defaultProps: { disableElevation: true },
      styleOverrides: {
        root: { borderRadius: 8, minHeight: 40, paddingInline: 16, boxShadow: 'none', transition: 'background-color .15s, border-color .15s, color .15s' },
        sizeSmall: { minHeight: 34, paddingInline: 12 },
        sizeLarge: { minHeight: 46 },
        contained: { '&:hover': { boxShadow: 'none' } },
        containedPrimary: ({ theme }) => ({
          backgroundColor: theme.palette.primary.main, color: theme.palette.primary.contrastText,
          '&:hover': { backgroundColor: theme.palette.primary.dark },
        }),
        outlined: ({ theme }) => ({
          borderColor: theme.palette.divider, backgroundColor: theme.palette.background.paper,
          '&:hover': { borderColor: alpha(theme.palette.text.secondary, 0.65), backgroundColor: theme.palette.action.hover },
        }),
        text: ({ theme }) => ({ color: theme.palette.text.secondary }),
        textPrimary: ({ theme }) => ({ color: theme.palette.primary.main }),
      },
    },
    MuiIconButton: { styleOverrides: { root: ({ theme }) => ({ borderRadius: 8, color: theme.palette.text.secondary, '&:hover': { backgroundColor: theme.palette.action.hover } }) } },
    MuiChip: {
      styleOverrides: {
        root: { borderRadius: 6, fontWeight: 600 },
        sizeSmall: { height: 24 },
        outlined: ({ theme }) => ({ borderColor: 'transparent', backgroundColor: theme.palette.action.hover, color: theme.palette.text.secondary }),
        filled: ({ theme }) => ({
          '&.MuiChip-colorDefault': { backgroundColor: theme.palette.action.hover, color: theme.palette.text.secondary },
          '&.MuiChip-colorPrimary': { backgroundColor: alpha(theme.palette.primary.main, dark ? 0.18 : 0.10), color: theme.palette.primary.main },
          '&.MuiChip-colorSecondary': { backgroundColor: alpha(theme.palette.secondary.main, dark ? 0.18 : 0.10), color: theme.palette.secondary.main },
          '&.MuiChip-colorSuccess': { backgroundColor: alpha(theme.palette.success.main, dark ? 0.16 : 0.10), color: theme.palette.success.main },
          '&.MuiChip-colorWarning': { backgroundColor: alpha(theme.palette.warning.main, dark ? 0.16 : 0.10), color: theme.palette.warning.main },
          '&.MuiChip-colorError': { backgroundColor: alpha(theme.palette.error.main, dark ? 0.16 : 0.10), color: theme.palette.error.main },
          '&.MuiChip-colorInfo': { backgroundColor: alpha(theme.palette.info.main, dark ? 0.16 : 0.10), color: theme.palette.info.main },
        }),
      },
    },
    MuiDialog: { styleOverrides: { paper: ({ theme }) => ({ borderRadius: 16, border: `1px solid ${theme.palette.divider}`, backgroundImage: 'none', boxShadow: '0 24px 80px rgba(0,0,0,0.24)' }) } },
    MuiDialogTitle: { styleOverrides: { root: { fontWeight: 700, fontSize: 19, padding: '24px 24px 16px' } } },
    MuiDialogContent: { styleOverrides: { root: { paddingInline: 24 } } },
    MuiDialogActions: { styleOverrides: { root: { padding: '16px 24px 20px' } } },
    MuiMenu: { styleOverrides: { paper: ({ theme }) => ({ borderRadius: 10, border: `1px solid ${theme.palette.divider}`, boxShadow: '0 12px 36px rgba(0,0,0,0.18)' }) } },
    MuiOutlinedInput: {
      styleOverrides: {
        root: ({ theme }) => ({
          borderRadius: 8, backgroundColor: theme.palette.background.paper, transition: 'border-color .15s, box-shadow .15s',
          '&:hover': { backgroundColor: theme.palette.background.paper },
          '&:hover .MuiOutlinedInput-notchedOutline': { borderColor: alpha(theme.palette.text.secondary, 0.65) },
          '&.Mui-focused': { backgroundColor: theme.palette.background.paper, boxShadow: `0 0 0 3px ${alpha(theme.palette.primary.main, 0.10)}` },
        }),
        notchedOutline: ({ theme }) => ({ borderColor: theme.palette.divider }),
      },
    },
    MuiInputLabel: { styleOverrides: { outlined: ({ theme }) => ({ color: theme.palette.text.secondary, '&.Mui-focused': { color: theme.palette.primary.main } }) } },
    MuiTable: {
      styleOverrides: {
        root: ({ theme }) => ({ backgroundColor: theme.palette.background.paper, borderRadius: 12, borderCollapse: 'separate', overflow: 'hidden', border: `1px solid ${theme.palette.divider}` }),
      },
    },
    MuiTableCell: {
      styleOverrides: {
        root: ({ theme }) => ({ borderBottom: `1px solid ${theme.palette.divider}`, padding: '14px 18px' }),
        sizeSmall: { padding: '12px 16px' },
        head: ({ theme }) => ({ color: theme.palette.text.secondary, fontWeight: 600, fontSize: 12, backgroundColor: theme.palette.background.default, letterSpacing: '0.02em' }),
      },
    },
    MuiTableRow: { styleOverrides: { root: ({ theme }) => ({ '&.MuiTableRow-hover:hover': { backgroundColor: theme.palette.action.hover } }) } },
    MuiTabs: { styleOverrides: { indicator: { height: 2, borderRadius: 1 } } },
    MuiTab: { styleOverrides: { root: ({ theme }) => ({ textTransform: 'none', fontWeight: 600, minHeight: 48, color: theme.palette.text.secondary, '&.Mui-selected': { color: theme.palette.primary.main } }) } },
    MuiTooltip: { defaultProps: { arrow: true } },
    MuiAlert: { styleOverrides: { root: { borderRadius: 8, border: 'none' } } },
    MuiListItemButton: { styleOverrides: { root: { borderRadius: 8, margin: '2px 12px', width: 'auto' } } },
  },
  })
}

// 泳道 / 状态强调色
export const LANE_ACCENT: Record<string, string> = {
  planning: '#98a2b3', ready: '#0284c7', work: '#2563eb', review: '#7c3aed',
  blocked: '#dc2626', done: '#15803d', archived: '#98a2b3',
}
export const STATUS_ACCENT: Record<string, string> = {
  planning: '#667085', ready: '#0277a8', in_progress: '#2563eb', review: '#7c3aed',
  blocked: '#dc2626', done: '#15803d', archived: '#667085', cancelled: '#667085',
}