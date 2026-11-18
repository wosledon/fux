import { useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Card, CardContent, Chip, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Alert, IconButton, Tooltip } from '@mui/material'
import AddLinkIcon from '@mui/icons-material/AddLink'
import DeleteIcon from '@mui/icons-material/Delete'
import CheckCircleIcon from '@mui/icons-material/CheckCircle'
import ContentCopyIcon from '@mui/icons-material/ContentCopy'
import { get, post, del, parseJson, type NodeInfo } from '../api'
import { DeleteButton } from '../ui'

export default function NodesPage() {
  const qc = useQueryClient()
  const { data: nodes = [] } = useQuery({ queryKey: ['nodes'], queryFn: () => get<NodeInfo[]>('/nodes'), refetchInterval: 8000 })
  const [pairOpen, setPairOpen] = useState(false)
  const [pair, setPair] = useState<{ code: string; joinCommand: string } | null>(null)
  const [err, setErr] = useState('')
  const invalidate = () => qc.invalidateQueries({ queryKey: ['nodes'] })

  const genCode = async () => {
    setErr('')
    try {
      const r = await post<{ code: string; joinCommand: string }>('/nodes/pairing-codes', { expiresHours: 24 })
      setPair(r)
    } catch (e: any) { setErr(e.message) }
  }

  return (
    <Box sx={{ p: { xs: 2, md: 3.5 }, overflowY: 'auto', height: '100%', width: '100%', maxWidth: 1500, mx: 'auto' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', mb: 2 }}>
        <Typography variant="h6" fontWeight={700} sx={{ flexGrow: 1 }}>节点</Typography>
        <Button variant="contained" startIcon={<AddLinkIcon />} onClick={() => { setPairOpen(true); setPair(null); genCode() }}>添加节点（配对码）</Button>
      </Box>
      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 2 }}>
        非集群：另一台电脑安装 fux-runtime，一行命令配对加入（出站 WS，无需开入站端口/SSH）。admin 批准后即可调度。
      </Typography>
      {err && <Alert severity="error" sx={{ mb: 2 }} onClose={() => setErr('')}>{err}</Alert>}
      <Box sx={{ display: 'grid', gap: 2, gridTemplateColumns: 'repeat(auto-fill, minmax(340px, 1fr))' }}>
        {nodes.map(n => {
          const caps = parseJson<{ os?: string; clis?: string[]; load?: number }>(n.capabilities, {})
          const online = n.connStatus === 'online'
          return (
            <Card key={n.id}>
              <CardContent>
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 1 }}>
                  <Typography fontWeight={700} sx={{ flexGrow: 1 }}>{n.name}</Typography>
                  {!n.approved && <Chip label="待批准" color="warning" />}
                  <Chip label={online ? '在线' : '离线'} color={online ? 'success' : 'default'} />
                  {n.mode === 'local' ? <Chip label="控制面本地" variant="outlined" /> : <Chip label="runtime" variant="outlined" />}
                </Box>
                <Typography variant="body2" color="text.secondary">OS：{caps.os || '—'} · 负载：{caps.load ?? 0}</Typography>
                <Box sx={{ display: 'flex', gap: 0.5, flexWrap: 'wrap', mt: 1 }}>
                  {(caps.clis || []).map(c => <Chip key={c} label={c} variant="outlined" />)}
                  {(caps.clis || []).length === 0 && <Typography variant="caption" color="text.secondary">未探测到 CLI</Typography>}
                </Box>
                <Typography variant="caption" color="text.secondary" display="block" sx={{ mt: 1 }}>
                  心跳：{n.lastHeartbeat?.replace('T', ' ').slice(0, 19) || '—'}
                </Typography>
                <Box sx={{ display: 'flex', gap: 1, mt: 1.5 }}>
                  {!n.approved && (
                    <Button size="small" variant="contained" color="success" startIcon={<CheckCircleIcon />}
                      onClick={async () => { await post(`/nodes/${n.id}/approve`); invalidate() }}>批准</Button>
                  )}
                  {n.mode !== 'local' && (
                    <DeleteButton title="删除节点" onDelete={() => del(`/nodes/${n.id}`)} />
                  )}
                </Box>
              </CardContent>
            </Card>
          )
        })}
      </Box>

      <Dialog open={pairOpen} onClose={() => setPairOpen(false)} fullWidth maxWidth="sm">
        <DialogTitle>添加节点</DialogTitle>
        <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
          <Typography variant="body2">在目标机器上安装 fux-runtime 后执行（配对码 24 小时有效，一次性）：</Typography>
          {pair && (
            <>
              <TextField label="一行命令" value={pair.joinCommand} fullWidth multiline InputProps={{ readOnly: true, endAdornment: <IconButton onClick={() => navigator.clipboard.writeText(pair.joinCommand)}><ContentCopyIcon fontSize="small" /></IconButton> }} />
              <TextField label="配对码" value={pair.code} fullWidth InputProps={{ readOnly: true, endAdornment: <IconButton onClick={() => navigator.clipboard.writeText(pair.code)}><ContentCopyIcon fontSize="small" /></IconButton> }} />
            </>
          )}
          <Alert severity="info">runtime 连接后处于「待批准」状态，在本页点击批准后开始接收任务。节点需自行安装并认证 CLI（codex/opencode/pi/copilot）与 git。</Alert>
        </DialogContent>
        <DialogActions><Button onClick={() => setPairOpen(false)}>关闭</Button></DialogActions>
      </Dialog>
    </Box>
  )
}