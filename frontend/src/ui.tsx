import { createContext, useCallback, useContext, useState, type ReactNode } from 'react'
import { Box, Typography, Button, Dialog, DialogTitle, DialogContent, DialogContentText, DialogActions, Snackbar, Alert, Chip, Avatar as MuiAvatar, IconButton, Tooltip } from '@mui/material'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import BoltIcon from '@mui/icons-material/Bolt'
import InboxIcon from '@mui/icons-material/Inbox'
import DeleteIcon from '@mui/icons-material/Delete'
import BuildOutlinedIcon from '@mui/icons-material/BuildOutlined'
import EditOutlinedIcon from '@mui/icons-material/EditOutlined'
import DescriptionOutlinedIcon from '@mui/icons-material/DescriptionOutlined'
import TerminalIcon from '@mui/icons-material/Terminal'
import SearchIcon from '@mui/icons-material/Search'
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
        width: 58, height: 58, borderRadius: 2, display: 'grid', placeItems: 'center', mx: 'auto', mb: 2,
        color: 'text.secondary', bgcolor: '#eef0f6', border: 'none',
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
    <MuiAvatar sx={{ width: size, height: size, fontSize: size * 0.42, fontWeight: 750, bgcolor: `hsl(${hue} 68% 91%)`, color: `hsl(${hue} 44% 38%)` }}>
      {n.slice(0, 1).toUpperCase()}
    </MuiAvatar>
  )
}

export function StatusChip({ status }: { status: TaskStatus }) {
  const c = STATUS_ACCENT[status] || '#718096'
  return (
    <Chip size="small" label={STATUS_LABEL[status] || status}
      sx={{ bgcolor: `${c}12`, color: c, border: 'none', fontWeight: 700, px: 0.5 }} />
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

// ---------- Markdown 渲染 ----------
export function Markdown({ text }: { text: string }) {
  return (
    <Box sx={{
      fontSize: 14, lineHeight: 1.75, wordBreak: 'break-word', color: 'text.primary',
      '& > :first-of-type': { mt: 0 },
      '& > :last-child': { mb: 0 },
      '& p': { my: 1 },
      '& h1, & h2, & h3, & h4': { fontWeight: 700, mt: 2, mb: 1, lineHeight: 1.4 },
      '& h1': { fontSize: 22 },
      '& h2': { fontSize: 18 },
      '& h3': { fontSize: 16 },
      '& h4': { fontSize: 14.5 },
      '& ul, & ol': { pl: 3, my: 1 },
      '& li': { my: 0.35 },
      '& li > p': { my: 0.25 },
      '& blockquote': { borderLeft: '3px solid', borderColor: 'divider', pl: 1.5, color: 'text.secondary', my: 1, ml: 0 },
      '& a': { color: 'primary.main', textDecoration: 'none', '&:hover': { textDecoration: 'underline' } },
      '& hr': { border: 0, borderTop: '1px solid', borderColor: 'divider', my: 2 },
      '& table': { borderCollapse: 'collapse', width: '100%', my: 1.5, fontSize: 13, display: 'block', overflowX: 'auto' },
      '& th, & td': { border: '1px solid', borderColor: 'divider', px: 1, py: 0.5, textAlign: 'left' },
      '& th': { bgcolor: '#f7f8fb', fontWeight: 700 },
      '& code': { px: 0.5, py: 0.1, borderRadius: 0.5, bgcolor: 'rgba(29,36,58,0.07)', fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', fontSize: '0.92em' },
      '& pre': { bgcolor: '#0f172a', color: '#e2e8f0', p: 1.5, borderRadius: 1, overflowX: 'auto', my: 1.25, lineHeight: 1.6 },
      '& pre code': { bgcolor: 'transparent', color: 'inherit', p: 0, fontSize: 12.5, fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace' },
    }}>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
    </Box>
  )
}

// ---------- 智能体工具调用展示 ----------
export interface ToolCallView { id: string; title: string; status: string; kind?: string; detail?: string }

export const TOOL_STATUS: Record<string, string> = {
  pending: '等待',
  in_progress: '执行中',
  running: '执行中',
  completed: '完成',
  failed: '失败',
}

export const TOOL_TITLE_ALIAS: Record<string, string> = {
  execute: '执行命令',
  shell: '执行命令',
  bash: '执行命令',
  read: '读取文件',
  edit: '编辑文件',
  write: '写入文件',
  patch: '编辑文件',
  search: '搜索',
  grep: '搜索',
  glob: '查找文件',
  list: '列目录',
  fetch: '请求数据',
  webfetch: '请求数据',
  think: '思考',
  other: '工具',
}

/** 从 rawInput 提取可读的工具操作摘要（命令 / 路径 / 搜索词等）。 */
export function toolDetail(raw: any): string | undefined {
  if (typeof raw === 'string') {
    const s = raw.trim()
    return s ? s.slice(0, 200) : undefined
  }
  if (!raw || typeof raw !== 'object') return undefined
  for (const key of ['command', 'path', 'filePath', 'pattern', 'query', 'description', 'url', 'name']) {
    const v = (raw as Record<string, unknown>)[key]
    if (typeof v === 'string' && v.trim()) return v.trim().slice(0, 200)
  }
  return undefined
}

export function ToolIcon({ kind }: { kind?: string }) {
  const sx = { fontSize: 14 }
  switch (kind) {
    case 'edit': case 'write': case 'patch': return <EditOutlinedIcon sx={sx} />
    case 'read': case 'list': return <DescriptionOutlinedIcon sx={sx} />
    case 'execute': case 'shell': case 'bash': return <TerminalIcon sx={sx} />
    case 'search': case 'grep': case 'glob': return <SearchIcon sx={sx} />
    default: return <BuildOutlinedIcon sx={sx} />
  }
}

export function ToolRow({ tool }: { tool: ToolCallView }) {
  const color = tool.status === 'completed' ? 'success.main' : tool.status === 'failed' ? 'error.main' : 'primary.main'
  const title = tool.title ? (TOOL_TITLE_ALIAS[tool.title] || tool.title) : '工具'
  const label = tool.detail ? `${title} · ${tool.detail}` : title
  return (
    <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75 }}>
      <Box sx={{ color: 'text.disabled', display: 'flex', flexShrink: 0 }}><ToolIcon kind={tool.kind} /></Box>
      <Typography variant="caption" sx={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
        {label}
      </Typography>
      <Box component="span" sx={{ width: 6, height: 6, borderRadius: '50%', bgcolor: color, flexShrink: 0 }} />
      <Typography variant="caption" sx={{ color, flexShrink: 0 }}>{TOOL_STATUS[tool.status] || tool.status}</Typography>
    </Box>
  )
}