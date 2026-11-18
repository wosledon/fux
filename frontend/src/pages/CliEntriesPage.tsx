import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Table, TableHead, TableRow, TableCell, TableBody, Chip, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Alert, IconButton, Tooltip } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import DeleteIcon from '@mui/icons-material/Delete'
import NetworkCheckIcon from '@mui/icons-material/NetworkCheck'
import { get, post, del, type CliEntry } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton } from '../ui'

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
  const [probe, setProbe] = useState<Record<string, string>>({})

  const invalidate = () => qc.invalidateQueries({ queryKey: ['clis', wsId] })
  const doProbe = async (id: string) => {
    const r = await post<{ available: boolean; path?: string }>(`/cli-entries/${id}/probe`)
    setProbe(p => ({ ...p, [id]: r.available ? `可用：${r.path}` : '未安装 / 不在 PATH' }))
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
    <Box sx={{ p: 3, overflowY: 'auto', height: '100%' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', mb: 2 }}>
        <Typography variant="h6" fontWeight={700} sx={{ flexGrow: 1 }}>CLI 接入（ACP）</Typography>
        {isAdmin && <Button variant="contained" startIcon={<AddIcon />} onClick={() => setOpen(true)}>自定义条目</Button>}
      </Box>
      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 2 }}>
        Fux 作为 ACP Client（JSON-RPC over stdio）。默认条目：Codex（codex-acp）/ OpenCode（opencode acp）/ Pi（pi-acp）/ Copilot CLI（copilot --acp --stdio）。CLI 认证在节点本地自理。
      </Typography>
      <Table size="small">
        <TableHead><TableRow>
          <TableCell>名称</TableCell><TableCell>类型</TableCell><TableCell>命令</TableCell><TableCell>来源</TableCell><TableCell>探测（本机）</TableCell><TableCell />
        </TableRow></TableHead>
        <TableBody>
          {clis.map(c => (
            <TableRow key={c.id} hover>
              <TableCell>{c.name}</TableCell>
              <TableCell><Chip label={c.kind} variant="outlined" /></TableCell>
              <TableCell><code>{c.command} {parseArgs(c.args)}</code></TableCell>
              <TableCell>{c.isDefault ? '默认' : '自定义'}</TableCell>
              <TableCell>
                <Button size="small" startIcon={<NetworkCheckIcon />} onClick={() => doProbe(c.id)}>探测</Button>
                {probe[c.id] && <Typography variant="caption" display="block" color={probe[c.id].startsWith('可用') ? 'success.main' : 'error.main'}>{probe[c.id]}</Typography>}
              </TableCell>
              <TableCell align="right">
                {!c.isDefault && isAdmin && (
                  <DeleteButton title="删除条目" onDelete={() => del(`/cli-entries/${c.id}`)} />
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>

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