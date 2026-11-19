import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useOutletContext, useParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Box, Typography, Chip, IconButton, Tabs, Tab, TextField, Button, Divider, Alert, Paper,
  Checkbox, FormControlLabel, Dialog, DialogTitle, DialogContent, DialogActions, Link, Tooltip, CircularProgress, LinearProgress,
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
import LightbulbOutlinedIcon from '@mui/icons-material/LightbulbOutlined'
import ListAltOutlinedIcon from '@mui/icons-material/ListAltOutlined'
import AutorenewIcon from '@mui/icons-material/Autorenew'
import RadioButtonUncheckedIcon from '@mui/icons-material/RadioButtonUnchecked'
import LinkOutlinedIcon from '@mui/icons-material/LinkOutlined'
import AccountTreeIcon from '@mui/icons-material/AccountTree'
import SwapHorizIcon from '@mui/icons-material/SwapHoriz'
import ExpandMoreIcon from '@mui/icons-material/ExpandMore'
import ExpandLessIcon from '@mui/icons-material/ExpandLess'
import { get, post, taskEventsUrl, parseJson, STATUS_LABEL, STAGE_LABEL, STAGE_COLOR, type Task, type Run, type Project, type Envelope, type Workflow, type WorkflowStage, type Delivery, type Agent } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { EmptyState, PersonAvatar, StatusChip, useToast } from '../ui'
import WorkflowCanvas, { autoLayout, NODE_H_STATUS } from './WorkflowCanvas'

const fmtEventTime = (s?: string) => s ? `${s.slice(5, 10)} ${s.slice(11, 19)}` : ''

type TimelineItem = { kind: string; env?: Envelope; text?: string; runId?: string; at: string }

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
  const pageScrollRef = useRef<HTMLDivElement>(null)
  const allowAutoScroll = useRef(false)
  const isAdmin = role === 'owner' || role === 'admin'

  const { data, isError } = useQuery({ queryKey: ['task', taskId], queryFn: () => get<{ task: Task; runs: Run[]; project?: Project; workflow?: Workflow; deliveries?: Delivery[] }>(`/tasks/${taskId}`) })
  const task = data?.task
  const deliveries = data?.deliveries || []
  const deliveriesMerged = deliveries.length > 0 && deliveries.every(d => d.prState === 'merged')
  const { data: projects = [] } = useQuery({ queryKey: ['projects', wsId], queryFn: () => get<Project[]>(`/workspaces/${wsId}/projects`) })
  const { data: agents = [] } = useQuery({ queryKey: ['agents', wsId], queryFn: () => get<Agent[]>(`/workspaces/${wsId}/agents`) })
  const { data: pr } = useQuery({ queryKey: ['pr', taskId], queryFn: () => get<any>(`/tasks/${taskId}/pull-request`), enabled: !!task && !task.workflowId })

  useEffect(() => {
    setEvents([])
    allowAutoScroll.current = false
    const scrollTimer = window.setTimeout(() => { allowAutoScroll.current = true }, 1000)
    const es = new EventSource(taskEventsUrl(taskId, 0))
    es.onmessage = (ev) => {
      try {
        const env: Envelope = JSON.parse(ev.data)
        setEvents(prev => prev.some(e => e.seq === env.seq) ? prev : [...prev, env])
        if (String(env.kind).startsWith('task.') || env.kind.startsWith('pr.') || env.kind.startsWith('workflow.') || env.kind === 'comment.added') {
          qc.invalidateQueries({ queryKey: ['task', taskId] })
          qc.invalidateQueries({ queryKey: ['pr', taskId] })
          qc.invalidateQueries({ queryKey: ['tasks'] })
        }
      } catch { /* ignore */ }
    }
    return () => {
      es.close()
      window.clearTimeout(scrollTimer)
    }
  }, [taskId, qc])

  useEffect(() => {
    if (!allowAutoScroll.current) return
    const el = pageScrollRef.current
    // 只有用户本来就在底部时才跟随最新动态，避免查看工作流状态时被拽走
    if (!el || el.scrollHeight - el.scrollTop - el.clientHeight > 160) return
    bottomRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' })
  }, [events.length])

  useEffect(() => { if (pageScrollRef.current) pageScrollRef.current.scrollTop = 0 }, [tab, taskId])

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
    const out: TimelineItem[] = []
    for (const env of events) {
      if (env.kind === 'agent.message' || env.kind === 'agent.thought') {
        const last = out[out.length - 1]
        const text = env.payload?.text || ''
        if (last && last.kind === env.kind && last.runId === env.runId) { last.text = last.text ? `${last.text}\n${text}` : text }
        else out.push({ kind: env.kind, text, runId: env.runId, at: env.createdAt })
      } else if (env.kind === 'usage.updated' || env.kind === 'notification') {
        continue
      } else {
        out.push({ kind: env.kind, env, at: env.createdAt })
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
    <Box sx={{ height: '100%', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <Box sx={{ px: { xs: 2, md: 3.5 }, pt: 3, pb: 2.5, flexShrink: 0, bgcolor: 'background.default', borderBottom: '1px solid', borderColor: 'divider' }}>
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
                disabled={busy || ((task.workflowId ? !(deliveries.length > 0 && deliveries.every(d => d.prState === 'merged')) : task.prState !== 'merged') && !manualConfirm)}
                onClick={() => action(() => post(`/tasks/${taskId}/transition`, { to: 'done', manualConfirm }), '已批准，任务完成')}>批准交付</Button>}
              {task.status === 'review' && <Button variant="outlined" color="error" startIcon={<CancelIcon />} onClick={() => setRejectOpen(true)}>驳回</Button>}
              {task.status === 'done' && <Button variant="outlined" onClick={() => action(() => post(`/tasks/${taskId}/archive`), '已归档')}>归档任务</Button>}
            </Box>
          </Box>
          {task.status === 'blocked' && task.blockReason && <Alert severity="error" sx={{ mt: 2 }}>{task.blockReason}</Alert>}
        </Box>
      </Box>

      <Box ref={pageScrollRef} sx={{ flexGrow: 1, minHeight: 0, overflowY: 'auto', px: { xs: 2, md: 3.5 }, pb: 3.5 }}>
        <Box sx={{
          maxWidth: 1500, mx: 'auto', mt: 2, display: 'grid', gap: 2.5,
          gridTemplateColumns: { xs: 'minmax(0,1fr)', lg: 'minmax(0,1fr) 340px' }, alignItems: 'start',
        }}>
          <Paper sx={{ minHeight: 560, display: 'flex', flexDirection: 'column', borderRadius: 1.25, boxShadow: '0 1px 4px rgba(29,36,58,0.05)' }}>
            <Tabs value={tab} onChange={(_, v) => setTab(v)} sx={{
              position: 'sticky', top: 0, zIndex: 3, px: 2, borderBottom: '1px solid', borderColor: 'divider',
              bgcolor: '#fafbfc', flexShrink: 0, borderTopLeftRadius: 'inherit', borderTopRightRadius: 'inherit',
            }}>
              <Tab label={`时间轴 ${items.length}`} /><Tab label="任务说明" />{task.workflowId && <Tab label="流程" />}
            </Tabs>

      {tab === 0 && (
        <>
          <Box sx={{ p: { xs: 2, md: 3 } }}>
            {data?.workflow && (
              <WorkflowStatusPanel workflow={data.workflow} runs={data.runs || []} projects={projects} agents={agents}
                taskStatus={task.status} onDetail={() => setTab(2)} />
            )}
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 1.75 }}>
              <Typography variant="subtitle2" sx={{ flexGrow: 1 }}>活动时间轴</Typography>
              <Typography variant="caption" color="text.secondary">共 {items.length} 条动态</Typography>
            </Box>
            {items.length === 0 && <EmptyState title="还没有任务动态" desc="智能体输出、工具调用、评论和状态变化会实时出现在这里。" />}
            {items.length > 0 && (
              <Box sx={{ position: 'relative', '&::before': { content: '""', position: 'absolute', left: 13, top: 12, bottom: 24, width: 2, borderRadius: 1, bgcolor: '#e6e8ef' } }}>
                {items.map((it, i) => {
                  const env = it.env
                  const time = fmtEventTime(it.at)
                  switch (it.kind) {
                case 'agent.message':
                  return (
                    <TimelineRow key={i} icon={<SmartToyIcon sx={{ fontSize: 15 }} />} tone="primary" title="智能体输出" time={time}>
                      <Paper elevation={0} sx={{ p: 1.5, bgcolor: '#f5f6fa', borderRadius: 1.25, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                        <Typography variant="body2" sx={{ lineHeight: 1.75 }}>{it.text}</Typography>
                      </Paper>
                    </TimelineRow>
                  )
                case 'agent.thought':
                  return (
                    <TimelineRow key={i} icon={<LightbulbOutlinedIcon sx={{ fontSize: 14 }} />} tone="muted" title="思考" time={time}>
                      <Typography variant="caption" color="text.secondary" sx={{ display: 'block', fontStyle: 'italic', whiteSpace: 'pre-wrap', lineHeight: 1.7 }}>{it.text}</Typography>
                    </TimelineRow>
                  )
                case 'comment.added':
                  return (
                    <TimelineRow key={i} node={<PersonAvatar name={env?.payload?.username || 'U'} size={26} />}
                      title={`${env?.payload?.username || env?.actor || '成员'} 的评论`} time={time}>
                      <Paper elevation={0} sx={{ p: 1.5, bgcolor: '#eceaff', borderRadius: 1.25, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                        <Typography variant="body2">{env?.payload?.text}</Typography>
                      </Paper>
                    </TimelineRow>
                  )
                case 'task.moved':
                  return (
                    <TimelineRow key={i} icon={<SwapHorizIcon sx={{ fontSize: 15 }} />} title="状态变更" time={time}>
                      <Typography variant="body2" color="text.secondary">
                        {STATUS_LABEL[env?.payload?.from as keyof typeof STATUS_LABEL] || env?.payload?.from} → {STATUS_LABEL[env?.payload?.to as keyof typeof STATUS_LABEL] || env?.payload?.to}
                        {env?.payload?.reason ? ` · ${env.payload.reason}` : ''}
                      </Typography>
                    </TimelineRow>
                  )
                case 'task.created':
                  return <TimelineRow key={i} icon={<PlayArrowIcon sx={{ fontSize: 14 }} />} tone="muted" title="任务创建" time={time} />
                case 'run.started':
                  return (
                    <TimelineRow key={i} icon={<PlayArrowIcon sx={{ fontSize: 15 }} />} tone="primary"
                      title={`Run 开始 · ${env?.payload?.agentName || ''}${env?.payload?.stageName ? ` · ${env.payload.stageName}` : ''}`} time={time}>
                      <Typography variant="caption" color="text.secondary">
                        {env?.payload?.nodeId === 'local' ? '本地节点' : '远程节点'}{env?.payload?.branch ? ` · ${env.payload.branch}` : ''}
                      </Typography>
                    </TimelineRow>
                  )
                case 'tool.call': {
                  const st = toolStatus.get(env?.payload?.toolCallId)
                  return (
                    <TimelineRow key={i} icon={<BuildIcon sx={{ fontSize: 14 }} />} tone="muted" title={env?.payload?.title || env?.payload?.kind || '工具调用'} time={time}>
                      <Box sx={{ display: 'flex', gap: 1, alignItems: 'center' }}>
                        <Typography variant="caption" color="text.secondary" sx={{ flexGrow: 1, fontFamily: 'monospace' }}>{env?.payload?.kind || 'tool'}</Typography>
                        <Chip size="small" label={st === 'completed' ? '完成' : st === 'failed' ? '失败' : st || '进行中'}
                          color={st === 'completed' ? 'success' : st === 'failed' ? 'error' : 'default'} variant="outlined" sx={{ height: 18, fontSize: 10 }} />
                      </Box>
                    </TimelineRow>
                  )
                }
                case 'tool.result': return null
                case 'plan.updated':
                  return (
                    <TimelineRow key={i} icon={<ListAltOutlinedIcon sx={{ fontSize: 15 }} />} title="执行计划" time={time}>
                      <Paper elevation={0} sx={{ p: 1.5, bgcolor: 'background.paper', border: '1px solid', borderColor: 'divider', borderRadius: 1.25 }}>
                        {(env?.payload?.entries || []).map((e: any, j: number) => (
                          <Box key={j} sx={{ display: 'flex', alignItems: 'center', gap: 0.75, py: 0.25 }}>
                            {e.status === 'completed' ? <CheckCircleIcon sx={{ fontSize: 16, color: 'success.main' }} /> : e.status === 'in_progress' ? <AutorenewIcon sx={{ fontSize: 16, color: 'primary.main' }} /> : <RadioButtonUncheckedIcon sx={{ fontSize: 16, color: 'text.disabled' }} />}
                            <Typography variant="body2">{e.content}</Typography>
                          </Box>
                        ))}
                      </Paper>
                    </TimelineRow>
                  )
                case 'permission.requested': {
                  const aid = env?.payload?.approvalId
                  const resolved = resolvedApprovals.has(aid)
                  return (
                    <TimelineRow key={i} icon={<RateReviewIcon sx={{ fontSize: 15 }} />} tone="warning" title="工具授权请求" time={time}>
                      <Alert severity="warning" action={
                        !resolved && (
                          <Box sx={{ display: 'flex', gap: 0.5 }}>
                            <Button size="small" color="success" variant="contained" onClick={() => action(() => post(`/approvals/${aid}/decide`, { decision: 'approve' }), '已批准')}>批准</Button>
                            <Button size="small" color="error" variant="outlined" onClick={() => action(() => post(`/approvals/${aid}/decide`, { decision: 'reject' }), '已拒绝')}>拒绝</Button>
                          </Box>
                        )
                      }>
                        {env?.payload?.request?.toolCall?.title || '未知操作'}{resolved ? '（已处理）' : ''}
                      </Alert>
                    </TimelineRow>
                  )
                }
                case 'permission.resolved':
                  return (
                    <TimelineRow key={i} icon={env?.payload?.decision === 'approve' ? <CheckCircleIcon sx={{ fontSize: 15 }} /> : <CancelIcon sx={{ fontSize: 15 }} />}
                      tone={env?.payload?.decision === 'approve' ? 'success' : 'error'}
                      title={`授权已${env?.payload?.decision === 'approve' ? '批准' : '拒绝'}`} time={time} />
                  )
                case 'watchdog.loop': case 'watchdog.stalled': case 'watchdog.hard_limit': case 'watchdog.no_progress': case 'watchdog.interrupted':
                  return (
                    <TimelineRow key={i} icon={<CancelIcon sx={{ fontSize: 15 }} />} tone="error" title={`质量阀触发 · ${it.kind.split('.')[1]}`} time={time}>
                      <Alert severity="error">{env?.payload?.diagnostics || env?.payload?.error || '触发熔断'}</Alert>
                    </TimelineRow>
                  )
                case 'pr.linked':
                  return (
                    <TimelineRow key={i} icon={<LinkOutlinedIcon sx={{ fontSize: 15 }} />} tone="primary" title="PR/MR 已就绪" time={time}>
                      <Paper elevation={0} sx={{ p: 1.25, bgcolor: 'background.paper', border: '1px solid', borderColor: 'divider', borderRadius: 1.25 }}>
                        <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75, mb: env?.payload?.prUrl ? 0.75 : 0 }}>
                          <Chip size="small" label={env?.payload?.prState || 'open'} variant="outlined" sx={{ height: 20, fontSize: 10 }} />
                          <Typography variant="caption" color="text.secondary">等待合并</Typography>
                        </Box>
                        {env?.payload?.prUrl && <Link href={env.payload.prUrl} target="_blank" rel="noreferrer" variant="body2" sx={{ wordBreak: 'break-all' }}>{env.payload.prUrl} <OpenInNewIcon sx={{ fontSize: 12 }} /></Link>}
                      </Paper>
                    </TimelineRow>
                  )
                case 'pr.merged_detected':
                  return <TimelineRow key={i} icon={<CheckCircleIcon sx={{ fontSize: 15 }} />} tone="success" title="检测到 PR/MR 已合并" time={time} />
                case 'comment.injected':
                  return <TimelineRow key={i} icon={<SendIcon sx={{ fontSize: 14 }} />} tone="success" title="引导评论已送达智能体" time={time} />
                case 'workflow.started':
                  return (
                    <TimelineRow key={i} icon={<AccountTreeIcon sx={{ fontSize: 15 }} />} tone="primary" title={`工作流开始 · ${env?.payload?.workflowName || ''}`} time={time}>
                      <Typography variant="caption" color="text.secondary">节点依赖满足后自动执行，可在上方查看实时状态。</Typography>
                    </TimelineRow>
                  )
                case 'workflow.stage_completed': {
                  const files: string[] = Array.isArray(env?.payload?.files) ? env.payload.files : []
                  return (
                    <TimelineRow key={i} icon={<CheckCircleIcon sx={{ fontSize: 15 }} />} tone="success"
                      title={`节点完成 · ${env?.payload?.stageName || env?.payload?.stageKey || ''}`} time={time}>
                      <Box sx={{ display: 'flex', gap: 0.75, flexWrap: 'wrap' }}>
                        {env?.payload?.branch && <Chip size="small" variant="outlined" label={env.payload.branch} sx={{ height: 20, fontSize: 10 }} />}
                        {env?.payload?.headSha && <Chip size="small" variant="outlined" label={env.payload.headSha.slice(0, 8)} sx={{ height: 20, fontSize: 10 }} />}
                        {files.length > 0 && (
                          <Tooltip title={files.join('\n')}>
                            <Chip size="small" variant="outlined" label={`${files.length} 个文件`} sx={{ height: 20, fontSize: 10 }} />
                          </Tooltip>
                        )}
                      </Box>
                    </TimelineRow>
                  )
                }
                case 'workflow.stage_skipped':
                  return (
                    <TimelineRow key={i} icon={<RadioButtonUncheckedIcon sx={{ fontSize: 15 }} />} tone="muted"
                      title={`节点跳过 · ${env?.payload?.stageName || env?.payload?.stageKey || ''}`} time={time}>
                      {env?.payload?.reason && <Typography variant="caption" color="text.secondary">条件：{env.payload.reason}</Typography>}
                    </TimelineRow>
                  )
                case 'workflow.stage_failed':
                  return (
                    <TimelineRow key={i} icon={<CancelIcon sx={{ fontSize: 15 }} />} tone="error"
                      title={`节点失败 · ${env?.payload?.stageName || env?.payload?.stageKey || ''}`} time={time}>
                      <Alert severity="error">{env?.payload?.error || env?.payload?.reason || '节点执行失败'}</Alert>
                    </TimelineRow>
                  )
                case 'workflow.completed':
                  return (
                    <TimelineRow key={i} icon={<CheckCircleIcon sx={{ fontSize: 15 }} />} tone="success" title="工作流全部完成" time={time}>
                      <Typography variant="caption" color="text.secondary">所有节点已完成，任务进入审核。</Typography>
                    </TimelineRow>
                  )
                case 'workflow.rewound':
                  return (
                    <TimelineRow key={i} icon={<AutorenewIcon sx={{ fontSize: 15 }} />} tone="warning" title={`返工 · 从「${env?.payload?.fromStage || ''}」开始`} time={time}>
                      <Typography variant="caption" color="text.secondary">该节点及其下游已重置，等待重新执行。</Typography>
                    </TimelineRow>
                  )
                case 'workflow.rework_reset':
                  return <TimelineRow key={i} icon={<AutorenewIcon sx={{ fontSize: 15 }} />} tone="warning" title="返工重置" time={time}>
                    <Typography variant="caption" color="text.secondary">已重置汇点节点，等待重跑。</Typography>
                  </TimelineRow>
                case 'workflow.retry_reset':
                  return <TimelineRow key={i} icon={<AutorenewIcon sx={{ fontSize: 15 }} />} tone="warning" title="失败节点已重置" time={time}>
                    <Typography variant="caption" color="text.secondary">解除阻塞后失败节点将重新执行。</Typography>
                  </TimelineRow>
                default:
                  return <TimelineRow key={i} icon={<RadioButtonUncheckedIcon sx={{ fontSize: 14 }} />} tone="muted" title={it.kind} time={time} />
              }
                })}
              </Box>
            )}
            <div ref={bottomRef} />
          </Box>
          <Box sx={{ p: 2, borderTop: '1px solid', borderColor: 'divider', bgcolor: '#fafbfc', flexShrink: 0 }}>
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
        <Box sx={{ p: { xs: 2.5, md: 3.5 } }}>
          <Typography variant="subtitle2" gutterBottom>任务内容</Typography>
          <Typography variant="body2" color={task.content ? 'text.primary' : 'text.secondary'} sx={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word', lineHeight: 1.85, mt: 2 }}>{task.content || '这个任务还没有填写详细内容。'}</Typography>
          <Divider sx={{ my: 3 }} />
          <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 0.75 }}>完整任务 ID</Typography>
          <Typography variant="body2" sx={{ fontFamily: 'monospace', wordBreak: 'break-all' }}>{task.id}</Typography>
        </Box>
      )}

      {tab === 2 && data?.workflow && (
        <WorkflowFlowPanel
          workflow={data.workflow}
          runs={data.runs || []}
          projects={projects}
          agents={agents}
          taskStatus={task.status}
          isAdmin={isAdmin}
          onRewind={(key) => action(() => post(`/tasks/${taskId}/rewind`, { fromStage: key }), `已重置节点「${key}」及其下游`)}
          onSkip={(key) => action(() => post(`/tasks/${taskId}/stages/${key}/skip`), `已跳过节点「${key}」`)}
        />
      )}
          </Paper>

          <Box component="aside" sx={{
            display: 'flex', flexDirection: 'column', gap: 2, pb: 0.5,
            position: { xs: 'static', lg: 'sticky' }, top: { lg: 16 }, alignSelf: 'start',
            maxHeight: { lg: 'calc(100vh - 32px)' }, overflowY: { xs: 'visible', lg: 'auto' },
          }}>
            <Paper elevation={0} sx={{ p: 2.25, bgcolor: 'background.paper', borderRadius: 1.25, flexShrink: 0, boxShadow: '0 1px 4px rgba(29,36,58,0.05)' }}>
              <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 2 }}>
                <RateReviewIcon sx={{ fontSize: 19, color: 'secondary.main' }} />
                <Typography variant="subtitle2" sx={{ flexGrow: 1 }}>交付审核</Typography>
                <Chip size="small" label={task.prState || '未交付'} color={task.prState === 'merged' ? 'success' : 'default'} variant={task.prState === 'merged' ? 'filled' : 'outlined'} />
              </Box>
              {deliveries.length > 0 ? (
                <Box sx={{ display: 'grid', gap: 1.5 }}>
                  {deliveries.map(d => (
                    <DeliveryRow key={d.id} d={d} projectName={projects.find(p => p.id === d.projectId)?.name || d.projectId} onSaved={refresh} />
                  ))}
                  <Button size="small" variant="outlined" startIcon={<RefreshIcon />}
                    onClick={() => action(() => post(`/tasks/${taskId}/check-merge`), '合并检测完成')}>检测全部交付</Button>
                  {task.status === 'review' && !deliveriesMerged && isAdmin && (
                    <FormControlLabel control={<Checkbox size="small" checked={manualConfirm} onChange={e => setManualConfirm(e.target.checked)} />}
                      label={<Typography variant="caption">人工确认全部已合并（保留审计记录）</Typography>} sx={{ alignItems: 'flex-start', '& .MuiCheckbox-root': { mt: -0.5 } }} />
                  )}
                  {task.status === 'review' && !deliveriesMerged && !isAdmin && (
                    <Typography variant="caption" color="text.secondary">尚未全部合并；需管理员人工确认或等待自动检测。</Typography>
                  )}
                </Box>
              ) : (
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
              )}
            </Paper>

            <Paper elevation={0} sx={{ p: 2.25, bgcolor: 'background.paper', borderRadius: 1.25, flexShrink: 0, boxShadow: '0 1px 4px rgba(29,36,58,0.05)' }}>
              <Typography variant="subtitle2" gutterBottom sx={{ mb: 2 }}>任务信息</Typography>
              <Box sx={{ display: 'grid', gap: 1.25 }}>
                <MetaRow k="项目" v={data?.project?.name || '—'} />
                <MetaRow k="优先级" v={task.priority === 0 ? '普通' : `P${task.priority}`} />
                <MetaRow k="标签" v={parseJson<string[]>(task.tags, []).join(', ') || '—'} />
                <MetaRow k="创建时间" v={task.createdAt?.replace('T', ' ').slice(0, 16)} />
                <MetaRow k="版本" v={`v${task.version}`} />
              </Box>
            </Paper>

            <Paper elevation={0} sx={{ p: 2.25, bgcolor: 'background.paper', borderRadius: 1.25, flexShrink: 0, boxShadow: '0 1px 4px rgba(29,36,58,0.05)' }}>
              <Typography variant="subtitle2" gutterBottom sx={{ mb: 1.5 }}>运行记录（{data?.runs.length || 0}）</Typography>
              {(data?.runs || []).map(r => (
                <Box key={r.id} sx={{ py: 1.25, borderBottom: '1px solid', borderColor: 'divider', '&:last-child': { borderBottom: 0, pb: 0 } }}>
                  <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 1 }}>
                    <Chip size="small" label={r.status} color={r.status === 'finished' ? 'success' : r.status === 'running' ? 'primary' : 'error'} />
                    <Typography variant="caption" color="text.secondary">{r.startedAt?.replace('T', ' ').slice(5, 16)}</Typography>
                  </Box>
                  <Typography variant="caption" color="text.secondary" display="block" sx={{ mt: 0.75 }}>
                    {r.nodeId === 'local' ? '本地节点' : '远程节点'} · {r.turnCount} 轮{r.stageKey ? ` · 节点 ${r.stageKey}` : ''}{r.retryCount > 0 ? ` · 重试 #${r.retryCount}` : ''}{r.stopReason ? ` · ${r.stopReason}` : ''}
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

const TIMELINE_TONE = {
  default: { color: '#697386', bg: '#f1f2f6' },
  primary: { color: '#635bff', bg: '#eceaff' },
  success: { color: '#16a34a', bg: '#e9f8ef' },
  warning: { color: '#b45309', bg: '#fff6e6' },
  error: { color: '#d85160', bg: '#fdeef0' },
  muted: { color: '#8b93a7', bg: '#f4f5f8' },
} as const

function TimelineRow({ icon, node, tone = 'default', title, time, children }: {
  icon?: React.ReactNode; node?: React.ReactNode; tone?: keyof typeof TIMELINE_TONE
  title: string; time?: string; children?: React.ReactNode
}) {
  const t = TIMELINE_TONE[tone]
  return (
    <Box sx={{ position: 'relative', display: 'flex', gap: 1.5, pb: 2.25 }}>
      <Box sx={{ position: 'relative', zIndex: 1, width: 28, flexShrink: 0, display: 'flex', justifyContent: 'center', pt: 0.25 }}>
        {node || (
          <Box sx={{
            width: 26, height: 26, borderRadius: '50%', bgcolor: t.bg, color: t.color,
            display: 'grid', placeItems: 'center', border: '2px solid #fff', boxShadow: '0 0 0 1px #e6e8ef',
          }}>{icon}</Box>
        )}
      </Box>
      <Box sx={{ flexGrow: 1, minWidth: 0, pt: 0.25 }}>
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
          <Typography variant="body2" fontWeight={650} sx={{ lineHeight: 1.4, minWidth: 0, wordBreak: 'break-word' }}>{title}</Typography>
          {time && <Typography variant="caption" color="text.secondary" sx={{ ml: 'auto', flexShrink: 0, fontVariantNumeric: 'tabular-nums' }}>{time}</Typography>}
        </Box>
        {children && <Box sx={{ mt: 1 }}>{children}</Box>}
      </Box>
    </Box>
  )
}

/** 按节点取最新 Run，供流程图与时间轴共用。 */
function useStageStatus(runs: Run[]) {
  return useMemo(() => {
    const latest = new Map<string, Run>()
    for (const r of runs) if (r.stageKey) latest.set(r.stageKey, r)
    const statusOf = (key: string) => {
      const r = latest.get(key)
      if (!r) return 'pending'
      if (r.status === 'finished') return 'succeeded'
      if (r.status === 'running') return 'running'
      if (r.status === 'skipped') return 'skipped'
      if (r.status === 'cancelled') return 'cancelled'
      return 'failed'
    }
    const metaOf = (key: string) => {
      const r = latest.get(key)
      if (!r) return undefined
      return `${r.turnCount} 轮${r.retryCount > 0 ? ` · 重试 #${r.retryCount}` : ''}${r.headSha ? ` · ${r.headSha.slice(0, 8)}` : ''}`
    }
    return { latest, statusOf, metaOf }
  }, [runs])
}

/** 任务详情内嵌的工作流状态面板。 */
function WorkflowStatusPanel({ workflow, runs, projects, agents, taskStatus, onDetail }: {
  workflow: Workflow; runs: Run[]; projects: Project[]; agents: Agent[]; taskStatus: string; onDetail: () => void
}) {
  const [open, setOpen] = useState(true)
  const stages = useMemo(() => autoLayout(parseJson<WorkflowStage[]>(workflow.stages, [])), [workflow.stages])
  const { statusOf, metaOf } = useStageStatus(runs)
  const done = stages.filter(s => ['succeeded', 'skipped'].includes(statusOf(s.key))).length
  const failed = stages.some(s => ['failed', 'cancelled'].includes(statusOf(s.key)))
  const running = stages.some(s => statusOf(s.key) === 'running')
  const allDone = stages.length > 0 && done === stages.length
  const pct = stages.length ? Math.round((done / stages.length) * 100) : 0
  const summary = failed ? '存在失败节点' : running ? '正在执行' : allDone ? '全部完成' : taskStatus === 'in_progress' ? '等待节点就绪' : '未开始'
  const barColor: 'error' | 'success' | 'primary' = failed ? 'error' : allDone ? 'success' : 'primary'
  const contentBottom = Math.max(0, ...stages.map(s => (s.y ?? 0) + NODE_H_STATUS))
  const canvasHeight = Math.min(440, Math.max(260, contentBottom + 80))
  return (
    <Paper elevation={0} sx={{ mb: 2.5, border: '1px solid', borderColor: 'divider', borderRadius: 1.25, overflow: 'hidden' }}>
      <Box sx={{ px: 2, py: 1.25, display: 'flex', alignItems: 'center', gap: 1, bgcolor: '#fafbfc' }}>
        <AccountTreeIcon sx={{ fontSize: 18, color: 'primary.main' }} />
        <Typography variant="subtitle2" sx={{ flexGrow: 1 }}>工作流状态</Typography>
        <Chip size="small" variant="outlined" label={`${done}/${stages.length} 节点`}
          color={failed ? 'error' : allDone ? 'success' : 'default'} sx={{ height: 22 }} />
        <Chip size="small" label={summary} color={failed ? 'error' : running ? 'primary' : 'default'} sx={{ height: 22 }} />
        <Button size="small" onClick={onDetail}>流程详情</Button>
        <IconButton size="small" onClick={() => setOpen(v => !v)} aria-label={open ? '收起工作流状态' : '展开工作流状态'}>
          {open ? <ExpandLessIcon fontSize="small" /> : <ExpandMoreIcon fontSize="small" />}
        </IconButton>
      </Box>
      <LinearProgress variant="determinate" value={pct} color={barColor} sx={{ height: 3 }} />
      {open && (
        <>
          <Box sx={{ display: 'flex', gap: 2, px: 2, py: 1, flexWrap: 'wrap', borderBottom: '1px solid', borderColor: 'divider', bgcolor: '#fff' }}>
            {([['succeeded', '已完成', '#16a34a'], ['running', '执行中', '#635bff'], ['pending', '待执行', '#a0a7b5'], ['failed', '失败', '#d85160'], ['skipped', '已跳过', '#8b93a7']] as const).map(([k, label, color]) => (
              <Box key={k} sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}>
                <Box sx={{ width: 6, height: 6, borderRadius: '50%', bgcolor: color }} />
                <Typography variant="caption" color="text.secondary">{label}</Typography>
              </Box>
            ))}
          </Box>
          <Box sx={{ height: canvasHeight }}>
            <WorkflowCanvas readonly stages={stages} agents={agents} projects={projects} selected={null}
              onChange={() => {}} onSelect={() => {}} statusOf={statusOf} metaOf={metaOf} />
          </Box>
        </>
      )}
    </Paper>
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

function DeliveryRow({ d, projectName, onSaved }: { d: Delivery; projectName: string; onSaved: () => void }) {
  const toast = useToast()
  const [url, setUrl] = useState(d.prUrl || '')
  const [busy, setBusy] = useState(false)
  const save = async () => {
    if (!url.trim()) return
    setBusy(true)
    try {
      await post(`/tasks/${d.taskId}/deliveries`, { deliveryId: d.id, prUrl: url.trim() })
      toast('PR/MR 链接已保存', 'success'); onSaved()
    } catch (e: any) { toast(e.message, 'error') } finally { setBusy(false) }
  }
  return (
    <Box sx={{ p: 1.25, border: '1px solid', borderColor: 'divider', borderRadius: 1.25 }}>
      <Box sx={{ display: 'flex', gap: 0.75, alignItems: 'center', mb: 0.75 }}>
        <Typography variant="body2" fontWeight={650} sx={{ flexGrow: 1 }}>{projectName}</Typography>
        <Chip size="small" variant="outlined" label={d.prState || '未交付'} color={d.prState === 'merged' ? 'success' : 'default'} />
      </Box>
      <Typography variant="caption" color="text.secondary" display="block">节点 {d.nodeKey} · <code>{d.branch || '—'}</code></Typography>
      {d.prState !== 'merged' && (
        <Box sx={{ display: 'flex', gap: 0.75, mt: 1 }}>
          <TextField size="small" fullWidth placeholder="粘贴 PR/MR 链接" value={url} onChange={e => setUrl(e.target.value)} />
          <Button size="small" variant="outlined" disabled={busy || !url.trim()} onClick={() => void save()}>保存</Button>
        </Box>
      )}
      {d.prUrl && <Link href={d.prUrl} target="_blank" rel="noreferrer" variant="body2" sx={{ mt: 0.75, display: 'inline-block' }}>打开 PR/MR <OpenInNewIcon sx={{ fontSize: 12 }} /></Link>}
      {d.prState === 'merged' && d.mergeCommit && <Typography variant="caption" color="text.secondary" display="block">合并提交 {d.mergeCommit.slice(0, 10)}</Typography>}
    </Box>
  )
}

function WorkflowFlowPanel({ workflow, runs, projects, agents, taskStatus, isAdmin, onRewind, onSkip }: {
  workflow: Workflow; runs: Run[]; projects: Project[]; agents: Agent[]; taskStatus: string; isAdmin: boolean
  onRewind: (key: string) => void; onSkip: (key: string) => void
}) {
  const stages = useMemo(() => autoLayout(parseJson<WorkflowStage[]>(workflow.stages, [])), [workflow.stages])
  const { latest, statusOf, metaOf } = useStageStatus(runs)
  const levelOf = (s: WorkflowStage, depth = 0): number => {
    if (depth > 20 || s.dependsOn.length === 0) return 0
    return 1 + Math.max(...s.dependsOn.map(k => { const d = stages.find(x => x.key === k); return d ? levelOf(d, depth + 1) : 0 }))
  }
  const levels = new Map<number, WorkflowStage[]>()
  for (const s of stages) {
    const lv = levelOf(s)
    levels.set(lv, [...(levels.get(lv) || []), s])
  }
  return (
    <Box sx={{ p: { xs: 2, md: 3 } }}>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 1 }}>
        <AccountTreeIcon sx={{ fontSize: 18, color: 'primary.main' }} />
        <Typography variant="subtitle2" sx={{ flexGrow: 1 }}>{workflow.name}</Typography>
      </Box>
      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 1.5 }}>
        节点依赖满足即自动执行；绿色为已完成，紫色为执行中，红色为失败。
      </Typography>
      <Box sx={{ height: 320, mb: 2.5, border: '1px solid', borderColor: 'divider', borderRadius: 1.25, overflow: 'hidden' }}>
        <WorkflowCanvas readonly stages={stages} agents={agents} projects={projects} selected={null}
          onChange={() => {}} onSelect={() => {}} statusOf={statusOf} metaOf={metaOf} />
      </Box>
      <Box sx={{ display: 'grid', gap: 2 }}>
        {[...levels.keys()].sort((a, b) => a - b).map(lv => (
          <Box key={lv}>
            <Typography variant="caption" color="text.secondary" sx={{ mb: 0.75, display: 'block' }}>第 {lv + 1} 层</Typography>
            <Box sx={{ display: 'grid', gap: 1.25, gridTemplateColumns: { xs: '1fr', md: 'repeat(auto-fill, minmax(240px, 1fr))' } }}>
              {levels.get(lv)!.map(s => {
                const st = statusOf(s.key)
                const r = latest.get(s.key)
                return (
                  <Paper key={s.key} elevation={0} sx={{ p: 1.5, border: '1px solid', borderColor: 'divider', borderRadius: 1.25 }}>
                    <Box sx={{ display: 'flex', gap: 0.75, alignItems: 'center', mb: 0.75 }}>
                      <Typography variant="body2" fontWeight={650} sx={{ flexGrow: 1 }}>{s.name || s.key}</Typography>
                      <Chip size="small" color={STAGE_COLOR[st] as any} label={STAGE_LABEL[st] || st} variant={st === 'pending' ? 'outlined' : 'filled'} />
                    </Box>
                    <Typography variant="caption" color="text.secondary" display="block">
                      项目：{projects.find(p => p.id === s.projectId)?.name || '继承主项目'} · 智能体：{agents.find(a => a.id === s.agentId)?.name || s.agentId.slice(0, 8)}
                    </Typography>
                    <Typography variant="caption" color="text.secondary" display="block">
                      {s.dependsOn.length ? `依赖：${s.dependsOn.join(', ')}` : '入口节点'}{s.when ? ` · 条件：${s.when}` : ''}
                    </Typography>
                    {r && <Typography variant="caption" color="text.secondary" display="block">
                      {r.turnCount} 轮{r.retryCount > 0 ? ` · 重试 #${r.retryCount}` : ''}{r.headSha ? ` · ${r.headSha.slice(0, 8)}` : ''}
                    </Typography>}
                    <Box sx={{ display: 'flex', gap: 1 }}>
                      {isAdmin && (taskStatus === 'blocked' || taskStatus === 'review' || taskStatus === 'done') && (
                        <Button size="small" sx={{ mt: 0.75 }} onClick={() => onRewind(s.key)}>从该节点返工</Button>
                      )}
                      {isAdmin && taskStatus === 'in_progress' && st !== 'succeeded' && st !== 'skipped' && (
                        <Button size="small" sx={{ mt: 0.75 }} onClick={() => onSkip(s.key)}>跳过</Button>
                      )}
                    </Box>
                  </Paper>
                )
              })}
            </Box>
          </Box>
        ))}
      </Box>
    </Box>
  )
}