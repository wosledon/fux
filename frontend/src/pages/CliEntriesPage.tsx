import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Table, TableHead, TableRow, TableCell, TableBody, Chip, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Alert, Tooltip, CircularProgress, TableContainer } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import NetworkCheckIcon from '@mui/icons-material/NetworkCheck'
import CheckCircleOutlineIcon from '@mui/icons-material/CheckCircleOutline'
import ErrorOutlineIcon from '@mui/icons-material/ErrorOutline'
import { get, post, del, type CliEntry } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton } from '../ui'

type ProbeState = { state: 'probing' } | { state: 'ok'; path?: string } | { state: 'fail'; message: string }

export default function CliEntriesPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const qc = useQueryClient()
  const isAdmin = role === 'owner' || role === 'admin'
  const { data: clis = [] } = useQuery({ queryKey: ['clis', wsId], queryFn: () => get<CliEntry[]>(`/cli-entries?workspaceId=${wsId}`) })
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [command, setCommand] = useState('')
  const [args, setArgs] = useState('')
  const [env, setEnv] = useState('')
  const [err, setErr] = useState('')
  const [probe, setProbe] = useState<Record<string, ProbeState>>({})
  const [probingAll, setProbingAll] = useState(false)

  const invalidate = () => qc.invalidateQueries({ queryKey: ['clis', wsId] })
  const doProbe = async (id: string) => {
    setProbe(p => ({ ...p, [id]: { state: 'probing' } }))
    try {
      const r = await post<{ available: boolean; path?: string }>(`/cli-entries/${id}/probe`)
      setProbe(p => ({ ...p, [id]: r.available ? { state: 'ok', path: r.path } : { state: 'fail', message: '未安装或不在 PATH' } }))
    } catch (e: any) {
      setProbe(p => ({ ...p, [id]: { state: 'fail', message: e.message || '探测失败' } }))
    }
  }
  const probeAll = async () => {
    setProbingAll(true)
    await Promise.all(clis.map(c => doProbe(c.id)))
    setProbingAll(false)
  }
  const create = async () => {
    setErr('')
    try {
      await post('/cli-entries', {
        workspaceId: wsId, name, command,
        args: args ? args.split(/\s+/).filter(Boolean) : [],
        envTemplate: env ? JSON.parse(env) : {},
      })
      invalidate(); setOpen(false); setName(''); setCommand(''); setArgs(''); setEnv('')
    } catch (e: any) { setErr(e.message) }
  }

  return (
    <Box sx={{ p: { xs: 2, md: 3.5 }, overflowY: 'auto', height: '100%', width: '100%', maxWidth: 1500, mx: 'auto' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 1.5, flexWrap: 'wrap' }}>
        <Typography variant="h6" fontWeight={700}>CLI 接入（ACP）</Typography>
        <Chip size="small" variant="outlined" label={`${clis.length} 个条目`} />
        <Box sx={{ flexGrow: 1 }} />
        <Button variant="outlined" startIcon={probingAll ? <CircularProgress size={14} /> : <NetworkCheckIcon />}
          disabled={probingAll || clis.length === 0} onClick={() => void probeAll()}>全部探测</Button>
        {isAdmin && <Button variant="contained" startIcon={<AddIcon />} onClick={() => setOpen(true)}>自定义条目</Button>}
      </Box>
      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 2 }}>
        Fux 作为 ACP Client（JSON-RPC over stdio）。默认条目：Codex（codex-acp）/ OpenCode（opencode acp）/ Pi（pi-acp）/ Copilot CLI（copilot --acp --stdio）。探测只检查控制面所在机器的 PATH；远程节点上的 CLI 以节点上报能力为准，认证在节点本地自理。
      </Typography>
      <TableContainer sx={{ border: '1px solid', borderColor: 'divider', borderRadius: 1.25, bgcolor: 'background.paper', overflowX: 'auto' }}>
        <Table size="small" sx={{ tableLayout: 'fixed', minWidth: 880 }}>
          <TableHead><TableRow>
            <TableCell sx={{ width: '20%' }}>名称</TableCell>
            <TableCell sx={{ width: '10%' }}>类型</TableCell>
            <TableCell sx={{ width: '30%' }}>启动命令</TableCell>
            <TableCell sx={{ width: '9%' }}>来源</TableCell>
            <TableCell sx={{ width: '23%' }}>探测结果</TableCell>
            <TableCell sx={{ width: '8%' }} align="right">操作</TableCell>
          </TableRow></TableHead>
          <TableBody>
            {clis.map(c => {
              const p = probe[c.id]
              const cmd = [c.command, parseArgs(c.args)].filter(Boolean).join(' ')
              return (
                <TableRow key={c.id} hover>
                  <TableCell><Typography variant="body2" fontWeight={650} noWrap>{c.name}</Typography></TableCell>
                  <TableCell><Chip size="small" label={c.kind} variant="outlined" /></TableCell>
                  <TableCell>
                    <Tooltip title={cmd}>
                      <Box component="code" sx={{
                        display: 'block', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                        fontFamily: 'monospace', fontSize: 12.5, color: 'text.secondary',
                      }}>{cmd}</Box>
                    </Tooltip>
                  </TableCell>
                  <TableCell><Chip size="small" label={c.isDefault ? '默认' : '自定义'} variant="outlined" /></TableCell>
                  <TableCell>
                    {!p && <Button size="small" startIcon={<NetworkCheckIcon />} onClick={() => void doProbe(c.id)}>探测</Button>}
                    {p?.state === 'probing' && (
                      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, color: 'text.secondary' }}>
                        <CircularProgress size={14} /><Typography variant="caption">探测中…</Typography>
                      </Box>
                    )}
                    {p?.state === 'ok' && (
                      <Box sx={{ minWidth: 0 }}>
                        <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75 }}>
                          <Chip size="small" color="success" icon={<CheckCircleOutlineIcon />} label="可用" sx={{ height: 22 }} />
                          <Button size="small" sx={{ minWidth: 0 }} onClick={() => void doProbe(c.id)}>重试</Button>
                        </Box>
                        <Tooltip title={p.path || ''}>
                          <Typography variant="caption" color="text.secondary" noWrap sx={{ display: 'block', mt: 0.25 }}>{p.path || '命令可用'}</Typography>
                        </Tooltip>
                      </Box>
                    )}
                    {p?.state === 'fail' && (
                      <Box sx={{ minWidth: 0 }}>
                        <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75 }}>
                          <Chip size="small" color="error" icon={<ErrorOutlineIcon />} label="不可用" sx={{ height: 22 }} />
                          <Button size="small" sx={{ minWidth: 0 }} onClick={() => void doProbe(c.id)}>重试</Button>
                        </Box>
                        <Tooltip title={p.message}>
                          <Typography variant="caption" color="error.main" noWrap sx={{ display: 'block', mt: 0.25 }}>{p.message}</Typography>
                        </Tooltip>
                      </Box>
                    )}
                  </TableCell>
                  <TableCell align="right">
                    {!c.isDefault && isAdmin && (
                      <DeleteButton title="删除条目" onDelete={async () => { await del(`/cli-entries/${c.id}`); invalidate() }} />
                    )}
                  </TableCell>
                </TableRow>
              )
            })}
            {clis.length === 0 && (
              <TableRow><TableCell colSpan={6}><Typography color="text.secondary" sx={{ py: 2, textAlign: 'center' }}>暂无 CLI 条目</Typography></TableCell></TableRow>
            )}
          </TableBody>
        </Table>
      </TableContainer>

      <Dialog open={open} onClose={() => setOpen(false)} fullWidth maxWidth="xs">
        <DialogTitle>自定义 ACP CLI 条目</DialogTitle>
        <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
          <TextField label="名称 *" value={name} onChange={e => setName(e.target.value)} />
          <TextField label="命令 *" value={command} onChange={e => setCommand(e.target.value)} placeholder="任意 ACP 兼容 CLI" />
          <TextField label="参数（空格分隔）" value={args} onChange={e => setArgs(e.target.value)} placeholder="--acp --stdio" />
          <TextField label="环境变量（JSON，可空）" value={env} onChange={e => setEnv(e.target.value)} multiline minRows={2} placeholder='{"KEY":"value"}' />
          {err && <Alert severity="error">{err}</Alert>}
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setOpen(false)}>取消</Button>
          <Button variant="contained" onClick={create} disabled={!name.trim() || !command.trim()}>创建</Button>
        </DialogActions>
      </Dialog>
    </Box>
  )
}

function parseArgs(s: string): string {
  try { return (JSON.parse(s) as string[]).join(' ') } catch { return '' }
}