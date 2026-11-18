import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useOutletContext, useParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Box, Typography, Chip, IconButton, Tabs, Tab, TextField, Button, Divider, Alert, Paper,
  Checkbox, FormControlLabel, Dialog, DialogTitle, DialogContent, DialogActions, Link, Tooltip, CircularProgress,
} from '@mui/material'
import ArrowBackIcon from '@mui/icons-material/ArrowBack'
import SendIcon from '@mui/icons-material/Send'
import OpenInNewIcon from '@mui/icons-material/OpenInNew'
import RefreshIcon from '@mui/icons-material/Refresh'
import SmartToyIcon from '@mui/icons-material/SmartToy'
import BuildIcon from '@mui/icons-material/Build'
import CheckCircleIcon from '@mui/icons-material/CheckCircle'
import CancelIcon from '@mui/icons-material/Cancel'
import PlayArrowIcon from '@mui/icons-material/PlayArrow'
import RateReviewIcon from '@mui/icons-material/RateReview'
import { get, post, taskEventsUrl, parseJson, STATUS_LABEL, type Task, type Run, type Project, type Envelope } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { EmptyState, PersonAvatar, StatusChip, useToast } from '../ui'

const fmtTime = (s: string) => s?.replace('T', ' ').slice(11, 19)

export default function TaskPage() {
  const { taskId = '' } = useParams()
  const nav = useNavigate()
  const { wsId, role } = useOutletContext<WsCtx>()
  const qc = useQueryClient()
  const toast = useToast()
  const [tab, setTab] = useState(0)
  const [events, setEvents] = useState<Envelope[]>([])
  const [comment, setComment] = useState('')
  const [busy, setBusy] = useState(false)
  const [manualConfirm, setManualConfirm] = useState(false)
  const [rejectOpen, setRejectOpen] = useState(false)
  const [rejectReason, setRejectReason] = useState('')
  const bottomRef = useRef<HTMLDivElement>(null)
  const isAdmin = role === 'owner' || role === 'admin'

  const { data, isError } = useQuery({ queryKey: ['task', taskId], queryFn: () => get<{ task: Task; runs: Run[]; project?: Project }>(`/tasks/${taskId}`) })
  const task = data?.task
  const { data: pr } = useQuery({ queryKey: ['pr', taskId], queryFn: () => get<any>(`/tasks/${taskId}/pull-request`), enabled: !!task })

  useEffect(() => {
    setEvents([])
    const es = new EventSource(taskEventsUrl(taskId, 0))
    es.onmessage = (ev) => {
      try {
        const env: Envelope = JSON.parse(ev.data)
        setEvents(prev => prev.some(e => e.seq === env.seq) ? prev : [...prev, env])
        if (String(env.kind).startsWith('task.') || env.kind.startsWith('pr.') || env.kind === 'comment.added') {
          qc.invalidateQueries({ queryKey: ['task', taskId] })
          qc.invalidateQueries({ queryKey: ['pr', taskId] })
          qc.invalidateQueries({ queryKey: ['tasks'] })
        }
      } catch { /* ignore */ }
    }
    return () => es.close()
  }, [taskId, qc])

  useEffect(() => { bottomRef.current?.scrollIntoView({ behavior: 'smooth' }) }, [events.length])

  const refresh = () => {
    qc.invalidateQueries({ queryKey: ['task', taskId] })
    qc.invalidateQueries({ queryKey: ['tasks'] })
    qc.invalidateQueries({ queryKey: ['pr', taskId] })
  }
  const action = async (fn: () => Promise<any>, okMsg?: string) => {
    setBusy(true)
    try { await fn(); refresh(); if (okMsg) toast(okMsg, 'success') }
    catch (e: any) { toast(e.message, 'error') } finally { setBusy(false) }
  }
  const sendComment = async () => {
    if (!comment.trim() || busy) return
    setBusy(true)
    try { await post(`/tasks/${taskId}/comments`, { text: comment.trim() }); setComment('') }
    catch (e: any) { toast(e.message, 'error') } finally { setBusy(false) }
  }

  const commentHint = task ? ({
    in_progress: '评论将注入当前运行会话，引导智能体工作',
    done: '评论将触发返工：任务回到待办并被重新认领',
    blocked: '评论视为对阻塞问题的解答，将自动解除阻塞',
    review: '评论作为评审意见；驳回请使用「驳回」按钮',
  } as Record<string, string>)[task.status] || '评论不会触发执行' : ''

  const items = useMemo(() => {
    const out: { kind: string; env?: Envelope; text?: string; runId?: string }[] = []
    for (const env of events) {
      if (env.kind === 'agent.message' || env.kind === 'agent.thought') {
        const last = out[out.length - 1]
        const text = env.payload?.text || ''
        if (last && last.kind === env.kind && last.runId === env.runId) { last.text = (last.text || '') + text }
        else out.push({ kind: env.kind, text, runId: env.runId })
      } else if (env.kind === 'usage.updated' || env.kind === 'notification') {
        continue
      } else {
        out.push({ kind: env.kind, env })
      }
    }
    return out
  }, [events])

  const resolvedApprovals = useMemo(() => {
    const s = new Set<string>()
    for (const e of events) if (e.kind === 'permission.resolved') s.add(e.payload?.approvalId)
    return s
  }, [events])
  const toolStatus = useMemo(() => {
    const m = new Map<string, string>()
    for (const e of events) if (e.kind === 'tool.result') m.set(e.payload?.toolCallId, e.payload?.status || 'completed')
    return m
  }, [events])

  if (!task) return (
    <Box sx={{ height: '100%', display: 'grid', placeItems: 'center', p: 3 }}>
      {isError
        ? <EmptyState title="无法打开任务" desc="任务可能已被删除，或你暂时没有访问权限。" action={<Button startIcon={<ArrowBackIcon />} onClick={() => nav(`/ws/${wsId}`)}>返回看板</Button>} />
        : <CircularProgress />}
    </Box>
  )

  return (
    <Box sx={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: { xs: 'auto', lg: 'hidden' } }}>
      <Box sx={{ px: { xs: 2, md: 3.5 }, pt: 3, pb: 2.5, flexShrink: 0 }}>
        <Box sx={{ maxWidth: 1500, mx: 'auto' }}>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 2 }}>
            <Tooltip title="返回上一页"><IconButton size="small" onClick={() => nav(-1)} sx={{ ml: -0.75 }}><ArrowBackIcon /></IconButton></Tooltip>
            <Typography variant="body2" color="text.secondary">任务详情</Typography>
            <Typography variant="body2" color="text.secondary">/</Typography>
            <Typography variant="body2" color="text.secondary" sx={{ fontFamily: 'monospace' }}>#{task.id.slice(0, 8)}</Typography>
            <Box sx={{ flexGrow: 1 }} />
            <Tooltip title="刷新任务"><IconButton size="small" onClick={refresh}><RefreshIcon /></IconButton></Tooltip>
          </Box>
          <Box sx={{ display: 'flex', gap: 2, alignItems: 'flex-start', flexWrap: 'wrap' }}>
            <Box sx={{ flexGrow: 1, minWidth: 260 }}>
              <Box sx={{ display: 'flex', gap: 0.75, mb: 1.25, flexWrap: 'wrap', alignItems: 'center' }}>
                <StatusChip status={task.status} />
                {task.prState && <Chip size="small" variant="outlined" color={task.prState === 'merged' ? 'success' : 'default'} label={`PR: ${task.prState}`} />}
                {task.branch && <Chip size="small" variant="outlined" label={task.branch} />}
                {task.retryCount > 0 && <Chip size="small" variant="outlined" label={`重试 ${task.retryCount}`} />}
              </Box>
              <Typography variant="h5" sx={{ wordBreak: 'break-word', lineHeight: 1.35 }}>{task.title}</Typography>
              <Typography variant="caption" color="text.secondary" display="block" sx={{ mt: 1 }}>创建于 {task.createdAt?.replace('T', ' ').slice(0, 16)} · 最近更新 {task.updatedAt?.replace('T', ' ').slice(0, 16)}</Typography>
            </Box>
            <Box sx={{ display: 'flex', gap: 1, flexWrap: 'wrap', alignItems: 'center', pt: 0.5 }}>
              {task.status === 'planning' && <Button variant="contained" onClick={() => action(() => post(`/tasks/${taskId}/transition`, { to: 'ready' }), '已移入待办，等待智能体认领')}>规划完成 → 待办</Button>}
              {task.status === 'blocked' && <Button variant="contained" color="warning" onClick={() => action(() => post(`/tasks/${taskId}/unblock`), '已解除阻塞')}>解除阻塞</Button>}
              {task.status === 'in_progress' && <Button variant="outlined" color="warning" onClick={() => action(() => post(`/tasks/${taskId}/cancel-run`), '已发送取消信号')}>取消运行</Button>}
              {task.status === 'review' && <Button variant="contained" color="success" startIcon={<CheckCircleIcon />}
                disabled={busy || (task.prState !== 'merged' && !manualConfirm)}
                onClick={() => action(() => post(`/tasks/${taskId}/transition`, { to: 'done', manualConfirm }), '已批准，任务完成')}>批准交付</Button>}
              {task.status === 'review' && <Button variant="outlined" color="error" startIcon={<CancelIcon />} onClick={() => setRejectOpen(true)}>驳回</Button>}
              {task.status === 'done' && <Button variant="outlined" onClick={() => action(() => post(`/tasks/${taskId}/archive`), '已归档')}>归档任务</Button>}
            </Box>
          </Box>
          {task.status === 'blocked' && task.blockReason && <Alert severity="error" sx={{ mt: 2 }}>{task.blockReason}</Alert>}
        </Box>
      </Box>

      <Box sx={{ flexGrow: 1, minHeight: 0, px: { xs: 2, md: 3.5 }, pb: 3.5, overflow: { xs: 'visible', lg: 'hidden' } }}>
        <Box sx={{
          maxWidth: 1500, mx: 'auto', height: { xs: 'auto', lg: '100%' }, display: 'grid', gap: 2.5,
          gridTemplateColumns: { xs: 'minmax(0,1fr)', lg: 'minmax(0,1fr) 340px' }, alignItems: 'stretch',
        }}>
          <Paper sx={{ minHeight: { xs: 560, lg: 0 }, display: 'flex', flexDirection: 'column', border: '1px solid', borderColor: 'divider', borderRadius: 4, overflow: 'hidden' }}>
            <Tabs value={tab} onChange={(_, v) => setTab(v)} sx={{ px: 2, borderBottom: '1px solid', borderColor: 'divider', bgcolor: 'rgba(148,163,184,0.02)', flexShrink: 0 }}>
              <Tab label={`活动 ${items.length}`} /><Tab label="任务说明" />
            </Tabs>

      {tab === 0 && (
        <>
          <Box sx={{ flexGrow: 1, minHeight: 0, overflowY: 'auto', p: { xs: 2, md: 3 }, display: 'flex', flexDirection: 'column', gap: 1.5 }}>
            {items.length === 0 && <EmptyState title="还没有任务动态" desc="智能体输出、工具调用、评论和状态变化会实时出现在这里。" />}
            {items.map((it, i) => {
              const env = it.env
              const prev = items[i - 1]
              const runHeader = it.runId && it.runId !== prev?.runId && ['agent.message', 'agent.thought', 'tool.call'].includes(it.kind)
              switch (it.kind) {
                case 'agent.message':
                  return (
                    <Box key={i}>
                      {runHeader && <RunDivider label={`智能体输出 · ${fmtTime(env?.createdAt || '')}`} />}
                      <Box sx={{ display: 'flex', gap: 1.25, alignItems: 'flex-start' }}>
                        <Box sx={{ width: 30, height: 30, borderRadius: 2.5, bgcolor: 'rgba(139,157,255,0.12)', display: 'grid', placeItems: 'center', flexShrink: 0, mt: 0.25 }}>
                          <SmartToyIcon sx={{ fontSize: 17, color: 'primary.light' }} />
                        </Box>
                        <Paper elevation={0} sx={{ p: 1.75, bgcolor: '#191f2b', border: '1px solid', borderColor: 'divider', borderRadius: 3, whiteSpace: 'pre-wrap', wordBreak: 'break-word', maxWidth: '92%' }}>
                          <Typography variant="body2">{it.text}</Typography>
                        </Paper>
                      </Box>
                    </Box>
                  )
                case 'agent.thought':
                  return <Typography key={i} variant="caption" color="text.secondary" sx={{ fontStyle: 'italic', whiteSpace: 'pre-wrap', pl: 4.5 }}>💭 {it.text}</Typography>
                case 'comment.added':
                  return (
                    <Box key={i} sx={{ display: 'flex', gap: 1.25, justifyContent: 'flex-end', alignItems: 'flex-start' }}>
                      <Paper elevation={0} sx={{ p: 1.5, bgcolor: 'rgba(109,141,255,0.12)', border: '1px solid rgba(109,141,255,0.3)', borderRadius: 2.5, maxWidth: '85%' }}>
                        <Typography variant="caption" sx={{ color: 'primary.light', display: 'block', mb: 0.25 }}>
                          {env?.payload?.username || env?.actor} · {fmtTime(env?.createdAt || '')}
                        </Typography>
                        <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap' }}>{env?.payload?.text}</Typography>
                      </Paper>
                      <PersonAvatar name={env?.payload?.username || 'U'} size={26} />
                    </Box>
                  )
                case 'task.moved':
                  return (
                    <Typography key={i} variant="caption" color="text.secondary" sx={{ textAlign: 'center' }}>
                      {STATUS_LABEL[env?.payload?.from as keyof typeof STATUS_LABEL] || env?.payload?.from} → {STATUS_LABEL[env?.payload?.to as keyof typeof STATUS_LABEL] || env?.payload?.to}
                      {env?.payload?.reason ? ` · ${env.payload.reason}` : ''}
                    </Typography>
                  )
                case 'task.created':
                  return <Typography key={i} variant="caption" color="text.secondary" sx={{ textAlign: 'center' }}>任务创建 · {fmtTime(env?.createdAt || '')}</Typography>
                case 'run.started':
                  return <RunDivider key={i} icon={<PlayArrowIcon sx={{ fontSize: 14 }} />} label={`Run 开始 · ${env?.payload?.agentName || ''} @ ${env?.payload?.nodeId === 'local' ? '本地节点' : '远程节点'}`} />
                case 'tool.call': {
                  const st = toolStatus.get(env?.payload?.toolCallId)
                  return (
                    <Box key={i} sx={{ display: 'flex', gap: 1, alignItems: 'center', pl: 4.5 }}>
                      <BuildIcon sx={{ fontSize: 14, color: 'text.secondary' }} />
                      <Typography variant="caption" sx={{ flexGrow: 1, color: 'text.secondary' }}>{env?.payload?.title || env?.payload?.kind || '工具调用'}</Typography>
                      <Chip size="small" label={st === 'completed' ? '完成' : st === 'failed' ? '失败' : st || '进行中'}
                        color={st === 'completed' ? 'success' : st === 'failed' ? 'error' : 'default'} variant="outlined" sx={{ height: 18, fontSize: 10 }} />
                    </Box>
                  )
                }
                case 'tool.result': return null
                case 'plan.updated':
                  return (
                    <Paper key={i} elevation={0} sx={{ p: 1.5, ml: 4.5, bgcolor: 'background.paper', border: '1px solid', borderColor: 'divider', borderRadius: 2 }}>
                      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 0.5 }}>📋 计划</Typography>
                      {(env?.payload?.entries || []).map((e: any, j: number) => (
                        <Typography key={j} variant="body2">{e.status === 'completed' ? '✅' : e.status === 'in_progress' ? '🔄' : '⬜'} {e.content}</Typography>
                      ))}
                    </Paper>
                  )
                case 'permission.requested': {
                  const aid = env?.payload?.approvalId
                  const resolved = resolvedApprovals.has(aid)
                  return (
                    <Alert key={i} severity="warning" sx={{ ml: 4.5 }} action={
                      !resolved && (
                        <Box sx={{ display: 'flex', gap: 0.5 }}>
                          <Button size="small" color="success" variant="contained" onClick={() => action(() => post(`/approvals/${aid}/decide`, { decision: 'approve' }), '已批准')}>批准</Button>
                          <Button size="small" color="error" variant="outlined" onClick={() => action(() => post(`/approvals/${aid}/decide`, { decision: 'reject' }), '已拒绝')}>拒绝</Button>
                        </Box>
                      )
                    }>
                      工具授权请求：{env?.payload?.request?.toolCall?.title || '未知操作'}{resolved ? '（已处理）' : ''}
                    </Alert>
                  )
                }
                case 'permission.resolved':
                  return <Typography key={i} variant="caption" color="text.secondary" sx={{ textAlign: 'center' }}>授权已{env?.payload?.decision === 'approve' ? '批准' : '拒绝'}</Typography>
                case 'watchdog.loop': case 'watchdog.stalled': case 'watchdog.hard_limit': case 'watchdog.no_progress': case 'watchdog.interrupted':
                  return <Alert key={i} severity="error">质量阀 [{it.kind.split('.')[1]}]：{env?.payload?.diagnostics || env?.payload?.error || '触发熔断'}</Alert>
                case 'pr.linked':
                  return (
                    <Paper key={i} elevation={0} sx={{ p: 1.5, bgcolor: 'background.paper', border: '1px solid', borderColor: 'divider', borderRadius: 2 }}>
                      <Typography variant="body2">🔗 PR/MR 已就绪（{env?.payload?.prState}）</Typography>
                      {env?.payload?.prUrl && <Link href={env.payload.prUrl} target="_blank" rel="noreferrer" variant="body2">{env.payload.prUrl} <OpenInNewIcon sx={{ fontSize: 12 }} /></Link>}
                    </Paper>
                  )
                case 'pr.merged_detected':
                  return <Alert key={i} severity="success">检测到 PR/MR 已合并，可以批准任务</Alert>
                case 'comment.injected':
                  return <Typography key={i} variant="caption" sx={{ textAlign: 'center', color: 'success.main' }}>引导评论已送达智能体</Typography>
                case 'team.handoff':
                  return <RunDivider key={i} label={`团队交接：阶段 ${env?.payload?.fromStage} → ${env?.payload?.toStage}`} />
                default:
                  return <Typography key={i} variant="caption" color="text.secondary">{it.kind} · {fmtTime(env?.createdAt || '')}</Typography>
              }
            })}
            <div ref={bottomRef} />
          </Box>
          <Box sx={{ p: 2, borderTop: '1px solid', borderColor: 'divider', bgcolor: 'rgba(148,163,184,0.02)', flexShrink: 0 }}>
            <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 1 }}>{commentHint}</Typography>
            <Box sx={{ display: 'flex', gap: 1, alignItems: 'flex-end' }}>
              <TextField fullWidth multiline maxRows={4} placeholder="输入评论，引导智能体或记录评审意见…" value={comment} onChange={e => setComment(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); void sendComment() } }} />
              <Button variant="contained" onClick={() => void sendComment()} disabled={busy || !comment.trim()} sx={{ minWidth: 44, px: 0, height: 40 }} aria-label="发送评论"><SendIcon fontSize="small" /></Button>
            </Box>
          </Box>
        </>
      )}

      {tab === 1 && (
        <Box sx={{ flexGrow: 1, minHeight: 0, overflowY: 'auto', p: { xs: 2.5, md: 3.5 } }}>
          <Typography variant="subtitle2" gutterBottom>任务内容</Typography>
          <Typography variant="body2" color={task.content ? 'text.primary' : 'text.secondary'} sx={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word', lineHeight: 1.85, mt: 2 }}>{task.content || '这个任务还没有填写详细内容。'}</Typography>
          <Divider sx={{ my: 3 }} />
          <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 0.75 }}>完整任务 ID</Typography>
          <Typography variant="body2" sx={{ fontFamily: 'monospace', wordBreak: 'break-all' }}>{task.id}</Typography>
        </Box>
      )}
          </Paper>

          <Box component="aside" sx={{ minHeight: 0, overflowY: { xs: 'visible', lg: 'auto' }, display: 'flex', flexDirection: 'column', gap: 2, pb: 0.5 }}>
            <Paper elevation={0} sx={{ p: 2.25, bgcolor: 'background.paper', border: '1px solid', borderColor: 'divider', borderRadius: 3.5, flexShrink: 0 }}>
              <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 2 }}>
                <RateReviewIcon sx={{ fontSize: 19, color: 'secondary.main' }} />
                <Typography variant="subtitle2" sx={{ flexGrow: 1 }}>交付审核</Typography>
                <Chip size="small" label={task.prState || '未交付'} color={task.prState === 'merged' ? 'success' : 'default'} variant={task.prState === 'merged' ? 'filled' : 'outlined'} />
              </Box>
              <Box sx={{ display: 'grid', gap: 1.25 }}>
                <MetaRow k="分支" v={<code>{task.branch || '—'}</code>} />
                {pr?.prUrl && <MetaRow k="链接" v={<Link href={pr.prUrl} target="_blank" rel="noreferrer">打开 PR/MR <OpenInNewIcon sx={{ fontSize: 12 }} /></Link>} />}
                {!pr?.prUrl && pr?.createUrl && task.status === 'review' && (
                  <MetaRow k="创建" v={<Link href={pr.createUrl} target="_blank" rel="noreferrer">从分支创建 PR/MR <OpenInNewIcon sx={{ fontSize: 12 }} /></Link>} />
                )}
                {task.mergeCommit && <MetaRow k="合并提交" v={<code>{task.mergeCommit.slice(0, 12)}</code>} />}
                <Button size="small" variant="outlined" startIcon={<RefreshIcon />} sx={{ mt: 0.5 }} onClick={() => action(() => post(`/tasks/${taskId}/check-merge`), '合并检测完成')}>检测合并状态</Button>
                {task.status === 'review' && task.prState !== 'merged' && isAdmin && (
                  <FormControlLabel control={<Checkbox size="small" checked={manualConfirm} onChange={e => setManualConfirm(e.target.checked)} />}
                    label={<Typography variant="caption">人工确认已合并（保留审计记录）</Typography>} sx={{ alignItems: 'flex-start', '& .MuiCheckbox-root': { mt: -0.5 } }} />
                )}
                {task.status === 'review' && task.prState !== 'merged' && !isAdmin && (
                  <Typography variant="caption" color="text.secondary">尚未检测到合并；需管理员人工确认或等待自动检测。</Typography>
                )}
              </Box>
            </Paper>

            <Paper elevation={0} sx={{ p: 2.25, bgcolor: 'background.paper', border: '1px solid', borderColor: 'divider', borderRadius: 3.5, flexShrink: 0 }}>
              <Typography variant="subtitle2" gutterBottom sx={{ mb: 2 }}>任务信息</Typography>
              <Box sx={{ display: 'grid', gap: 1.25 }}>
                <MetaRow k="项目" v={data?.project?.name || '—'} />
                <MetaRow k="优先级" v={task.priority === 0 ? '普通' : `P${task.priority}`} />
                <MetaRow k="标签" v={parseJson<string[]>(task.tags, []).join(', ') || '—'} />
                <MetaRow k="创建时间" v={task.createdAt?.replace('T', ' ').slice(0, 16)} />
                <MetaRow k="版本" v={`v${task.version}`} />
              </Box>
            </Paper>

            <Paper elevation={0} sx={{ p: 2.25, bgcolor: 'background.paper', border: '1px solid', borderColor: 'divider', borderRadius: 3.5, flexShrink: 0 }}>
              <Typography variant="subtitle2" gutterBottom sx={{ mb: 1.5 }}>运行记录（{data?.runs.length || 0}）</Typography>
              {(data?.runs || []).map(r => (
                <Box key={r.id} sx={{ py: 1.25, borderBottom: '1px solid', borderColor: 'divider', '&:last-child': { borderBottom: 0, pb: 0 } }}>
                  <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 1 }}>
                    <Chip size="small" label={r.status} color={r.status === 'finished' ? 'success' : r.status === 'running' ? 'primary' : 'error'} />
                    <Typography variant="caption" color="text.secondary">{r.startedAt?.replace('T', ' ').slice(5, 16)}</Typography>
                  </Box>
                  <Typography variant="caption" color="text.secondary" display="block" sx={{ mt: 0.75 }}>
                    {r.nodeId === 'local' ? '本地节点' : '远程节点'} · {r.turnCount} 轮{r.retryCount > 0 ? ` · 重试 #${r.retryCount}` : ''}{r.stopReason ? ` · ${r.stopReason}` : ''}
                  </Typography>
                </Box>
              ))}
              {(data?.runs || []).length === 0 && <Typography variant="body2" color="text.secondary">还没有运行记录</Typography>}
            </Paper>
          </Box>
        </Box>
      </Box>

      <Dialog open={rejectOpen} onClose={() => setRejectOpen(false)} fullWidth maxWidth="xs">
        <DialogTitle>驳回返工</DialogTitle>
        <DialogContent>
          <TextField autoFocus fullWidth multiline minRows={3} label="评审意见（作为返工指令）" value={rejectReason} onChange={e => setRejectReason(e.target.value)} sx={{ mt: 1 }} />
        </DialogContent>
        <DialogActions sx={{ px: 3, pb: 2 }}>
          <Button onClick={() => setRejectOpen(false)}>取消</Button>
          <Button variant="contained" color="error" disabled={!rejectReason.trim()} onClick={async () => {
            await action(() => post(`/tasks/${taskId}/transition`, { to: 'ready', reason: rejectReason }), '已驳回，任务回到待办')
            setRejectOpen(false); setRejectReason('')
          }}>驳回</Button>
        </DialogActions>
      </Dialog>
    </Box>
  )
}

function RunDivider({ label, icon }: { label: string; icon?: React.ReactNode }) {
  return (
    <Divider sx={{ my: 1.5 }}>
      <Chip size="small" icon={icon as any} label={label} variant="outlined" sx={{ color: 'text.secondary', height: 22 }} />
    </Divider>
  )
}

function MetaRow({ k, v }: { k: string; v: React.ReactNode }) {
  return (
    <Box sx={{ display: 'flex', gap: 2, alignItems: 'center' }}>
      <Typography variant="caption" color="text.secondary" sx={{ width: 72, flexShrink: 0 }}>{k}</Typography>
      <Box sx={{ minWidth: 0, wordBreak: 'break-all' }}>{typeof v === 'string' ? <Typography variant="body2">{v}</Typography> : v}</Box>
    </Box>
  )
}