import { createTheme } from '@mui/material/styles'

// Fux 设计语言：轻盈的中性色画布、柔和控件与清晰的靛紫品牌色
export const theme = createTheme({
  palette: {
    mode: 'light',
    primary: { main: '#635bff', light: '#6c63d9', dark: '#4f46d8' },
    secondary: { main: '#875bf7' },
    success: { main: '#249d70' },
    warning: { main: '#c98522' },
    error: { main: '#d85160' },
    info: { main: '#3282c4' },
    background: { default: '#f5f6fa', paper: '#ffffff' },
    divider: '#e7eaf0',
    text: { primary: '#202534', secondary: '#778195' },
    action: { hover: 'rgba(38,43,67,0.05)', selected: 'rgba(99,91,255,0.09)' },
  },
  shape: { borderRadius: 8 },
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
          scrollbarColor: '#cdd2df #f5f6fa',
          '&::-webkit-scrollbar, & *::-webkit-scrollbar': { width: 9, height: 9 },
          '&::-webkit-scrollbar-thumb, & *::-webkit-scrollbar-thumb': {
            backgroundColor: '#cdd2df', borderRadius: 8, border: '2px solid transparent', backgroundClip: 'padding-box',
          },
          '&::-webkit-scrollbar-thumb:hover, & *::-webkit-scrollbar-thumb:hover': { backgroundColor: '#aeb6c7' },
          '&::-webkit-scrollbar-track, & *::-webkit-scrollbar-track': { backgroundColor: 'transparent' },
          '::selection': { backgroundColor: 'rgba(99,91,255,0.18)' },
        },
      },
    },
    MuiPaper: { defaultProps: { elevation: 0 }, styleOverrides: { root: { backgroundImage: 'none' } } },
    MuiCard: {
      styleOverrides: {
        root: { border: 'none', borderRadius: 8, backgroundImage: 'none', boxShadow: '0 1px 3px rgba(29,36,58,0.055)' },
      },
    },
    MuiButton: {
      defaultProps: { disableElevation: true },
      styleOverrides: {
        root: { borderRadius: 8, minHeight: 38, paddingInline: 16, transition: 'background-color .15s, box-shadow .15s, transform .15s' },
        sizeSmall: { minHeight: 32, paddingInline: 12 },
        containedPrimary: {
          backgroundColor: '#635bff', color: '#ffffff', boxShadow: '0 3px 10px rgba(99,91,255,0.2)',
          '&:hover': { backgroundColor: '#544ce7', boxShadow: '0 5px 14px rgba(99,91,255,0.25)' },
        },
        outlined: { borderColor: '#e1e5ee', backgroundColor: '#ffffff', '&:hover': { borderColor: '#cfd4e2', backgroundColor: '#fafbfe' } },
        text: { color: '#5f687b' },
        textPrimary: { color: '#635bff' },
      },
    },
    MuiIconButton: { styleOverrides: { root: { borderRadius: 8, color: '#697386', '&:hover': { backgroundColor: 'rgba(38,43,67,0.06)' } } } },
    MuiChip: {
      styleOverrides: {
        root: { borderRadius: 6, fontWeight: 600 },
        sizeSmall: { height: 24 },
        outlined: { borderColor: 'transparent', backgroundColor: '#f0f2f7', color: '#626c7f' },
        filled: {
          '&.MuiChip-colorDefault': { backgroundColor: '#eef0f5', color: '#626c7f' },
          '&.MuiChip-colorPrimary': { backgroundColor: '#eceaff', color: '#574bcf' },
          '&.MuiChip-colorSecondary': { backgroundColor: '#f0e9fc', color: '#7544c5' },
          '&.MuiChip-colorSuccess': { backgroundColor: '#e4f6ee', color: '#187951' },
          '&.MuiChip-colorWarning': { backgroundColor: '#fff3df', color: '#9c6418' },
          '&.MuiChip-colorError': { backgroundColor: '#fdecef', color: '#bd3f50' },
          '&.MuiChip-colorInfo': { backgroundColor: '#e7f2fc', color: '#276a9f' },
        },
      },
    },
    MuiDialog: { styleOverrides: { paper: { borderRadius: 10, border: 'none', backgroundImage: 'none', boxShadow: '0 24px 80px rgba(27,32,52,0.2)' } } },
    MuiDialogTitle: { styleOverrides: { root: { fontWeight: 700, fontSize: 18 } } },
    MuiMenu: { styleOverrides: { paper: { borderRadius: 8, border: '1px solid #e8eaf1', boxShadow: '0 12px 36px rgba(29,36,58,0.12)' } } },
    MuiOutlinedInput: {
      styleOverrides: {
        root: {
          borderRadius: 8, backgroundColor: '#ffffff', transition: 'background-color .15s, box-shadow .15s',
          '&:hover': { backgroundColor: '#ffffff' },
          '&:hover .MuiOutlinedInput-notchedOutline': { borderColor: '#cbd2e1' },
          '&.Mui-focused': { backgroundColor: '#ffffff', boxShadow: '0 0 0 3px rgba(99,91,255,0.12)' },
        },
        notchedOutline: { borderColor: '#e1e5ee' },
      },
    },
    MuiInputLabel: { styleOverrides: { outlined: { color: '#7a8497', '&.Mui-focused': { color: '#635bff' } } } },
    MuiTable: {
      styleOverrides: {
        root: { backgroundColor: '#ffffff', borderRadius: 8, borderCollapse: 'separate', overflow: 'hidden', boxShadow: '0 1px 3px rgba(29,36,58,0.05)' },
      },
    },
    MuiTableCell: {
      styleOverrides: {
        root: { borderBottom: '1px solid #edf0f4', padding: '14px 18px' },
        sizeSmall: { padding: '12px 16px' },
        head: { color: '#7a8496', fontWeight: 650, fontSize: 12, backgroundColor: '#fafbfc', letterSpacing: '0.02em' },
      },
    },
    MuiTableRow: { styleOverrides: { root: { '&.MuiTableRow-hover:hover': { backgroundColor: '#fafbfe' } } } },
    MuiTabs: { styleOverrides: { indicator: { height: 3, borderRadius: 1.5 } } },
    MuiTab: { styleOverrides: { root: { textTransform: 'none', fontWeight: 650, minHeight: 44, color: '#7a8496', '&.Mui-selected': { color: '#635bff' } } } },
    MuiTooltip: { defaultProps: { arrow: true } },
    MuiAlert: { styleOverrides: { root: { borderRadius: 8, border: 'none' } } },
    MuiListItemButton: { styleOverrides: { root: { borderRadius: 8, margin: '3px 10px', width: 'auto' } } },
  },
})

// 泳道 / 状态强调色
export const LANE_ACCENT: Record<string, string> = {
  planning: '#8791a3', ready: '#3b82c4', work: '#635bff', review: '#8b5cc7',
  blocked: '#d85160', done: '#249d70', archived: '#8791a3',
}
export const STATUS_ACCENT: Record<string, string> = {
  planning: '#718096', ready: '#327db9', in_progress: '#635bff', review: '#8655bd',
  blocked: '#cf4959', done: '#218763', archived: '#718096', cancelled: '#718096',
}