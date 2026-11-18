import { createContext, useCallback, useContext, useState, type ReactNode } from 'react'
import { Box, Typography, Button, Dialog, DialogTitle, DialogContent, DialogContentText, DialogActions, Snackbar, Alert, Chip, Avatar as MuiAvatar, IconButton, Tooltip } from '@mui/material'
import BoltIcon from '@mui/icons-material/Bolt'
import InboxIcon from '@mui/icons-material/Inbox'
import DeleteIcon from '@mui/icons-material/Delete'
import { STATUS_LABEL, type TaskStatus } from './api'
import { STATUS_ACCENT } from './theme'

export function Logo({ size = 30 }: { size?: number }) {
  return (
    <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
      <Box sx={{
        width: size, height: size, borderRadius: `${size / 3}px`, display: 'grid', placeItems: 'center',
        background: 'linear-gradient(135deg,#91a2ff 0%,#776de5 100%)',
        boxShadow: '0 6px 18px rgba(104,121,232,0.2)',
      }}>
        <BoltIcon sx={{ fontSize: size * 0.62, color: '#fff' }} />
      </Box>
      <Typography sx={{ fontWeight: 800, fontSize: size * 0.66, letterSpacing: '-0.035em' }}>Fux</Typography>
    </Box>
  )
}

export function PageHeader({ title, desc, actions }: { title: string; desc?: string; actions?: ReactNode }) {
  return (
    <Box sx={{ display: 'flex', alignItems: 'flex-start', gap: 2, mb: 3.5 }}>
      <Box sx={{ flexGrow: 1, minWidth: 0 }}>
        <Typography variant="h5">{title}</Typography>
        {desc && <Typography variant="body2" color="text.secondary" sx={{ mt: 0.75, maxWidth: 760 }}>{desc}</Typography>}
      </Box>
      {actions && <Box sx={{ display: 'flex', gap: 1, flexShrink: 0, pt: 0.25 }}>{actions}</Box>}
    </Box>
  )
}

export function EmptyState({ icon, title, desc, action }: { icon?: ReactNode; title: string; desc?: string; action?: ReactNode }) {
  return (
    <Box sx={{ textAlign: 'center', py: 7, px: 3 }}>
      <Box sx={{
        width: 58, height: 58, borderRadius: 4, display: 'grid', placeItems: 'center', mx: 'auto', mb: 2,
        color: 'text.secondary', bgcolor: 'rgba(148,163,184,0.06)', border: '1px solid', borderColor: 'divider',
        '& svg': { fontSize: 28 },
      }}>{icon || <InboxIcon />}</Box>
      <Typography variant="subtitle1" fontWeight={700}>{title}</Typography>
      {desc && <Typography variant="body2" color="text.secondary" sx={{ mt: 0.75, mb: 2.5, maxWidth: 420, mx: 'auto' }}>{desc}</Typography>}
      {action}
    </Box>
  )
}

const HUES = [210, 260, 320, 20, 150, 180, 45, 285]
export function PersonAvatar({ name, size = 26 }: { name?: string; size?: number }) {
  const n = name || '?'
  let h = 0
  for (const c of n) h = (h * 31 + c.charCodeAt(0)) % 997
  const hue = HUES[h % HUES.length]
  return (
    <MuiAvatar sx={{ width: size, height: size, fontSize: size * 0.42, fontWeight: 700, bgcolor: `hsl(${hue} 55% 42%)` }}>
      {n.slice(0, 1).toUpperCase()}
    </MuiAvatar>
  )
}

export function StatusChip({ status }: { status: TaskStatus }) {
  const c = STATUS_ACCENT[status] || '#8b93a7'
  return (
    <Chip size="small" label={STATUS_LABEL[status] || status}
      sx={{ bgcolor: `${c}22`, color: c, border: `1px solid ${c}55`, fontWeight: 700 }} />
  )
}

// ---------- 全局 Toast ----------
interface Toast { msg: string; severity: 'success' | 'error' | 'info' | 'warning' }
const ToastCtx = createContext<(msg: string, severity?: Toast['severity']) => void>(() => {})
export const useToast = () => useContext(ToastCtx)

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toast, setToast] = useState<Toast | null>(null)
  const push = useCallback((msg: string, severity: Toast['severity'] = 'info') => setToast({ msg, severity }), [])
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <Snackbar open={!!toast} autoHideDuration={4200} onClose={() => setToast(null)}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'center' }}>
        <Alert severity={toast?.severity || 'info'} onClose={() => setToast(null)} variant="filled" sx={{ boxShadow: 6 }}>
          {toast?.msg}
        </Alert>
      </Snackbar>
    </ToastCtx.Provider>
  )
}

// ---------- 全局确认 ----------
interface ConfirmOpts { title: string; content?: string; danger?: boolean; okText?: string }
const ConfirmCtx = createContext<(opts: ConfirmOpts) => Promise<boolean>>(async () => false)
export const useConfirm = () => useContext(ConfirmCtx)

export function ConfirmProvider({ children }: { children: ReactNode }) {
  const [opts, setOpts] = useState<ConfirmOpts | null>(null)
  const [resolver, setResolver] = useState<((v: boolean) => void) | null>(null)
  const confirm = useCallback((o: ConfirmOpts) => {
    setOpts(o)
    return new Promise<boolean>(res => setResolver(() => res))
  }, [])
  const close = (v: boolean) => { resolver?.(v); setOpts(null); setResolver(null) }
  return (
    <ConfirmCtx.Provider value={confirm}>
      {children}
      <Dialog open={!!opts} onClose={() => close(false)} maxWidth="xs" fullWidth>
        <DialogTitle>{opts?.title}</DialogTitle>
        {opts?.content && <DialogContent><DialogContentText>{opts.content}</DialogContentText></DialogContent>}
        <DialogActions sx={{ px: 3, pb: 2 }}>
          <Button onClick={() => close(false)}>取消</Button>
          <Button variant="contained" color={opts?.danger ? 'error' : 'primary'} onClick={() => close(true)}>
            {opts?.okText || '确定'}
          </Button>
        </DialogActions>
      </Dialog>
    </ConfirmCtx.Provider>
  )
}

// ---------- 带确认的删除按钮 ----------
export function DeleteButton({ onDelete, title = '删除', content }: { onDelete: () => Promise<any>; title?: string; content?: string }) {
  const confirm = useConfirm()
  const toast = useToast()
  return (
    <Tooltip title={title}>
      <IconButton size="small" onClick={async () => {
        if (await confirm({ title: `确认${title}？`, content: content || '此操作不可撤销。', danger: true, okText: title })) {
          try { await onDelete(); toast(`${title}成功`, 'success') }
          catch (e: any) { toast(e.message, 'error') }
        }
      }}>
        <DeleteIcon fontSize="small" />
      </IconButton>
    </Tooltip>
  )
}