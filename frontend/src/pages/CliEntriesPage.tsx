import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Table, TableHead, TableRow, TableCell, TableBody, Chip, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Alert, Tooltip, CircularProgress, TableContainer, Paper } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import NetworkCheckIcon from '@mui/icons-material/NetworkCheck'
import CheckCircleOutlineIcon from '@mui/icons-material/CheckCircleOutline'
import ErrorOutlineIcon from '@mui/icons-material/ErrorOutline'
import TerminalIcon from '@mui/icons-material/Terminal'
import AutoAwesomeIcon from '@mui/icons-material/AutoAwesome'
import { alpha } from '@mui/material/styles'
import { get, post, del, parseJson, type CliEntry, type CliDetectResult } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton, EmptyState, PageHeader, useToast } from '../ui'

type ProbeState = { state: 'probing' } | { state: 'ok'; path?: string } | { state: 'fail'; message: string }

export default function CliEntriesPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const qc = useQueryClient()
  const toast = useToast()
  const isAdmin = role === 'owner' || role === 'admin'
  const { data: clis = [] } = useQuery({ queryKey: ['clis', wsId], queryFn: () => get<CliEntry[]>(`/cli-entries?workspaceId=${wsId}`) })
  const { data: detection, isFetching: detecting, error: detectError, refetch } = useQuery({
    queryKey: ['cli-detection', wsId],
    queryFn: () => get<CliDetectResult>(`/cli-entries/detect?workspaceId=${wsId}`),
    refetchOnWindowFocus: false,
  })
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [command, setCommand] = useState('')
  const [args, setArgs] = useState('')
  const [env, setEnv] = useState('')
  const [err, setErr] = useState('')
  const [creating, setCreating] = useState(false)
  const [probe, setProbe] = useState<Record<string, ProbeState>>({})

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['clis', wsId] })
    qc.invalidateQueries({ queryKey: ['cli-detection', wsId] })
  }
  const doProbe = async (id: string) => {
    setProbe(p => ({ ...p, [id]: { state: 'probing' } }))
    try {
      const r = await post<{ available: boolean; path?: string }>(`/cli-entries/${id}/probe`)
      setProbe(p => ({ ...p, [id]: r.available ? { state: 'ok', path: r.path } : { state: 'fail', message: '未在 PATH 中找到命令' } }))
    } catch (e: any) {
      setProbe(p => ({ ...p, [id]: { state: 'fail', message: e.message || '探测失败' } }))
    }
  }
  const detectAll = async () => {
    setProbe({})
    const result = await refetch()
    if (result.data) toast(`探测完成，${result.data.availableCount}/${result.data.total} 个 CLI 可用`, 'success')
  }
  const probeFor = (id: string): ProbeState | undefined => {
    if (probe[id]) return probe[id]
    const result = detection?.results.find(r => r.entryId === id)
    if (result) return result.available
      ? { state: 'ok', path: result.path || undefined }
      : { state: 'fail', message: '未安装或不在控制面 PATH 中' }
    return detecting ? { state: 'probing' } : undefined
  }
  const availableCount = clis.filter(c => probeFor(c.id)?.state === 'ok').length
  const create = async () => {
    setErr('')
    setCreating(true)
    try {
      const envTemplate = env.trim() ? JSON.parse(env) : {}
      if (!envTemplate || typeof envTemplate !== 'object' || Array.isArray(envTemplate)) throw new Error('环境变量必须是 JSON 对象')
      await post('/cli-entries', {
        workspaceId: wsId, name: name.trim(), command: command.trim(),
        args: args ? args.split(/\s+/).filter(Boolean) : [],
        envTemplate,
      })
      invalidate()
      toast('CLI 条目已创建，正在自动探测', 'success')
      setOpen(false); setName(''); setCommand(''); setArgs(''); setEnv('')
    } catch (e: any) { setErr(e.message || '创建失败') }
    finally { setCreating(false) }
  }

  return (
    <Box sx={{ p: { xs: 2, md: 3.5 }, overflowY: 'auto', height: '100%', width: '100%', maxWidth: 1400, mx: 'auto' }}>
      <PageHeader
        title="CLI 接入"
        desc="管理 Fux 的 ACP 命令行运行时，自动识别控制面当前可用的 CLI。"
        actions={isAdmin && <Button variant="contained" startIcon={<AddIcon />} onClick={() => { setErr(''); setOpen(true) }}>接入自定义 CLI</Button>}
      />

      <Paper sx={{
        p: { xs: 2.25, md: 3 }, mb: 2.5, border: '1px solid', borderColor: 'divider', borderRadius: 2,
        background: theme => `linear-gradient(120deg, ${alpha(theme.palette.primary.main, theme.palette.mode === 'dark' ? 0.12 : 0.055)}, ${theme.palette.background.paper} 58%)`,
      }}>
        <Box sx={{ display: 'flex', alignItems: { xs: 'stretch', sm: 'center' }, flexDirection: { xs: 'column', sm: 'row' }, gap: 2 }}>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.75, flexGrow: 1, minWidth: 0 }}>
            <Box sx={{ width: 46, height: 46, borderRadius: 2, display: 'grid', placeItems: 'center', bgcolor: 'primary.main', color: 'primary.contrastText', flexShrink: 0 }}>
              <AutoAwesomeIcon />
            </Box>
            <Box>
              <Typography variant="subtitle1">自动探测本机 CLI</Typography>
              <Typography variant="body2" color="text.secondary" sx={{ mt: 0.25 }}>
                扫描内置与自定义条目的启动命令，无需逐条手动检查。
              </Typography>
            </Box>
          </Box>
          <Button variant="outlined" startIcon={detecting ? <CircularProgress size={15} /> : <NetworkCheckIcon />}
            disabled={detecting} onClick={() => void detectAll()} sx={{ alignSelf: { xs: 'flex-start', sm: 'center' } }}>
            {detecting ? '正在探测…' : '重新探测'}
          </Button>
        </Box>
        <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 1.25, mt: 2.5 }}>
          {[
            { label: '已配置', value: clis.length },
            { label: '当前可用', value: detection ? availableCount : '—' },
            { label: '自定义', value: clis.filter(c => !c.isDefault).length },
          ].map(item => (
            <Box key={item.label} sx={{ px: 1.5, py: 1.25, borderRadius: 1.5, bgcolor: 'action.hover' }}>
              <Typography variant="caption" color="text.secondary">{item.label}</Typography>
              <Typography variant="h6" sx={{ mt: 0.25, fontVariantNumeric: 'tabular-nums' }}>{item.value}</Typography>
            </Box>
          ))}
        </Box>
        <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 1.5 }}>
          探测位置：控制面本机 PATH{detection?.detectedAt ? ` · 上次扫描 ${detection.detectedAt.replace('T', ' ').slice(5, 19)}` : ' · 首次扫描将在页面打开后自动执行'}
        </Typography>
      </Paper>

      {detectError && <Alert severity="error" sx={{ mb: 2 }} action={<Button color="inherit" size="small" onClick={() => void refetch()}>重试</Button>}>
        自动探测失败：{detectError.message}
      </Alert>}
      <Alert severity="info" sx={{ mb: 2.5 }}>
        这里仅检查控制面所在机器。远程节点的 CLI 能力由节点上线时上报，账号认证请在对应机器本地完成。
      </Alert>

      <Paper sx={{ border: '1px solid', borderColor: 'divider', borderRadius: 2, overflow: 'hidden' }}>
        <Box sx={{ px: 2.5, py: 2, display: 'flex', alignItems: 'center', gap: 1.5, borderBottom: '1px solid', borderColor: 'divider' }}>
          <Box sx={{ flexGrow: 1 }}>
            <Typography variant="subtitle2">ACP CLI 清单</Typography>
            <Typography variant="caption" color="text.secondary">智能体创建与模型探测将使用这里配置的启动命令</Typography>
          </Box>
          <Chip size="small" color={availableCount > 0 ? 'success' : 'default'} label={`${availableCount} 个可用`} />
        </Box>
        <TableContainer sx={{ overflowX: 'auto' }}>
          <Table size="small" sx={{ tableLayout: 'fixed', minWidth: 800, border: 0 }}>
            <TableHead><TableRow>
              <TableCell sx={{ width: '25%' }}>CLI</TableCell>
              <TableCell sx={{ width: '31%' }}>启动命令</TableCell>
              <TableCell sx={{ width: '11%' }}>来源</TableCell>
              <TableCell sx={{ width: '25%' }}>控制面状态</TableCell>
              <TableCell sx={{ width: '8%' }} align="right">操作</TableCell>
            </TableRow></TableHead>
            <TableBody>
              {clis.map(c => {
                const cmd = [c.command, parseJson<string[]>(c.args, []).join(' ')].filter(Boolean).join(' ')
                return (
                  <TableRow key={c.id} hover>
                    <TableCell>
                      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.25, minWidth: 0 }}>
                        <Box sx={{ width: 32, height: 32, borderRadius: 1, display: 'grid', placeItems: 'center', bgcolor: 'action.selected', color: 'primary.main', flexShrink: 0 }}>
                          <TerminalIcon sx={{ fontSize: 17 }} />
                        </Box>
                        <Box sx={{ minWidth: 0 }}>
                          <Typography variant="body2" fontWeight={650} noWrap>{c.name}</Typography>
                          <Typography variant="caption" color="text.secondary" noWrap sx={{ display: 'block' }}>{c.kind}</Typography>
                        </Box>
                      </Box>
                    </TableCell>
                    <TableCell>
                      <Tooltip title={cmd}>
                        <Box component="code" sx={{ display: 'block', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontFamily: 'monospace', fontSize: 12.5, color: 'text.secondary' }}>{cmd}</Box>
                      </Tooltip>
                    </TableCell>
                    <TableCell><Chip size="small" label={c.isDefault ? '内置' : '自定义'} variant="outlined" /></TableCell>
                    <TableCell><ProbeStatus value={probeFor(c.id)} onRetry={() => void doProbe(c.id)} /></TableCell>
                    <TableCell align="right">
                      {!c.isDefault && isAdmin && <DeleteButton title="删除条目" content={`确定删除「${c.name}」？引用它的智能体需要先调整 CLI。`} onDelete={async () => { await del(`/cli-entries/${c.id}`); invalidate() }} />}
                    </TableCell>
                  </TableRow>
                )
              })}
              {clis.length === 0 && <TableRow><TableCell colSpan={5}><EmptyState title="还没有 CLI 条目" desc="接入一个 ACP 兼容 CLI 后，即可用于创建智能体。" /></TableCell></TableRow>}
            </TableBody>
          </Table>
        </TableContainer>
      </Paper>

      <Dialog open={open} onClose={() => { if (!creating) { setOpen(false); setErr('') } }} fullWidth maxWidth="sm">
        <DialogTitle>接入自定义 ACP CLI</DialogTitle>
        <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
          <Typography variant="body2" color="text.secondary">配置命令的启动方式。保存后会自动加入上方清单并探测可用性。</Typography>
          <Box sx={{ display: 'grid', gridTemplateColumns: { xs: '1fr', sm: '1fr 1fr' }, gap: 2 }}>
            <TextField autoFocus label="显示名称" required value={name} onChange={e => setName(e.target.value)} placeholder="例如：My Agent" />
            <TextField label="启动命令" required value={command} onChange={e => setCommand(e.target.value)} placeholder="命令或绝对路径" helperText="需位于控制面 PATH，或填写绝对路径" />
          </Box>
          <TextField label="启动参数" value={args} onChange={e => setArgs(e.target.value)} placeholder="--acp --stdio" helperText="多个参数使用空格分隔" />
          <TextField label="环境变量模板" value={env} onChange={e => setEnv(e.target.value)} multiline minRows={3} placeholder='{"KEY":"value"}' helperText="可选，需为 JSON 对象；值将传递给 CLI 子进程" />
          {err && <Alert severity="error">{err}</Alert>}
        </DialogContent>
        <DialogActions>
          <Button disabled={creating} onClick={() => { setOpen(false); setErr('') }}>取消</Button>
          <Button variant="contained" onClick={() => void create()} disabled={creating || !name.trim() || !command.trim()}
            startIcon={creating ? <CircularProgress size={15} /> : <AddIcon />}>{creating ? '正在接入…' : '创建并探测'}</Button>
        </DialogActions>
      </Dialog>
    </Box>
  )
}

function ProbeStatus({ value, onRetry }: { value?: ProbeState; onRetry: () => void }) {
  if (!value) return <Button size="small" startIcon={<NetworkCheckIcon />} onClick={onRetry}>探测</Button>
  if (value.state === 'probing') return (
    <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, color: 'text.secondary' }}>
      <CircularProgress size={14} /><Typography variant="caption">自动探测中…</Typography>
    </Box>
  )
  const ok = value.state === 'ok'
  const detail = ok ? value.path || '命令可用' : value.message
  return (
    <Box sx={{ minWidth: 0 }}>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75 }}>
        <Chip size="small" color={ok ? 'success' : 'default'} icon={ok ? <CheckCircleOutlineIcon /> : <ErrorOutlineIcon />}
          label={ok ? '可用' : '未检测到'} sx={{ height: 22 }} />
        <Button size="small" sx={{ minWidth: 0 }} onClick={onRetry}>重试</Button>
      </Box>
      <Tooltip title={detail}>
        <Typography variant="caption" color="text.secondary" noWrap sx={{ display: 'block', mt: 0.35 }}>{detail}</Typography>
      </Tooltip>
    </Box>
  )
}