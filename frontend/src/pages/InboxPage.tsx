import { useMemo, useState } from 'react'
import { useNavigate, useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Button, Chip, IconButton, Paper, Tab, Tabs, Tooltip, Typography } from '@mui/material'
import InboxIcon from '@mui/icons-material/Inbox'
import RateReviewIcon from '@mui/icons-material/RateReview'
import ErrorOutlineIcon from '@mui/icons-material/ErrorOutline'
import MergeIcon from '@mui/icons-material/Merge'
import ArchiveIcon from '@mui/icons-material/Archive'
import NotificationsNoneIcon from '@mui/icons-material/NotificationsNone'
import ArrowForwardIcon from '@mui/icons-material/ArrowForward'
import DoneAllIcon from '@mui/icons-material/DoneAll'
import RefreshIcon from '@mui/icons-material/Refresh'
import { get, post, type NotificationItem } from '../api'
import { EmptyState, PageHeader, useToast } from '../ui'
import type { WsCtx } from './WorkspaceLayout'

const KIND_META: Record<string, { label: string; color: string; icon: React.ReactNode }> = {
  review: { label: '待审核', color: '#875bf7', icon: <RateReviewIcon /> },
  blocked: { label: '任务阻塞', color: '#d85160', icon: <ErrorOutlineIcon /> },
  merged: { label: '合并完成', color: '#249d70', icon: <MergeIcon /> },
  archived: { label: '已归档', color: '#778195', icon: <ArchiveIcon /> },
}

function formatTime(value: string) {
  if (!value) return ''
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value.replace('T', ' ').slice(0, 16)
  return new Intl.DateTimeFormat('zh-CN', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(date)
}

export default function InboxPage() {
  const { wsId } = useOutletContext<WsCtx>()
  const nav = useNavigate()
  const qc = useQueryClient()
  const toast = useToast()
  const [filter, setFilter] = useState<'all' | 'unread'>('all')
  const [busy, setBusy] = useState(false)
  const { data: items = [], isLoading, refetch } = useQuery({
    queryKey: ['notifications', wsId],
    queryFn: () => get<NotificationItem[]>(`/workspaces/${wsId}/notifications`),
    refetchInterval: 15000,
  })
  const unread = items.filter(i => !i.read).length
  const filtered = useMemo(() => filter === 'unread' ? items.filter(i => !i.read) : items, [filter, items])

  const invalidate = () => qc.invalidateQueries({ queryKey: ['notifications', wsId] })
  const markRead = async (id: string) => {
    try { await post(`/notifications/${id}/read`); await invalidate() }
    catch (e: any) { toast(e.message, 'error') }
  }
  const openItem = async (item: NotificationItem) => {
    if (!item.read) await markRead(item.notification.id)
    if (item.notification.taskId) nav(`/ws/${wsId}/tasks/${item.notification.taskId}`)
  }
  const markAll = async () => {
    setBusy(true)
    try {
      await Promise.all(items.filter(i => !i.read).map(i => post(`/notifications/${i.notification.id}/read`)))
      await invalidate()
      toast('所有消息均已标为已读', 'success')
    } catch (e: any) { toast(e.message, 'error') } finally { setBusy(false) }
  }

  return (
    <Box sx={{ height: '100%', overflowY: 'auto', p: { xs: 2, md: 3.5 } }}>
      <Box sx={{ maxWidth: 1040, mx: 'auto' }}>
        <PageHeader title="收件箱" desc="集中查看任务交付、阻塞与审核动态，点击消息即可进入对应任务。"
          actions={<>
            <Tooltip title="刷新"><IconButton onClick={() => void refetch()}><RefreshIcon /></IconButton></Tooltip>
            <Button variant="outlined" startIcon={<DoneAllIcon />} disabled={!unread || busy} onClick={() => void markAll()}>全部已读</Button>
          </>} />

        <Box sx={{ display: 'flex', alignItems: 'center', mb: 2.5, gap: 2 }}>
          <Tabs value={filter} onChange={(_, v) => setFilter(v)} sx={{
            minHeight: 42, p: 0.5, bgcolor: 'action.hover', borderRadius: 1.5,
            '& .MuiTabs-indicator': { height: '100%', borderRadius: 1.25, bgcolor: 'background.paper', boxShadow: '0 1px 4px rgba(24,34,48,0.1)' },
            '& .MuiTab-root': { zIndex: 1 },
          }}>
            <Tab value="all" label={`全部消息 ${items.length}`} disableRipple sx={{ minHeight: 34, py: 0, px: 2, '&.Mui-selected': { color: 'text.primary' } }} />
            <Tab value="unread" label={`未读 ${unread}`} disableRipple sx={{ minHeight: 34, py: 0, px: 2, '&.Mui-selected': { color: 'text.primary' } }} />
          </Tabs>
          <Box sx={{ flexGrow: 1 }} />
          {unread > 0 && <Chip size="small" color="primary" label={`${unread} 条待处理`} />}
        </Box>

        <Paper sx={{ borderRadius: 1.25, overflow: 'hidden', boxShadow: '0 1px 4px rgba(29,36,58,0.05)' }}>
          {isLoading ? (
            <Box sx={{ py: 8, display: 'grid', placeItems: 'center' }}><InboxIcon color="disabled" sx={{ fontSize: 36 }} /></Box>
          ) : filtered.length === 0 ? (
            <EmptyState icon={<InboxIcon />} title={filter === 'unread' ? '没有未读消息' : '收件箱还是空的'}
              desc={filter === 'unread' ? '所有消息都已处理，可以切换到全部消息查看历史。' : '任务需要审核、发生阻塞或完成交付时，消息会出现在这里。'} />
          ) : filtered.map((item, index) => {
            const n = item.notification
            const meta = KIND_META[n.kind] || { label: '任务动态', color: '#635bff', icon: <NotificationsNoneIcon /> }
            return (
              <Box key={n.id} onClick={() => n.taskId && void openItem(item)}
                onKeyDown={e => { if (n.taskId && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); void openItem(item) } }}
                role={n.taskId ? 'button' : undefined} tabIndex={n.taskId ? 0 : undefined}
                sx={{
                  display: 'flex', alignItems: 'flex-start', gap: 1.75, p: 2.25, position: 'relative',
                  cursor: n.taskId ? 'pointer' : 'default', bgcolor: item.read ? 'transparent' : '#f6f4ff',
                  borderBottom: index < filtered.length - 1 ? '1px solid' : 0, borderColor: 'divider',
                  transition: 'background-color .15s', '&:hover': { bgcolor: n.taskId ? 'action.hover' : undefined },
                  '&:focus-visible': { outline: '2px solid', outlineColor: 'primary.main', outlineOffset: -2 },
                }}>
                <Box sx={{ position: 'relative', flexShrink: 0 }}>
                  <Box sx={{ width: 40, height: 40, borderRadius: 1.25, display: 'grid', placeItems: 'center', bgcolor: `${meta.color}14`, color: meta.color, '& svg': { fontSize: 21 } }}>{meta.icon}</Box>
                  {!item.read && <Box sx={{ position: 'absolute', width: 8, height: 8, borderRadius: '50%', bgcolor: 'primary.main', top: -2, right: -2, boxShadow: '0 0 0 3px #ffffff' }} />}
                </Box>
                <Box sx={{ flexGrow: 1, minWidth: 0, pt: 0.25 }}>
                  <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 0.75 }}>
                    <Typography variant="caption" fontWeight={700} sx={{ color: meta.color }}>{meta.label}</Typography>
                    <Typography variant="caption" color="text.secondary">{formatTime(n.createdAt)}</Typography>
                  </Box>
                  <Typography variant="body2" sx={{ color: item.read ? 'text.secondary' : 'text.primary', fontWeight: item.read ? 400 : 550 }}>{n.message}</Typography>
                </Box>
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75, flexShrink: 0, alignSelf: 'center' }}>
                  {!item.read && <Button size="small" onClick={e => { e.stopPropagation(); void markRead(n.id) }}>标为已读</Button>}
                  {n.taskId && <Button size="small" endIcon={<ArrowForwardIcon />} onClick={e => { e.stopPropagation(); void openItem(item) }}>查看任务</Button>}
                </Box>
              </Box>
            )
          })}
        </Paper>
      </Box>
    </Box>
  )
}
