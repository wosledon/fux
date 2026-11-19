import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Box, Button, Chip, CircularProgress, Divider, IconButton, Paper, Stack, TextField,
  Tooltip, Typography, ListItemButton, Dialog, DialogTitle, DialogContent, DialogActions, MenuItem, Alert, Collapse,
} from '@mui/material'
import AddCommentOutlinedIcon from '@mui/icons-material/AddCommentOutlined'
import SmartToyOutlinedIcon from '@mui/icons-material/SmartToyOutlined'
import SendRoundedIcon from '@mui/icons-material/SendRounded'
import StopCircleOutlinedIcon from '@mui/icons-material/StopCircleOutlined'
import ContentCopyIcon from '@mui/icons-material/ContentCopy'
import DeleteOutlineIcon from '@mui/icons-material/DeleteOutline'
import AutoAwesomeIcon from '@mui/icons-material/AutoAwesome'
import InsightsOutlinedIcon from '@mui/icons-material/InsightsOutlined'
import ErrorOutlineIcon from '@mui/icons-material/ErrorOutline'
import TaskAltIcon from '@mui/icons-material/TaskAlt'
import PlayCircleOutlineIcon from '@mui/icons-material/PlayCircleOutline'
import ArrowForwardIcon from '@mui/icons-material/ArrowForward'
import SettingsOutlinedIcon from '@mui/icons-material/SettingsOutlined'
import PsychologyOutlinedIcon from '@mui/icons-material/PsychologyOutlined'
import ExpandMoreIcon from '@mui/icons-material/ExpandMore'
import BuildOutlinedIcon from '@mui/icons-material/BuildOutlined'
import {
  API, get, patch, del, getToken, parseJson,
  type AssistantSession, type AssistantMessage, type Agent, type Board, type Task, type TaskStatus, type CliEntry,
} from '../api'
import { useAuth } from '../auth'
import { Markdown, StatusChip, ToolRow, useConfirm, useToast, type ToolCallView } from '../ui'
import type { WsCtx } from './WorkspaceLayout'

interface ChatMsg {
  role: 'user' | 'assistant' | 'process'
  content?: string
  thoughts?: string
  tools?: ToolCallView[]
  createdAt?: string
}

const SUGGESTIONS = [
  '汇总当前看板进展',
  '列出阻塞和待审核的任务',
  '把「登录页报错」创建成 Bug 修复任务',
  '解释一下当前工作流的节点状态',
]

export default function AssistantPage() {
  const { wsId, workspace, role } = useOutletContext<WsCtx>()
  const { user } = useAuth()
  const nav = useNavigate()
  const qc = useQueryClient()
  const toast = useToast()
  const confirm = useConfirm()
  const isAdmin = role === 'owner' || role === 'admin'

  const [activeId, setActiveId] = useState<string | null>(null)
  const [msgs, setMsgs] = useState<ChatMsg[]>([])
  const [input, setInput] = useState('')
  const [busy, setBusy] = useState(false)
  const [thinking, setThinking] = useState(false)
  const [status, setStatus] = useState('')
  const [settingsOpen, setSettingsOpen] = useState(false)

  const bottomRef = useRef<HTMLDivElement>(null)
  const abortRef = useRef<AbortController | null>(null)
  const newChatRef = useRef(false)
  const initializedRef = useRef(false)

  // 离开页面时中断本地 SSE；后端任务会继续生成，回来时通过状态轮询补齐结果
  useEffect(() => () => { abortRef.current?.abort() }, [])

  const { data: sessions = [] } = useQuery({
    queryKey: ['assistant-sessions', wsId],
    queryFn: () => get<AssistantSession[]>(`/workspaces/${wsId}/assistant/sessions`),
  })
  const { data: agents = [] } = useQuery({
    queryKey: ['agents', wsId],
    queryFn: () => get<Agent[]>(`/workspaces/${wsId}/agents`),
  })
  const { data: assistantStatus } = useQuery({
    queryKey: ['assistant-status', activeId],
    queryFn: () => get<{ generating: boolean }>(`/assistant/sessions/${activeId}/status`),
    enabled: !!activeId,
    refetchInterval: 2500,
  })
  const generating = !!assistantStatus?.generating
  const prevGenerating = useRef(false)
  useEffect(() => {
    if (prevGenerating.current && !generating && activeId) {
      qc.invalidateQueries({ queryKey: ['assistant-messages', activeId] })
      qc.invalidateQueries({ queryKey: ['assistant-sessions', wsId] })
    }
    prevGenerating.current = generating
  }, [generating, activeId])
  const { data: clis = [] } = useQuery({
    queryKey: ['clis', wsId],
    queryFn: () => get<CliEntry[]>(`/cli-entries?workspaceId=${wsId}`),
  })
  const { data: messages } = useQuery({
    queryKey: ['assistant-messages', activeId],
    queryFn: () => get<AssistantMessage[]>(`/assistant/sessions/${activeId}/messages`),
    enabled: !!activeId,
    refetchInterval: (busy || generating) ? 1500 : false,
  })
  const { data: boardData } = useQuery({
    queryKey: ['board', wsId],
    queryFn: () => get<{ board: Board; lanes: unknown[] }>(`/workspaces/${wsId}/board`),
  })
  const boardId = boardData?.board.id
  const { data: tasks = [] } = useQuery({
    queryKey: ['tasks', boardId],
    queryFn: () => get<Task[]>(`/tasks?boardId=${boardId}`),
    enabled: !!boardId,
    refetchInterval: 20000,
  })

  const settings = parseJson<{ assistantAgentId?: string }>(workspace.settings, {})
  const assistantAgent = agents.find(a => a.id === settings.assistantAgentId) || agents[0]
  const displayName = user?.displayName || user?.username || '你'

  // 首次进入自动打开最近会话
  useEffect(() => {
    if (initializedRef.current || sessions.length === 0) return
    initializedRef.current = true
    if (!newChatRef.current) setActiveId(sessions[0].id)
  }, [sessions])

  // 切换会话时载入历史（流式期间不覆盖本地消息）
  useEffect(() => {
    if (!messages || busy) return
    setMsgs(messages.flatMap<ChatMsg>(m => {
      if (m.role === 'process') {
        try {
          const p = JSON.parse(m.content) as { thoughts?: unknown; tools?: unknown }
          const thoughts = typeof p.thoughts === 'string' ? p.thoughts : ''
          const tools = Array.isArray(p.tools) ? p.tools as ToolCallView[] : []
          if (!thoughts.trim() && tools.length === 0) return []
          return [{ role: 'process' as const, thoughts, tools, createdAt: m.createdAt }]
        } catch {
          return []
        }
      }
      return [{ role: m.role, content: m.content, createdAt: m.createdAt }]
    }))
  }, [messages, activeId])

  // 滚动到底（流式时即时跟随）
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: 'end' })
  }, [msgs])

  const abortCurrent = () => {
    const sid = activeId
    abortRef.current?.abort()
    abortRef.current = null
    setBusy(false)
    setThinking(false)
    setStatus('')
    if (sid) {
      // 后端可能仍在生成并最终落库：稍后自动拉回结果，避免“停止后永远看不到回答”
      for (const ms of [3000, 10000, 30000]) {
        setTimeout(() => {
          qc.invalidateQueries({ queryKey: ['assistant-messages', sid] })
          qc.invalidateQueries({ queryKey: ['assistant-sessions', wsId] })
        }, ms)
      }
    }
  }

  const newChat = () => {
    abortCurrent()
    newChatRef.current = true
    setActiveId(null)
    setMsgs([])
    setInput('')
  }

  const openSession = (id: string) => {
    if (id === activeId) return
    abortCurrent()
    newChatRef.current = false
    setActiveId(id)
    setMsgs([])
  }

  const send = async (preset?: string) => {
    const text = (preset ?? input).trim()
    if (!text || busy) return
    setInput('')
    setMsgs(prev => [
      ...prev,
      { role: 'user', content: text, createdAt: new Date().toISOString() },
      { role: 'process', thoughts: '', tools: [] },
      { role: 'assistant', content: '' },
    ])
    setBusy(true)
    setThinking(true)
    setStatus('正在连接智能体…')
    const ctrl = new AbortController()
    abortRef.current = ctrl
    let sessionId = activeId
    try {
      const res = await fetch(`${API}/assistant/chat`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${getToken()}` },
        body: JSON.stringify({ workspaceId: wsId, message: text, sessionId: activeId || undefined }),
        signal: ctrl.signal,
      })
      if (!res.ok || !res.body) {
        const err = await res.json().catch(() => null)
        throw new Error(err?.error || `请求失败 (${res.status})`)
      }
      const reader = res.body.getReader()
      const dec = new TextDecoder()
      let buf = ''
      for (;;) {
        const { done, value } = await reader.read()
        if (done) break
        buf += dec.decode(value, { stream: true })
        const parts = buf.split('\n\n')
        buf = parts.pop() || ''
        for (const part of parts) {
          const line = part.split('\n').find(l => l.startsWith('data: '))
          if (!line) continue
          try {
            const ev = JSON.parse(line.slice(6))
            if (ev.type === 'session') {
              newChatRef.current = false
              sessionId = ev.sessionId
              setActiveId(ev.sessionId)
              setStatus('会话已创建，正在等待响应…')
              qc.invalidateQueries({ queryKey: ['assistant-sessions', wsId] })
            } else if (ev.type === 'chunk') {
              setThinking(false)
              setStatus('')
              setMsgs(prev => {
                const copy = [...prev]
                const idx = copy.map(m => m.role).lastIndexOf('assistant')
                if (idx >= 0) copy[idx] = { ...copy[idx], content: (copy[idx].content || '') + ev.text }
                return copy
              })
            } else if (ev.type === 'thought') {
              setMsgs(prev => {
                const copy = [...prev]
                const idx = copy.map(m => m.role).lastIndexOf('process')
                if (idx >= 0) copy[idx] = { ...copy[idx], thoughts: (copy[idx].thoughts || '') + ev.text }
                return copy
              })
            } else if (ev.type === 'tool') {
              setMsgs(prev => {
                const copy = [...prev]
                const idx = copy.map(m => m.role).lastIndexOf('process')
                if (idx >= 0) {
                  const tools = [...(copy[idx].tools || [])]
                  const ti = tools.findIndex(t => t.id === ev.id)
                  if (ti >= 0) {
                    const cur = tools[ti]
                    tools[ti] = {
                      ...cur,
                      title: ev.title || cur.title,
                      status: ev.status || cur.status,
                      kind: ev.kind || cur.kind,
                      detail: ev.detail || cur.detail,
                    }
                  } else if (ev.id) {
                    tools.push({ id: ev.id, title: ev.title || '工具', status: ev.status || 'running', kind: ev.kind, detail: ev.detail })
                  }
                  copy[idx] = { ...copy[idx], tools }
                }
                return copy
              })
            } else if (ev.type === 'status') {
              setThinking(true)
              setStatus(ev.text || '')
            } else if (ev.type === 'error') {
              setThinking(false)
              setStatus('')
              setMsgs(prev => {
                const copy = [...prev]
                const last = copy[copy.length - 1]
                if (last?.role === 'assistant') copy[copy.length - 1] = { ...last, content: `${last.content || ''}\n\n> 出错：${ev.message}` }
                return copy
              })
            }
          } catch { /* 忽略非 JSON 行 */ }
        }
      }
      qc.invalidateQueries({ queryKey: ['assistant-sessions', wsId] })
      if (sessionId) qc.invalidateQueries({ queryKey: ['assistant-messages', sessionId] })
    } catch (e: any) {
      if (e?.name === 'AbortError') {
        setMsgs(prev => {
          const copy = [...prev]
          const last = copy[copy.length - 1]
          if (last?.role === 'assistant' && !last.content) copy[copy.length - 1] = { ...last, content: '（已停止生成）' }
          return copy
        })
      } else {
        setMsgs(prev => {
          const copy = [...prev]
          const last = copy[copy.length - 1]
          if (last?.role === 'assistant') copy[copy.length - 1] = { ...last, content: last.content || `错误：${e.message}` }
          return copy
        })
      }
    } finally {
      setBusy(false)
      setThinking(false)
      setStatus('')
      abortRef.current = null
    }
  }

  const removeSession = async (id: string) => {
    const ok = await confirm({ title: '删除会话', content: '该会话的全部消息将被删除，且不可恢复。', danger: true, okText: '删除' })
    if (!ok) return
    try {
      await del(`/assistant/sessions/${id}`)
      if (activeId === id) newChat()
      qc.invalidateQueries({ queryKey: ['assistant-sessions', wsId] })
      toast('会话已删除', 'success')
    } catch (e: any) { toast(e.message, 'error') }
  }

  const copyText = async (text: string) => {
    try { await navigator.clipboard.writeText(text); toast('已复制', 'success') }
    catch { toast('复制失败，请手动选择文本', 'error') }
  }

  const grouped = useMemo(() => groupSessions(sessions), [sessions])
  const stats = useMemo(() => {
    const count = (s: TaskStatus) => tasks.filter(t => t.status === s).length
    return { total: tasks.length, doing: count('in_progress'), review: count('review'), blocked: count('blocked') }
  }, [tasks])
  const focusTasks = useMemo(
    () => tasks.filter(t => t.status === 'blocked' || t.status === 'review').slice(0, 5),
    [tasks],
  )
  const doingTasks = useMemo(() => tasks.filter(t => t.status === 'in_progress').slice(0, 5), [tasks])
  const lastAssistantIdx = msgs.map(m => m.role).lastIndexOf('assistant')
  const lastProcessIdx = msgs.map(m => m.role).lastIndexOf('process')

  return (
    <Box sx={{ height: '100%', display: 'flex', overflow: 'hidden', bgcolor: 'background.default' }}>
      {/* 会话列表 */}
      <Box sx={{
        width: 264, flexShrink: 0, borderRight: '1px solid', borderColor: 'divider', bgcolor: 'background.paper',
        display: { xs: 'none', md: 'flex' }, flexDirection: 'column', minHeight: 0,
      }}>
        <Box sx={{ p: 2 }}>
          <Button fullWidth variant="outlined" startIcon={<AddCommentOutlinedIcon />} onClick={newChat}>新对话</Button>
        </Box>
        <Divider />
        <Box sx={{ flexGrow: 1, overflowY: 'auto', px: 1, py: 1 }}>
          {sessions.length === 0 && (
            <Typography variant="caption" color="text.secondary" sx={{ p: 2, display: 'block' }}>还没有对话</Typography>
          )}
          {grouped.map(group => (
            <Box key={group.label} sx={{ mb: 1.25 }}>
              <Typography variant="caption" color="text.secondary" sx={{ px: 1.25, py: 0.75, display: 'block' }}>{group.label}</Typography>
              {group.items.map(s => (
                <ListItemButton key={s.id} selected={s.id === activeId} onClick={() => openSession(s.id)}
                  sx={{ borderRadius: 1.25, mb: 0.25, pr: 0.5, alignItems: 'flex-start' }}>
                  <Box sx={{ minWidth: 0, flexGrow: 1 }}>
                    <Typography variant="body2" noWrap fontWeight={s.id === activeId ? 650 : 500}>{s.title || '新对话'}</Typography>
                    <Typography variant="caption" color="text.secondary" noWrap display="block">
                      {s.preview || `${s.messageCount} 条消息`}
                    </Typography>
                  </Box>
                  <Tooltip title="删除会话">
                    <IconButton size="small" sx={{ opacity: 0.5, '&:hover': { opacity: 1 } }}
                      onClick={e => { e.stopPropagation(); void removeSession(s.id) }}>
                      <DeleteOutlineIcon sx={{ fontSize: 15 }} />
                    </IconButton>
                  </Tooltip>
                </ListItemButton>
              ))}
            </Box>
          ))}
        </Box>
        <Divider />
        <Box sx={{ p: 2, display: 'flex', alignItems: 'center', gap: 1.25 }}>
          <Box sx={{ width: 30, height: 30, borderRadius: 1, bgcolor: 'action.selected', color: 'primary.main', display: 'grid', placeItems: 'center', flexShrink: 0 }}>
            <SmartToyOutlinedIcon sx={{ fontSize: 17 }} />
          </Box>
          <Box sx={{ minWidth: 0, flexGrow: 1 }}>
            <Typography variant="caption" fontWeight={650} noWrap display="block">{assistantAgent?.name || '未配置助理'}</Typography>
            <Typography variant="caption" color="text.secondary" noWrap display="block">
              {assistantAgent ? (assistantAgent.model || '默认模型') : '请先在工作区创建智能体'}
            </Typography>
          </Box>
          {isAdmin && (
            <Tooltip title="助理设置">
              <IconButton size="small" onClick={() => setSettingsOpen(true)}><SettingsOutlinedIcon sx={{ fontSize: 15 }} /></IconButton>
            </Tooltip>
          )}
        </Box>
      </Box>

      {/* 对话主区 */}
      <Box sx={{ flexGrow: 1, minWidth: 0, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
        <Box sx={{ height: 66, px: { xs: 2, md: 3 }, display: 'flex', alignItems: 'center', gap: 1.5, bgcolor: 'background.paper', borderBottom: '1px solid', borderColor: 'divider', flexShrink: 0 }}>
          <Box sx={{ width: 34, height: 34, borderRadius: 1.5, bgcolor: 'primary.main', color: 'primary.contrastText', display: 'grid', placeItems: 'center' }}>
            <AutoAwesomeIcon sx={{ fontSize: 19 }} />
          </Box>
          <Box>
            <Typography variant="subtitle2" fontWeight={700}>Fux 助理</Typography>
            <Typography variant="caption" color="text.secondary">
              {activeId ? (sessions.find(s => s.id === activeId)?.title || '对话') : '新对话'} · 可通过 MCP 操作工作台
            </Typography>
          </Box>
          <Box sx={{ flexGrow: 1 }} />
          {isAdmin && (
            <Tooltip title="助理设置">
              <IconButton onClick={() => setSettingsOpen(true)}><SettingsOutlinedIcon fontSize="small" /></IconButton>
            </Tooltip>
          )}
          <Tooltip title="新对话"><IconButton onClick={newChat}><AddCommentOutlinedIcon fontSize="small" /></IconButton></Tooltip>
        </Box>

        <Box sx={{ flexGrow: 1, overflowY: 'auto', px: { xs: 2, md: 4 }, py: 3 }}>
          <Box sx={{ maxWidth: 860, mx: 'auto' }}>
            {msgs.length === 0 ? (
              <EmptyIntro name={displayName} onPick={s => void send(s)} />
            ) : (
              <Stack spacing={2.75}>
                {msgs.map((m, i) => {
                  if (m.role === 'process') {
                    return <ProcessBlock key={i} thoughts={m.thoughts || ''} tools={m.tools || []} live={busy && i === lastProcessIdx} />
                  }
                  if (m.role === 'user') {
                    return (
                      <Box key={i} sx={{ display: 'flex', justifyContent: 'flex-end' }}>
                        <Box sx={{
                          maxWidth: { xs: '88%', md: '72%' }, px: 2, py: 1.25,
                          borderRadius: '16px 16px 4px 16px', bgcolor: 'primary.main', color: 'primary.contrastText',
                          fontSize: 14, lineHeight: 1.7, whiteSpace: 'pre-wrap', wordBreak: 'break-word',
                        }}>{m.content}</Box>
                      </Box>
                    )
                  }
                  const isLastAssistant = i === lastAssistantIdx
                  return (
                    <Box key={i} sx={{ maxWidth: 900 }}>
                      {m.content ? <Markdown text={m.content} /> : <TypingDots />}
                      {busy && isLastAssistant && m.content && (
                        <Box component="span" sx={{
                          display: 'inline-block', width: 7, height: 15, ml: 0.5, bgcolor: 'primary.main', verticalAlign: '-3px',
                          animation: 'cursorBlink 1s steps(1) infinite', '@keyframes cursorBlink': { '50%': { opacity: 0 } },
                        }} />
                      )}
                      {m.content && (
                        <Box sx={{ display: 'flex', justifyContent: 'flex-end', mt: 0.25 }}>
                          <Tooltip title="复制回答">
                            <IconButton size="small" onClick={() => void copyText(m.content || '')} sx={{ p: 0.5 }}>
                              <ContentCopyIcon sx={{ fontSize: 13 }} />
                            </IconButton>
                          </Tooltip>
                        </Box>
                      )}
                    </Box>
                  )
                })}
                {(thinking || generating) && (
                  <Box sx={{ display: 'flex', gap: 1.25, alignItems: 'center', color: 'text.secondary', pl: 0.5 }}>
                    <CircularProgress size={14} />
                    <Typography variant="caption">{status || (generating ? '正在生成，完成后会自动显示…' : '正在分析工作区…')}</Typography>
                  </Box>
                )}
                <div ref={bottomRef} />
              </Stack>
            )}
          </Box>
        </Box>

        <Box sx={{ px: { xs: 2, md: 4 }, py: 2, bgcolor: 'background.paper', borderTop: '1px solid', borderColor: 'divider', flexShrink: 0 }}>
          <Box sx={{ maxWidth: 860, mx: 'auto' }}>
            {msgs.length === 0 && (
              <Stack direction="row" spacing={1} sx={{ mb: 1.25, flexWrap: 'wrap', gap: 1 }}>
                {SUGGESTIONS.map(s => (
                  <Chip key={s} label={s} size="small" variant="outlined" onClick={() => void send(s)}
                    sx={{ '&:hover': { borderColor: 'primary.main', bgcolor: 'action.hover' } }} />
                ))}
              </Stack>
            )}
            <Paper variant="outlined" sx={{
              display: 'flex', alignItems: 'flex-end', gap: 0.75, p: 0.75, pl: 1, borderRadius: '16px',
              transition: 'border-color .15s, box-shadow .15s',
              '&:focus-within': { borderColor: 'primary.main', boxShadow: '0 0 0 3px rgba(99,91,255,0.12)' },
            }}>
              <TextField variant="standard" fullWidth multiline maxRows={6} placeholder="描述你的目标，或让助理查询/创建任务…"
                value={input} onChange={e => setInput(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send() } }}
                slotProps={{ input: { disableUnderline: true } }}
                sx={{ px: 1, py: 0.5, '& .MuiInputBase-root': { fontSize: 14 } }} />
              {busy ? (
                <Tooltip title="停止生成">
                  <IconButton color="error" onClick={abortCurrent} sx={{ mb: 0.25 }}><StopCircleOutlinedIcon /></IconButton>
                </Tooltip>
              ) : (
                <Tooltip title="发送">
                  <span>
                    <IconButton color="primary" disabled={!input.trim()} onClick={() => void send()} sx={{ mb: 0.25 }}>
                      <SendRoundedIcon />
                    </IconButton>
                  </span>
                </Tooltip>
              )}
            </Paper>
            <Box sx={{ display: 'flex', justifyContent: 'space-between', mt: 0.75, px: 0.5 }}>
              <Typography variant="caption" color="text.secondary">Enter 发送 · Shift + Enter 换行</Typography>
              <Typography variant="caption" color="text.secondary">{assistantAgent ? `由 ${assistantAgent.name} 驱动` : '需要先创建智能体'}</Typography>
            </Box>
          </Box>
        </Box>
      </Box>

      {/* 工作区上下文 */}
      <Box sx={{
        width: 300, flexShrink: 0, borderLeft: '1px solid', borderColor: 'divider', bgcolor: 'background.paper',
        display: { xs: 'none', lg: 'block' }, overflowY: 'auto', p: 2.25,
      }}>
        <Typography variant="overline" color="text.secondary">工作区概览</Typography>
        <Box sx={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 1, mt: 1 }}>
          <StatCard label="全部任务" value={stats.total} color="#697386" />
          <StatCard label="进行中" value={stats.doing} color="#635bff" />
          <StatCard label="待审核" value={stats.review} color="#875bf7" />
          <StatCard label="阻塞" value={stats.blocked} color="#d85160" />
        </Box>

        <SectionTitle icon={<ErrorOutlineIcon sx={{ fontSize: 15, color: 'error.main' }} />} title="需要关注" />
        {focusTasks.length === 0
          ? <Typography variant="caption" color="text.secondary">没有阻塞或待审核任务</Typography>
          : focusTasks.map(t => <TaskRow key={t.id} task={t} onClick={() => nav(`/ws/${wsId}/tasks/${t.id}`)} />)}

        <SectionTitle icon={<PlayCircleOutlineIcon sx={{ fontSize: 15, color: 'primary.main' }} />} title="进行中" />
        {doingTasks.length === 0
          ? <Typography variant="caption" color="text.secondary">当前没有运行中的任务</Typography>
          : doingTasks.map(t => <TaskRow key={t.id} task={t} onClick={() => nav(`/ws/${wsId}/tasks/${t.id}`)} />)}

        <SectionTitle icon={<InsightsOutlinedIcon sx={{ fontSize: 15, color: 'secondary.main' }} />} title="快捷指令" />
        <Stack spacing={0.5}>
          {SUGGESTIONS.map(s => (
            <Button key={s} size="small" endIcon={<ArrowForwardIcon sx={{ fontSize: 13 }} />} onClick={() => void send(s)}
              sx={{ justifyContent: 'space-between', textTransform: 'none', color: 'text.primary', bgcolor: 'action.hover', borderRadius: 1.25, px: 1.25, py: 0.75 }}>
              <Typography variant="caption" noWrap>{s}</Typography>
            </Button>
          ))}
        </Stack>

        <Divider sx={{ my: 2.25 }} />
        <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.7, display: 'block' }}>
          助理通过工作区内置 MCP 执行操作，所有任务变更都会写入事件时间线。
        </Typography>
      </Box>
      {settingsOpen && (
        <AssistantSettingsDialog
          wsId={wsId}
          agents={agents}
          clis={clis}
          settings={workspace.settings}
          currentAgentId={assistantAgent?.id}
          onClose={() => setSettingsOpen(false)}
        />
      )}
    </Box>
  )
}

/// 选择驱动 AI 助理的智能体（保存到工作区 settings.assistantAgentId）。
function AssistantSettingsDialog({ wsId, agents, clis, settings, currentAgentId, onClose }: {
  wsId: string; agents: Agent[]; clis: CliEntry[]; settings: string; currentAgentId?: string; onClose: () => void
}) {
  const nav = useNavigate()
  const qc = useQueryClient()
  const toast = useToast()
  const [agentId, setAgentId] = useState(currentAgentId || agents[0]?.id || '')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const agent = agents.find(a => a.id === agentId)
  const cli = clis.find(c => c.id === agent?.cliEntryId)

  const save = async () => {
    setBusy(true)
    setErr('')
    try {
      const base = parseJson<Record<string, unknown>>(settings, {})
      await patch(`/workspaces/${wsId}`, { settings: { ...base, assistantAgentId: agentId } })
      await qc.invalidateQueries({ queryKey: ['workspace', wsId] })
      toast('助理设置已保存', 'success')
      onClose()
    } catch (e: any) {
      setErr(e.message)
    } finally {
      setBusy(false)
    }
  }

  const row = (label: string, value: string) => (
    <Box sx={{ display: 'flex', justifyContent: 'space-between', gap: 2 }}>
      <Typography variant="caption" color="text.secondary">{label}</Typography>
      <Typography variant="caption" sx={{ textAlign: 'right', wordBreak: 'break-all' }}>{value}</Typography>
    </Box>
  )

  return (
    <Dialog open onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>助理设置</DialogTitle>
      <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
        <TextField select label="助理智能体" value={agentId} onChange={e => setAgentId(e.target.value)}
          helperText="助理会继承该智能体的 CLI、模型、推理强度、系统提示词、SKILL 与 MCP 授权。">
          {agents.map(a => {
            const c = clis.find(x => x.id === a.cliEntryId)
            return <MenuItem key={a.id} value={a.id}>{a.name} · {c?.name || a.cliEntryId}{a.model ? ` · ${a.model}` : ''}</MenuItem>
          })}
          {agents.length === 0 && <MenuItem value="" disabled>暂无可选智能体</MenuItem>}
        </TextField>
        {agent ? (
          <Paper variant="outlined" sx={{ p: 1.5, borderRadius: 1.5 }}>
            <Stack spacing={0.75}>
              {row('CLI', cli?.name || agent.cliEntryId)}
              {row('模型', agent.model || 'CLI 默认')}
              {row('推理强度', agent.reasoningEffort)}
              {row('SKILL 授权', String(agent.skillIds?.length || 0))}
              {row('MCP 授权', String(agent.mcpServers?.length || 0))}
            </Stack>
          </Paper>
        ) : (
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
            <Typography variant="caption" color="text.secondary">还没有智能体</Typography>
            <Button size="small" onClick={() => { onClose(); nav(`/ws/${wsId}/agents`) }}>去创建智能体</Button>
          </Box>
        )}
        {err && <Alert severity="error">{err}</Alert>}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>取消</Button>
        <Button variant="contained" onClick={() => void save()} disabled={busy || !agentId}>保存</Button>
      </DialogActions>
    </Dialog>
  )
}

function StatCard({ label, value, color }: { label: string; value: number; color: string }) {
  return (
    <Paper elevation={0} sx={{ p: 1.25, borderRadius: 1.25, border: '1px solid', borderColor: 'divider' }}>
      <Typography variant="h6" sx={{ color, lineHeight: 1.2 }}>{value}</Typography>
      <Typography variant="caption" color="text.secondary">{label}</Typography>
    </Paper>
  )
}

function SectionTitle({ icon, title }: { icon: React.ReactNode; title: string }) {
  return (
    <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75, mt: 2.5, mb: 1 }}>
      {icon}
      <Typography variant="caption" fontWeight={700}>{title}</Typography>
    </Box>
  )
}

function TaskRow({ task, onClick }: { task: Task; onClick: () => void }) {
  return (
    <Box onClick={onClick} sx={{
      px: 1.25, py: 0.9, mb: 0.5, borderRadius: 1.25, border: '1px solid', borderColor: 'divider',
      cursor: 'pointer', transition: 'background-color .15s', '&:hover': { bgcolor: 'action.hover' },
    }}>
      <Typography variant="caption" fontWeight={600} noWrap display="block">{task.title}</Typography>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75, mt: 0.5 }}>
        <StatusChip status={task.status} />
        <Typography variant="caption" color="text.secondary" noWrap>#{task.id.slice(0, 6)}</Typography>
      </Box>
    </Box>
  )
}

function EmptyIntro({ name, onPick }: { name: string; onPick: (s: string) => void }) {
  return (
    <Box sx={{ textAlign: 'center', py: { xs: 4, md: 7 } }}>
      <Box sx={{
        width: 68, height: 68, mx: 'auto', mb: 2.5, borderRadius: 1.5, color: '#fff',
        background: 'linear-gradient(135deg,#635bff,#9b8cff)', display: 'grid', placeItems: 'center',
        boxShadow: '0 12px 28px rgba(99,91,255,0.25)',
      }}>
        <AutoAwesomeIcon sx={{ fontSize: 32 }} />
      </Box>
      <Typography variant="h5" fontWeight={750}>你好，{name}</Typography>
      <Typography variant="body2" color="text.secondary" sx={{ mt: 1, mb: 3.5, maxWidth: 460, mx: 'auto' }}>
        我可以查询看板、创建和推进任务、解释工作流状态。直接描述目标即可。
      </Typography>
      <Box sx={{ display: 'grid', gap: 1.25, gridTemplateColumns: { xs: '1fr', sm: 'repeat(3, 1fr)' }, maxWidth: 720, mx: 'auto', mb: 3.5 }}>
        {[
          { icon: <InsightsOutlinedIcon />, t: '查询进展', d: '汇总看板、阻塞与审核情况' },
          { icon: <TaskAltIcon />, t: '创建任务', d: '用一句话生成规范任务卡' },
          { icon: <PlayCircleOutlineIcon />, t: '推进流程', d: '发起工作流并跟踪节点' },
        ].map(c => (
          <Paper key={c.t} elevation={0} sx={{ p: 2, borderRadius: 1.5, border: '1px solid', borderColor: 'divider', textAlign: 'left' }}>
            <Box sx={{ color: 'primary.main', mb: 1 }}>{c.icon}</Box>
            <Typography variant="subtitle2" fontWeight={700}>{c.t}</Typography>
            <Typography variant="caption" color="text.secondary">{c.d}</Typography>
          </Paper>
        ))}
      </Box>
      <Stack direction="row" spacing={1} sx={{ flexWrap: 'wrap', justifyContent: 'center', gap: 1 }}>
        {SUGGESTIONS.slice(0, 3).map(s => (
          <Chip key={s} label={s} variant="outlined" onClick={() => onPick(s)}
            sx={{ '&:hover': { borderColor: 'primary.main', bgcolor: 'action.hover' } }} />
        ))}
      </Stack>
    </Box>
  )
}

function TypingDots() {
  return (
    <Box sx={{ display: 'inline-flex', alignItems: 'center', gap: 0.5, py: 0.5 }}>
      {[0, 1, 2].map(i => (
        <Box key={i} sx={{
          width: 6, height: 6, borderRadius: '50%', bgcolor: 'text.disabled',
          animation: 'dotPulse 1.2s ease-in-out infinite', animationDelay: `${i * 0.15}s`,
          '@keyframes dotPulse': { '0%,80%,100%': { opacity: 0.3, transform: 'translateY(0)' }, '40%': { opacity: 1, transform: 'translateY(-3px)' } },
        }} />
      ))}
    </Box>
  )
}

function ProcessBlock({ thoughts, tools, live }: { thoughts: string; tools: ToolCallView[]; live: boolean }) {
  const [open, setOpen] = useState(live)
  useEffect(() => { if (live) setOpen(true) }, [live])
  const hasThoughts = thoughts.trim().length > 0
  if (!hasThoughts && tools.length === 0) return null
  return (
    <Box sx={{ maxWidth: 900 }}>
      <Box onClick={() => setOpen(v => !v)} sx={{
        display: 'inline-flex', alignItems: 'center', gap: 0.75, cursor: 'pointer',
        color: 'text.secondary', py: 0.25, '&:hover': { color: 'text.primary' },
      }}>
        <PsychologyOutlinedIcon sx={{ fontSize: 16 }} />
        <Typography variant="caption" fontWeight={650}>执行过程</Typography>
        {hasThoughts && <Chip size="small" variant="outlined" label="思考" sx={{ height: 18, fontSize: 10 }} />}
        {tools.length > 0 && <Chip size="small" variant="outlined" label={`${tools.length} 个工具`} sx={{ height: 18, fontSize: 10 }} />}
        {live && <CircularProgress size={10} />}
        <ExpandMoreIcon sx={{ fontSize: 16, transition: 'transform .15s', transform: open ? 'rotate(180deg)' : 'none' }} />
      </Box>
      <Collapse in={open} timeout="auto" unmountOnExit>
        <Box sx={{
          mt: 0.75, pl: 1.5, ml: 0.5, borderLeft: '2px solid', borderColor: 'divider',
          display: 'grid', gap: 1.25,
        }}>
          {hasThoughts && (
            <Box>
              <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5, color: 'text.secondary', mb: 0.5 }}>
                <PsychologyOutlinedIcon sx={{ fontSize: 13 }} />
                <Typography variant="caption" fontWeight={650}>思考</Typography>
                {live && <CircularProgress size={9} />}
              </Box>
              <Box component="pre" sx={{
                m: 0, pl: 1.25, borderLeft: '1px dashed', borderColor: 'divider',
                fontFamily: 'inherit', fontSize: 12.5, lineHeight: 1.7, color: 'text.secondary',
                whiteSpace: 'pre-wrap', wordBreak: 'break-word', maxHeight: 260, overflowY: 'auto',
              }}>{thoughts}</Box>
            </Box>
          )}
          {tools.length > 0 && (
            <Box>
              <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5, color: 'text.secondary', mb: 0.5 }}>
                <BuildOutlinedIcon sx={{ fontSize: 13 }} />
                <Typography variant="caption" fontWeight={650}>工具调用</Typography>
              </Box>
              <Box sx={{ display: 'grid', gap: 0.5 }}>
                {tools.map(t => <ToolRow key={t.id || t.title} tool={t} />)}
              </Box>
            </Box>
          )}
        </Box>
      </Collapse>
    </Box>
  )
}

function groupSessions(sessions: AssistantSession[]) {
  const now = new Date()
  const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  const startYesterday = startToday - 86400000
  const groups: { label: string; items: AssistantSession[] }[] = [
    { label: '今天', items: [] },
    { label: '昨天', items: [] },
    { label: '更早', items: [] },
  ]
  for (const s of sessions) {
    const t = new Date(s.updatedAt).getTime()
    if (t >= startToday) groups[0].items.push(s)
    else if (t >= startYesterday) groups[1].items.push(s)
    else groups[2].items.push(s)
  }
  return groups.filter(g => g.items.length > 0)
}