import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Table, TableHead, TableRow, TableCell, TableBody, Chip, Dialog, DialogTitle, DialogContent, DialogActions, TextField, MenuItem, Alert, IconButton, Tooltip } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import DeleteIcon from '@mui/icons-material/Delete'
import NetworkCheckIcon from '@mui/icons-material/NetworkCheck'
import { get, post, del, type McpServer } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton } from '../ui'

export default function McpPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const qc = useQueryClient()
  const isAdmin = role === 'owner' || role === 'admin'
  const { data: mcps = [] } = useQuery({ queryKey: ['mcps', wsId], queryFn: () => get<McpServer[]>(`/workspaces/${wsId}/mcp-servers`) })
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [transport, setTransport] = useState('stdio')
  const [command, setCommand] = useState('')
  const [args, setArgs] = useState('')
  const [url, setUrl] = useState('')
  const [env, setEnv] = useState('')
  const [err, setErr] = useState('')
  const [probeMsg, setProbeMsg] = useState<Record<string, string>>({})
  const invalidate = () => qc.invalidateQueries({ queryKey: ['mcps', wsId] })

  const create = async () => {
    setErr('')
    try {
      await post(`/workspaces/${wsId}/mcp-servers`, {
        name, transport,
        command: command || undefined,
        args: args ? args.split(/\s+/).filter(Boolean) : [],
        url: url || undefined,
        env: env ? JSON.parse(env) : {},
      })
      invalidate(); setOpen(false)
      setName(''); setCommand(''); setArgs(''); setUrl(''); setEnv('')
    } catch (e: any) { setErr(e.message) }
  }
  const probe = async (id: string) => {
    const r = await post<{ ok: boolean; message: string }>(`/mcp-servers/${id}/probe`)
    setProbeMsg(p => ({ ...p, [id]: r.message }))
    invalidate()
  }

  return (
    <Box sx={{ p: { xs: 2, md: 3.5 }, overflowY: 'auto', height: '100%', width: '100%', maxWidth: 1500, mx: 'auto' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', mb: 2 }}>
        <Typography variant="h6" fontWeight={700} sx={{ flexGrow: 1 }}>MCP 服务</Typography>
        {isAdmin && <Button variant="contained" startIcon={<AddIcon />} onClick={() => setOpen(true)}>注册服务</Button>}
      </Box>
      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 2 }}>
        会话创建时经 ACP session/new 透传给 CLI（按 CLI 能力适配：OpenCode 支持 stdio/HTTP，不支持 SSE）。密钥加密存储，不回传明文。
      </Typography>
      <Table size="small">
        <TableHead><TableRow><TableCell>名称</TableCell><TableCell>传输</TableCell><TableCell>命令 / URL</TableCell><TableCell>状态</TableCell><TableCell /></TableRow></TableHead>
        <TableBody>
          {mcps.map(m => (
            <TableRow key={m.id} hover>
              <TableCell>{m.name}</TableCell>
              <TableCell><Chip label={m.transport} variant="outlined" /></TableCell>
              <TableCell><code>{m.command || m.url}</code></TableCell>
              <TableCell>
                <Chip label={m.status} color={m.status === 'ok' ? 'success' : m.status === 'unavailable' ? 'error' : 'default'} />
                {probeMsg[m.id] && <Typography variant="caption" display="block" color="text.secondary">{probeMsg[m.id]}</Typography>}
              </TableCell>
              <TableCell align="right">
                <Tooltip title="健康检查"><IconButton size="small" onClick={() => probe(m.id)}><NetworkCheckIcon fontSize="small" /></IconButton></Tooltip>
                {isAdmin && <DeleteButton title="删除服务" onDelete={() => del(`/mcp-servers/${m.id}`)} />}
              </TableCell>
            </TableRow>
          ))}
          {mcps.length === 0 && <TableRow><TableCell colSpan={5}><Typography color="text.secondary">暂无 MCP 服务</Typography></TableCell></TableRow>}
        </TableBody>
      </Table>

      <Dialog open={open} onClose={() => setOpen(false)} fullWidth maxWidth="xs">
        <DialogTitle>注册 MCP 服务</DialogTitle>
        <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
          <TextField label="名称 *" value={name} onChange={e => setName(e.target.value)} />
          <TextField select label="传输方式" value={transport} onChange={e => setTransport(e.target.value)}>
            <MenuItem value="stdio">stdio</MenuItem>
            <MenuItem value="http">Streamable HTTP</MenuItem>
            <MenuItem value="sse">SSE</MenuItem>
          </TextField>
          {transport === 'stdio' ? (
            <>
              <TextField label="命令 *" value={command} onChange={e => setCommand(e.target.value)} />
              <TextField label="参数（空格分隔）" value={args} onChange={e => setArgs(e.target.value)} />
            </>
          ) : (
            <TextField label="URL *" value={url} onChange={e => setUrl(e.target.value)} />
          )}
          <TextField label="env / headers（JSON，加密存储）" value={env} onChange={e => setEnv(e.target.value)} multiline minRows={2} placeholder='{"API_KEY":"***"}' />
          {err && <Alert severity="error">{err}</Alert>}
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setOpen(false)}>取消</Button>
          <Button variant="contained" onClick={create} disabled={!name.trim() || (transport === 'stdio' ? !command.trim() : !url.trim())}>注册</Button>
        </DialogActions>
      </Dialog>
    </Box>
  )
}