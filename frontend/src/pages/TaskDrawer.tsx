import { useEffect, useMemo, useRef, useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Drawer, Box, Typography, Chip, IconButton, Tabs, Tab, TextField, Button, Divider, Alert, Paper,
  Checkbox, FormControlLabel, Dialog, DialogTitle, DialogContent, DialogActions, Link, Tooltip, CircularProgress,
} from '@mui/material'
import CloseIcon from '@mui/icons-material/Close'
import SendIcon from '@mui/icons-material/Send'
import OpenInNewIcon from '@mui/icons-material/OpenInNew'
import RefreshIcon from '@mui/icons-material/Refresh'
import SmartToyIcon from '@mui/icons-material/SmartToy'
import BuildIcon from '@mui/icons-material/Build'
import CheckCircleIcon from '@mui/icons-material/CheckCircle'
import CancelIcon from '@mui/icons-material/Cancel'
import PlayArrowIcon from '@mui/icons-material/PlayArrow'
import { get, post, taskEventsUrl, parseJson, STATUS_LABEL, type Task, type Run, type Project, type Envelope } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { STATUS_ACCENT } from '../theme'
import { PersonAvatar, StatusChip, useToast } from '../ui'

const fmtTime = (s: string) => s?.replace('T', ' ').slice(11, 19)

export default function TaskDrawer({ taskId, onClose }: { taskId: string; onClose: () => void }) {
  const { role } = useOutletContext<WsCtx>()
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

  const { data } = useQuery({ queryKey: ['task', taskId], queryFn: () => get<{ task: Task; runs: Run[]; project?: Project }>(`/tasks/${taskId}`) })
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

  if (!task) return <Drawer anchor="right" open onClose={onClose} PaperProps={{ sx: { width: 680, p: 3, display: 'grid', placeItems: 'center' } }}><CircularProgress /></Drawer>

  const accent = STATUS_ACCENT[task.status] || '#8b93a7'

  return (
    <Drawer anchor="right" open onClose={onClose} PaperProps={{ sx: { width: 680, maxWidth: '100vw', display: 'flex', flexDirection: 'column', bgcolor: 'background.default' } }}>
      {/* 头部 */}
      <Box sx={{ p: 2.5, pb: 2, borderBottom: '1px solid', borderColor: 'divider', bgcolor: 'background.paper' }}>
        <Box sx={{ display: 'flex', gap: 1, alignItems: 'flex-start' }}>
          <Box sx={{ flexGrow: 1, minWidth: 0 }}>
            <Box sx={{ display: 'flex', gap: 0.75, mb: 1, flexWrap: 'wrap', alignItems: 'center' }}>
              <StatusChip status={task.status} />
              {task.prState && <Chip size="small" variant="outlined" color={task.prState === 'merged' ? 'success' : 'default'} label={`PR: ${task.prState}`} />}
              {task.branch && <Chip size="small" variant="outlined" label={task.branch} />}
              {task.retryCount > 0 && <Chip size="small" variant="outlined" label={`重试 ${task.retryCount}`} />}
            </Box>
            <Typography variant="h6" sx={{ wordBreak: 'break-word', lineHeight: 1.35 }}>{task.title}</Typography>
          </Box>
          <IconButton onClick={onClose}><CloseIcon /></IconButton>
        </Box>
        {/* 快捷操作 */}
        <Box sx={{ display: 'flex', gap: 1, mt: 1.5, flexWrap: 'wrap' }}>
          {task.status === 'planning' && <Button size="small" variant="contained" onClick={() => action(() => post(`/tasks/${taskId}/transition`, { to: 'ready' }), '已移入待办，等待智能体认领')}>规划完成 → 待办</Button>}
          {task.status === 'blocked' && <Button size="small" variant="contained" color="warning" onClick={() => action(() => post(`/tasks/${taskId}/unblock`), '已解除阻塞')}>解除阻塞</Button>}
          {task.status === 'in_progress' && <Button size="small" variant="outlined" color="warning" onClick={() => action(() => post(`/tasks/${taskId}/cancel-run`), '已发送取消信号')}>取消运行</Button>}
          {task.status === 'review' && <Button size="small" variant="contained" color="success" startIcon={<CheckCircleIcon />}
            disabled={busy || (task.prState !== 'merged' && !manualConfirm)}
            onClick={() => action(() => post(`/tasks/${taskId}/transition`, { to: 'done', manualConfirm }), '已批准，任务完成')}>批准</Button>}
          {task.status === 'review' && <Button size="small" variant="outlined" color="error" startIcon={<CancelIcon />} onClick={() => setRejectOpen(true)}>驳回</Button>}
          {task.status === 'done' && <Button size="small" variant="outlined" onClick={() => action(() => post(`/tasks/${taskId}/archive`), '已归档')}>归档</Button>}
        </Box>
      </Box>

      {task.status === 'blocked' && task.blockReason && (
        <Alert severity="error" sx={{ m: 2, mb: 0 }}>{task.blockReason}</Alert>
      )}

      <Tabs value={tab} onChange={(_, v) => setTab(v)} sx={{ px: 2, borderBottom: '1px solid', borderColor: 'divider', bgcolor: 'background.paper' }}>
        <Tab label="时间线" /><Tab label="审核与详情" />
      </Tabs>

      {tab === 0 && (
        <>
          <Box sx={{ flexGrow: 1, overflowY: 'auto', p: 2.5, display: 'flex', flexDirection: 'column', gap: 1.25 }}>
            {items.length === 0 && <Typography color="text.secondary" variant="body2" sx={{ textAlign: 'center', mt: 4 }}>暂无事件</Typography>}
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
                        <Box sx={{ width: 26, height: 26, borderRadius: '50%', bgcolor: 'rgba(109,141,255,0.15)', display: 'grid', placeItems: 'center', flexShrink: 0, mt: 0.25 }}>
                          <SmartToyIcon sx={{ fontSize: 15, color: 'primary.light' }} />
                        </Box>
                        <Paper elevation={0} sx={{ p: 1.5, bgcolor: 'background.paper', border: '1px solid', borderColor: 'divider', borderRadius: 2.5, whiteSpace: 'pre-wrap', wordBreak: 'break-word', maxWidth: '92%' }}>
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
          <Box sx={{ p: 2, borderTop: '1px solid', borderColor: 'divider', bgcolor: 'background.paper' }}>
            <Chip size="small" label={commentHint} variant="outlined" sx={{ mb: 1, color: 'text.secondary', maxWidth: '100%', '& .MuiChip-label': { overflow: 'hidden', textOverflow: 'ellipsis' } }} />
            <Box sx={{ display: 'flex', gap: 1 }}>
              <TextField fullWidth multiline maxRows={4} placeholder="输入评论…（Ctrl+Enter 发送）" value={comment} onChange={e => setComment(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) sendComment() }} />
              <IconButton color="primary" onClick={sendComment} disabled={busy || !comment.trim()} sx={{ alignSelf: 'flex-end' }}><SendIcon /></IconButton>
            </Box>
          </Box>
        </>
      )}

      {tab === 1 && (
        <Box sx={{ flexGrow: 1, overflowY: 'auto', p: 2.5, display: 'flex', flexDirection: 'column', gap: 2 }}>
          <Paper elevation={0} sx={{ p: 2.5, bgcolor: 'background.paper', border: '1px solid', borderColor: 'divider', borderRadius: 3 }}>
            <Typography variant="subtitle2" gutterBottom>PR/MR 审核</Typography>
            <Box sx={{ display: 'grid', gap: 1.25 }}>
              <MetaRow k="分支" v={<code>{task.branch || '—'}</code>} />
              <MetaRow k="状态" v={task.prState ? <Chip size="small" label={task.prState} color={task.prState === 'merged' ? 'success' : 'default'} /> : '未交付'} />
              {pr?.prUrl && <MetaRow k="链接" v={<Link href={pr.prUrl} target="_blank" rel="noreferrer">打开 PR/MR <OpenInNewIcon sx={{ fontSize: 12 }} /></Link>} />}
              {!pr?.prUrl && pr?.createUrl && task.status === 'review' && (
                <MetaRow k="创建" v={<Link href={pr.createUrl} target="_blank" rel="noreferrer">从分支创建 PR/MR <OpenInNewIcon sx={{ fontSize: 12 }} /></Link>} />
              )}
              {task.mergeCommit && <MetaRow k="合并提交" v={<code>{task.mergeCommit.slice(0, 12)}</code>} />}
              <Box sx={{ display: 'flex', gap: 1, flexWrap: 'wrap', mt: 0.5 }}>
                <Button size="small" startIcon={<RefreshIcon />} onClick={() => action(() => post(`/tasks/${taskId}/check-merge`), '合并检测完成')}>检测合并</Button>
                {task.status === 'review' && (
                  <>
                    <Button size="small" variant="contained" color="success" disabled={busy || (task.prState !== 'merged' && !manualConfirm)}
                      onClick={() => action(() => post(`/tasks/${taskId}/transition`, { to: 'done', manualConfirm }), '已批准')}>批准（已完成）</Button>
                    <Button size="small" variant="outlined" color="error" onClick={() => setRejectOpen(true)}>驳回返工</Button>
                  </>
                )}
              </Box>
              {task.status === 'review' && task.prState !== 'merged' && isAdmin && (
                <FormControlLabel control={<Checkbox size="small" checked={manualConfirm} onChange={e => setManualConfirm(e.target.checked)} />}
                  label={<Typography variant="caption">人工确认已合并（自动检测兜底，留审计）</Typography>} />
              )}
              {task.status === 'review' && task.prState !== 'merged' && !isAdmin && (
                <Typography variant="caption" color="text.secondary">未检测到合并；需 admin 人工确认或等待合并检测。</Typography>
              )}
            </Box>
          </Paper>

          <Paper elevation={0} sx={{ p: 2.5, bgcolor: 'background.paper', border: '1px solid', borderColor: 'divider', borderRadius: 3 }}>
            <Typography variant="subtitle2" gutterBottom>任务内容</Typography>
            <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>{task.content || '（无内容）'}</Typography>
          </Paper>

          <Paper elevation={0} sx={{ p: 2.5, bgcolor: 'background.paper', border: '1px solid', borderColor: 'divider', borderRadius: 3 }}>
            <Typography variant="subtitle2" gutterBottom>元数据</Typography>
            <Box sx={{ display: 'grid', gap: 1 }}>
              <MetaRow k="项目" v={data?.project?.name || '—'} />
              <MetaRow k="优先级" v={task.priority === 0 ? '普通' : `P${task.priority}`} />
              <MetaRow k="标签" v={parseJson<string[]>(task.tags, []).join(', ') || '—'} />
              <MetaRow k="创建时间" v={task.createdAt?.replace('T', ' ').slice(0, 19)} />
              <MetaRow k="版本" v={`v${task.version}`} />
            </Box>
          </Paper>

          <Paper elevation={0} sx={{ p: 2.5, bgcolor: 'background.paper', border: '1px solid', borderColor: 'divider', borderRadius: 3 }}>
            <Typography variant="subtitle2" gutterBottom>运行记录（{data?.runs.length || 0}）</Typography>
            {(data?.runs || []).map(r => (
              <Box key={r.id} sx={{ display: 'flex', gap: 1, alignItems: 'center', py: 0.75, borderBottom: '1px solid', borderColor: 'divider', '&:last-child': { borderBottom: 0 } }}>
                <Chip size="small" label={r.status} color={r.status === 'finished' ? 'success' : r.status === 'running' ? 'primary' : 'error'} />
                <Typography variant="caption" sx={{ flexGrow: 1 }}>
                  {r.nodeId === 'local' ? '本地' : '远程'} · {r.turnCount} 轮{r.retryCount > 0 ? ` · 重试#${r.retryCount}` : ''}{r.stopReason ? ` · ${r.stopReason}` : ''}
                </Typography>
                <Typography variant="caption" color="text.secondary">{r.startedAt?.replace('T', ' ').slice(5, 19)}</Typography>
              </Box>
            ))}
            {(data?.runs || []).length === 0 && <Typography variant="caption" color="text.secondary">暂无运行</Typography>}
          </Paper>
        </Box>
      )}

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
    </Drawer>
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