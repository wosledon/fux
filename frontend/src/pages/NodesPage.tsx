import { useState } from 'react'
import { Link as RouterLink, useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Card, CardContent, Chip, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Alert, IconButton, Tooltip, Paper, CircularProgress, Link } from '@mui/material'
import AddLinkIcon from '@mui/icons-material/AddLink'
import CheckCircleIcon from '@mui/icons-material/CheckCircle'
import ContentCopyIcon from '@mui/icons-material/ContentCopy'
import RefreshIcon from '@mui/icons-material/Refresh'
import ComputerIcon from '@mui/icons-material/Computer'
import DnsIcon from '@mui/icons-material/Dns'
import PlayCircleOutlineIcon from '@mui/icons-material/PlayCircleOutline'
import { get, post, del, parseJson, type NodeInfo } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton, EmptyState, PageHeader, useToast } from '../ui'

type Pairing = { code: string; joinCommand: string }

export default function NodesPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const qc = useQueryClient()
  const toast = useToast()
  const isAdmin = role === 'owner' || role === 'admin'
  const { data: nodes = [], isRefetching, refetch, error: loadError } = useQuery({ queryKey: ['nodes'], queryFn: () => get<NodeInfo[]>('/nodes'), refetchInterval: 8000 })
  const [pairOpen, setPairOpen] = useState(false)
  const [pair, setPair] = useState<Pairing | null>(null)
  const [pairLoading, setPairLoading] = useState(false)
  const [pairErr, setPairErr] = useState('')
  const [err, setErr] = useState('')
  const [actionBusy, setActionBusy] = useState('')
  const invalidate = () => qc.invalidateQueries({ queryKey: ['nodes'] })

  const genCode = async () => {
    setPairErr('')
    setPairLoading(true)
    try {
      const r = await post<Pairing>('/nodes/pairing-codes', { expiresHours: 24 })
      setPair(r)
    } catch (e: any) { setPairErr(e.message || '配对码生成失败') }
    finally { setPairLoading(false) }
  }
  const openPair = () => {
    setPairOpen(true)
    setPair(null)
    void genCode()
  }
  const approve = async (id: string) => {
    setErr('')
    setActionBusy(id)
    try {
      await post(`/nodes/${id}/approve`)
      invalidate()
      toast('节点已批准，可以接收任务', 'success')
    } catch (e: any) { setErr(e.message || '批准失败') }
    finally { setActionBusy('') }
  }
  const copy = async (text: string, label: string) => {
    try { await navigator.clipboard.writeText(text); toast(`${label}已复制`, 'success') }
    catch { toast('复制失败，请手动复制', 'error') }
  }

  const onlineCount = nodes.filter(n => n.connStatus === 'online').length
  const pendingCount = nodes.filter(n => !n.approved).length
  const runningCount = nodes.reduce((sum, n) => sum + (n.runningRuns?.length || 0), 0)

  return (
    <Box sx={{ p: { xs: 2, md: 3.5 }, overflowY: 'auto', height: '100%', width: '100%', maxWidth: 1500, mx: 'auto' }}>
      <PageHeader
        title="执行节点"
        desc="管理本地与远程 runtime 的连接、能力和任务调度状态。"
        actions={<Box sx={{ display: 'flex', gap: 1, flexWrap: 'wrap' }}>
          <Tooltip title="立即刷新节点状态"><span><Button variant="outlined" startIcon={isRefetching ? <CircularProgress size={14} /> : <RefreshIcon />} disabled={isRefetching} onClick={() => void refetch()}>刷新</Button></span></Tooltip>
          {isAdmin && <Button variant="contained" startIcon={<AddLinkIcon />} onClick={openPair}>添加节点</Button>}
        </Box>}
      />

      <Box sx={{ display: 'grid', gridTemplateColumns: { xs: 'repeat(2, minmax(0, 1fr))', md: 'repeat(4, minmax(0, 1fr))' }, gap: 1.5, mb: 2.5 }}>
        {[
          { label: '全部节点', value: nodes.length, color: 'text.primary' },
          { label: '当前在线', value: onlineCount, color: 'success.main' },
          { label: '运行中 Run', value: runningCount, color: 'primary.main' },
          { label: '等待批准', value: pendingCount, color: pendingCount > 0 ? 'warning.main' : 'text.primary' },
        ].map(item => (
          <Paper key={item.label} sx={{ p: 2, border: '1px solid', borderColor: 'divider', borderRadius: 2 }}>
            <Typography variant="caption" color="text.secondary">{item.label}</Typography>
            <Typography variant="h5" sx={{ mt: 0.5, color: item.color, fontVariantNumeric: 'tabular-nums' }}>{item.value}</Typography>
          </Paper>
        ))}
      </Box>

      <Alert severity="info" sx={{ mb: 2.5 }}>
        单控制面 + 轻量 runtime，非集群。远程节点通过出站 WebSocket 接入，无需 SSH 或入站端口；页面每 8 秒自动更新。
      </Alert>
      {loadError && <Alert severity="error" sx={{ mb: 2 }}>节点状态加载失败：{loadError.message}</Alert>}
      {err && <Alert severity="error" sx={{ mb: 2 }} onClose={() => setErr('')}>{err}</Alert>}

      {nodes.length === 0 ? (
        <Paper sx={{ border: '1px solid', borderColor: 'divider', borderRadius: 2 }}><EmptyState title="还没有执行节点" desc="添加一个远程 runtime，或在启动控制面后使用本地节点。" action={isAdmin && <Button variant="contained" startIcon={<AddLinkIcon />} onClick={openPair}>添加节点</Button>} /></Paper>
      ) : (
        <Box sx={{ display: 'grid', gap: 2, gridTemplateColumns: { xs: 'minmax(0, 1fr)', md: 'repeat(auto-fill, minmax(380px, 1fr))' }, alignItems: 'start' }}>
          {nodes.map(n => {
            const caps = parseJson<{ os?: string; clis?: string[]; load?: number; projectPaths?: string[] }>(n.capabilities, {})
            const online = n.connStatus === 'online'
            const approved = !!n.approved
            const local = n.mode === 'local'
            const statusText = !approved ? '等待管理员批准' : online ? '已连接，可接收任务' : '离线，暂不参与调度'
            return (
              <Card key={n.id} sx={{ borderRadius: 2, borderColor: !approved ? 'warning.main' : 'divider' }}>
                <CardContent sx={{ p: 2.5, '&:last-child': { pb: 2.5 } }}>
                  <Box sx={{ display: 'flex', alignItems: 'flex-start', gap: 1.25 }}>
                    <Box sx={{ width: 38, height: 38, borderRadius: 1.5, display: 'grid', placeItems: 'center', bgcolor: online ? 'action.selected' : 'action.hover', color: online ? 'primary.main' : 'text.secondary', flexShrink: 0 }}>
                      {local ? <ComputerIcon sx={{ fontSize: 20 }} /> : <DnsIcon sx={{ fontSize: 20 }} />}
                    </Box>
                    <Box sx={{ flexGrow: 1, minWidth: 0 }}>
                      <Typography variant="subtitle1" noWrap>{n.name}</Typography>
                      <Typography variant="caption" color="text.secondary" sx={{ fontFamily: 'monospace' }}>#{n.id.slice(0, 8)} · {local ? '控制面本地' : 'fux-runtime'}</Typography>
                    </Box>
                    <Chip size="small" label={online ? '在线' : '离线'} color={online ? 'success' : 'default'} />
                  </Box>

                  <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mt: 2, p: 1.25, borderRadius: 1.5, bgcolor: 'action.hover' }}>
                    <Box sx={{ width: 7, height: 7, borderRadius: '50%', bgcolor: !approved ? 'warning.main' : online ? 'success.main' : 'text.disabled', flexShrink: 0 }} />
                    <Typography variant="body2" fontWeight={650} sx={{ flexGrow: 1 }}>{statusText}</Typography>
                    {!approved && <Chip size="small" color="warning" label="待批准" />}
                  </Box>

                  <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: 1, my: 2 }}>
                    <NodeMetric label="操作系统" value={osLabel(caps.os)} />
                    <NodeMetric label="当前负载" value={`${caps.load ?? 0} Run`} />
                    <NodeMetric label="最近心跳" value={heartbeatText(n.lastHeartbeat, local)} />
                  </Box>

                  <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 0.75 }}>CLI 能力</Typography>
                  <Box sx={{ display: 'flex', gap: 0.5, flexWrap: 'wrap' }}>
                    {(caps.clis || []).map(c => <Chip key={c} size="small" label={c} variant="outlined" />)}
                    {(caps.clis || []).length === 0 && <Typography variant="caption" color="text.secondary">未探测到可用 CLI</Typography>}
                  </Box>
                  {n.cliAuthRequired && (
                    <Alert severity="warning" sx={{ mt: 1.5 }}>
                      <Typography variant="caption" fontWeight={650} display="block">需要在此节点完成 CLI 登录认证</Typography>
                      <Typography variant="caption" sx={{ display: 'block', mt: 0.25, wordBreak: 'break-word' }}>{n.cliAuthMessage || '最近一次运行返回了认证错误。'}</Typography>
                    </Alert>
                  )}

                  <Box sx={{ mt: 2 }}>
                    <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 0.75 }}>运行中任务（{n.runningRuns?.length || 0}）</Typography>
                    {(n.runningRuns || []).map(run => (
                      <Box key={run.runId} sx={{ display: 'flex', alignItems: 'center', gap: 0.75, py: 0.5 }}>
                        <PlayCircleOutlineIcon sx={{ fontSize: 16, color: 'primary.main', flexShrink: 0 }} />
                        <Link component={RouterLink} to={`/ws/${wsId}/tasks/${run.taskId}`} underline="hover" color="text.primary" variant="body2" sx={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flexGrow: 1 }}>{run.taskTitle}</Link>
                        {run.stageKey && <Chip size="small" variant="outlined" label={run.stageKey} sx={{ height: 20, fontSize: 10.5 }} />}
                      </Box>
                    ))}
                    {(n.runningRuns?.length || 0) === 0 && <Typography variant="caption" color="text.disabled">当前空闲</Typography>}
                  </Box>

                  {n.recentError && (
                    <Alert severity="error" sx={{ mt: 1.5 }}>
                      <Typography variant="caption" fontWeight={650} display="block">最近错误 · {stopReasonLabel(n.recentError.stopReason)}</Typography>
                      <Link component={RouterLink} to={`/ws/${wsId}/tasks/${n.recentError.taskId}`} underline="hover" color="inherit" variant="caption">{n.recentError.taskTitle}</Link>
                      <Typography variant="caption" color="text.secondary" display="block" sx={{ mt: 0.25 }}>{n.recentError.endedAt?.replace('T', ' ').slice(0, 19) || ''}</Typography>
                    </Alert>
                  )}

                  <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mt: 2, pt: 1.5, borderTop: '1px solid', borderColor: 'divider' }}>
                    <Typography variant="caption" color="text.disabled" sx={{ flexGrow: 1 }}>加入于 {n.createdAt?.replace('T', ' ').slice(0, 10)}</Typography>
                    {isAdmin && !approved && <Button size="small" variant="contained" color="success" startIcon={actionBusy === n.id ? <CircularProgress size={13} /> : <CheckCircleIcon />}
                      disabled={actionBusy === n.id} onClick={() => void approve(n.id)}>批准调度</Button>}
                    {isAdmin && !local && <DeleteButton title="删除节点" content={`确定删除「${n.name}」？其通道会立即断开，运行中的任务将由租约机制回收。`}
                      onDelete={async () => { await del(`/nodes/${n.id}`); invalidate() }} />}
                  </Box>
                </CardContent>
              </Card>
            )
          })}
        </Box>
      )}

      <Dialog open={pairOpen} onClose={() => setPairOpen(false)} fullWidth maxWidth="sm">
        <DialogTitle>添加执行节点</DialogTitle>
        <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
          <Box>
            <Typography variant="subtitle2">1. 准备目标机器</Typography>
            <Typography variant="body2" color="text.secondary" sx={{ mt: 0.5 }}>安装 fux-runtime、git 和所需 ACP CLI，并在目标机器本地完成 CLI 登录认证。</Typography>
          </Box>
          <Box>
            <Typography variant="subtitle2">2. 执行配对命令</Typography>
            <Typography variant="body2" color="text.secondary" sx={{ mt: 0.5, mb: 1.5 }}>runtime 会建立出站连接并上报 OS、CLI 与负载，无需开放入站端口。</Typography>
            {pairLoading && <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, py: 2 }}><CircularProgress size={18} /><Typography variant="body2" color="text.secondary">正在生成一次性配对码…</Typography></Box>}
            {pairErr && <Alert severity="error" action={<Button color="inherit" size="small" onClick={() => void genCode()}>重试</Button>}>{pairErr}</Alert>}
            {pair && <>
              <TextField label="一行命令" value={pair.joinCommand} fullWidth multiline minRows={2}
                slotProps={{ input: { readOnly: true, endAdornment: <Tooltip title="复制命令"><IconButton onClick={() => void copy(pair.joinCommand, '配对命令')}><ContentCopyIcon fontSize="small" /></IconButton></Tooltip>, sx: { fontFamily: 'monospace', fontSize: 13 } } }} />
              <TextField label="配对码" value={pair.code} fullWidth sx={{ mt: 1.5 }}
                slotProps={{ input: { readOnly: true, endAdornment: <Tooltip title="复制配对码"><IconButton onClick={() => void copy(pair.code, '配对码')}><ContentCopyIcon fontSize="small" /></IconButton></Tooltip> } }} />
            </>}
          </Box>
          <Alert severity="warning">配对码 24 小时内有效且只能使用一次。节点连接后仍处于待批准状态，返回本页点击「批准调度」后才会接收任务。</Alert>
        </DialogContent>
        <DialogActions><Button onClick={() => setPairOpen(false)}>完成</Button></DialogActions>
      </Dialog>
    </Box>
  )
}

function NodeMetric({ label, value }: { label: string; value: string }) {
  return (
    <Box sx={{ minWidth: 0 }}>
      <Typography variant="caption" color="text.secondary" display="block">{label}</Typography>
      <Tooltip title={value}><Typography variant="body2" fontWeight={650} noWrap sx={{ mt: 0.25 }}>{value}</Typography></Tooltip>
    </Box>
  )
}

function osLabel(os?: string) {
  return ({ windows: 'Windows', macos: 'macOS', linux: 'Linux' } as Record<string, string>)[os || ''] || os || '未知'
}

function heartbeatText(value?: string, local = false) {
  if (local) return '随控制面在线'
  if (!value) return '尚未上报'
  const seconds = Math.max(0, Math.floor((Date.now() - new Date(value).getTime()) / 1000))
  if (seconds < 60) return '刚刚'
  if (seconds < 3600) return `${Math.floor(seconds / 60)} 分钟前`
  return value.replace('T', ' ').slice(5, 16)
}

function stopReasonLabel(reason?: string) {
  return ({
    interrupted: '执行中断', lease_expired: '租约过期', auth_required: 'CLI 需要认证',
    cancelled: '已取消', end_turn: '执行结束', task_blocked: '任务阻塞',
  } as Record<string, string>)[reason || ''] || reason || '未知原因'
}