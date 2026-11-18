import { useRef, useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { Box, Button, Chip, CircularProgress, Paper, TextField, Typography } from '@mui/material'
import SendIcon from '@mui/icons-material/Send'
import SmartToyIcon from '@mui/icons-material/SmartToy'
import RestartAltIcon from '@mui/icons-material/RestartAlt'
import AutoAwesomeIcon from '@mui/icons-material/AutoAwesome'
import { API, getToken } from '../api'
import { useAuth } from '../auth'
import { PageHeader, PersonAvatar } from '../ui'
import type { WsCtx } from './WorkspaceLayout'

interface Msg { role: 'user' | 'assistant'; content: string }

const SUGGESTIONS = ['现在有哪些任务在阻塞？', '汇总当前看板的进展', '帮我创建一个任务']

export default function AssistantPage() {
  const { wsId } = useOutletContext<WsCtx>()
  const { user } = useAuth()
  const [msgs, setMsgs] = useState<Msg[]>([])
  const [input, setInput] = useState('')
  const [busy, setBusy] = useState(false)
  const bottomRef = useRef<HTMLDivElement>(null)

  const send = async (preset?: string) => {
    const text = (preset ?? input).trim()
    if (!text || busy) return
    setInput(''); setBusy(true)
    setMsgs(m => [...m, { role: 'user', content: text }, { role: 'assistant', content: '' }])
    try {
      const res = await fetch(`${API}/assistant/chat`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${getToken()}` },
        body: JSON.stringify({ workspaceId: wsId, message: text }),
      })
      if (!res.ok || !res.body) {
        const e = await res.json().catch(() => null)
        throw new Error(e?.error || `请求失败 (${res.status})`)
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
        for (const p of parts) {
          const line = p.split('\n').find(l => l.startsWith('data: '))
          if (!line) continue
          try {
            const ev = JSON.parse(line.slice(6))
            if (ev.type === 'chunk') {
              setMsgs(m => {
                const copy = [...m]
                copy[copy.length - 1] = { role: 'assistant', content: copy[copy.length - 1].content + ev.text }
                return copy
              })
            } else if (ev.type === 'error') {
              setMsgs(m => {
                const copy = [...m]
                copy[copy.length - 1] = { role: 'assistant', content: `⚠️ ${ev.message}` }
                return copy
              })
            }
          } catch { /* ignore */ }
        }
        bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
      }
    } catch (e: any) {
      setMsgs(m => {
        const copy = [...m]
        copy[copy.length - 1] = { role: 'assistant', content: `⚠️ ${e.message}` }
        return copy
      })
    } finally {
      setBusy(false)
      bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
    }
  }

  return (
    <Box sx={{ height: '100%', p: { xs: 2, md: 3.5 }, display: 'flex', justifyContent: 'center', overflow: 'hidden' }}>
      <Box sx={{ width: '100%', maxWidth: 1040, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
        <PageHeader title="AI 助理" desc="用自然语言查询看板、创建和更新任务。助理通过工作区内置 MCP 安全地完成操作。"
          actions={msgs.length > 0 ? <Button variant="outlined" startIcon={<RestartAltIcon />} disabled={busy} onClick={() => setMsgs([])}>新对话</Button> : undefined} />
        <Paper sx={{ flexGrow: 1, minHeight: 0, display: 'flex', flexDirection: 'column', border: '1px solid', borderColor: 'divider', borderRadius: 4, overflow: 'hidden' }}>
          <Box sx={{ flexGrow: 1, overflowY: 'auto', p: { xs: 2, md: 4 } }}>
            {msgs.length === 0 ? (
              <Box sx={{ minHeight: '100%', display: 'grid', placeContent: 'center', justifyItems: 'center', textAlign: 'center', py: 4 }}>
                <Box sx={{ width: 64, height: 64, borderRadius: 5, display: 'grid', placeItems: 'center', mb: 2.5, bgcolor: 'rgba(139,157,255,0.12)', color: 'primary.light', border: '1px solid rgba(139,157,255,0.2)' }}>
                  <AutoAwesomeIcon sx={{ fontSize: 30 }} />
                </Box>
                <Typography variant="h6">今天想推进什么？</Typography>
                <Typography variant="body2" color="text.secondary" sx={{ mt: 1, mb: 3, maxWidth: 460 }}>直接描述你的目标，或从下面一个问题开始</Typography>
                <Box sx={{ display: 'flex', gap: 1, flexWrap: 'wrap', justifyContent: 'center' }}>
                  {SUGGESTIONS.map(s => <Chip key={s} label={s} variant="outlined" onClick={() => void send(s)} sx={{ px: 0.5, py: 2, '&:hover': { borderColor: 'primary.main', bgcolor: 'action.selected' } }} />)}
                </Box>
              </Box>
            ) : (
              <Box sx={{ display: 'flex', flexDirection: 'column', gap: 3.5 }}>
                {msgs.map((m, i) => (
                  <Box key={i} sx={{ display: 'flex', gap: 1.5, alignItems: 'flex-start', flexDirection: m.role === 'user' ? 'row-reverse' : 'row' }}>
                    {m.role === 'assistant'
                      ? <Box sx={{ width: 34, height: 34, borderRadius: 2.5, bgcolor: 'rgba(139,157,255,0.12)', color: 'primary.light', display: 'grid', placeItems: 'center', flexShrink: 0 }}><SmartToyIcon sx={{ fontSize: 19 }} /></Box>
                      : <PersonAvatar name={user?.displayName || user?.username} size={34} />}
                    <Box sx={{ maxWidth: '82%', minWidth: 0 }}>
                      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 0.75, textAlign: m.role === 'user' ? 'right' : 'left' }}>{m.role === 'user' ? '你' : 'Fux 助理'}</Typography>
                      <Paper sx={{
                        p: 1.75, px: 2, borderRadius: 3, whiteSpace: 'pre-wrap', wordBreak: 'break-word',
                        bgcolor: m.role === 'user' ? 'rgba(139,157,255,0.14)' : 'rgba(148,163,184,0.055)',
                        border: '1px solid', borderColor: m.role === 'user' ? 'rgba(139,157,255,0.22)' : 'divider',
                      }}>
                        <Typography variant="body2">{m.content || (busy && i === msgs.length - 1 ? <Box component="span" sx={{ display: 'inline-flex', alignItems: 'center', gap: 1 }}><CircularProgress size={14} />正在思考…</Box> : '')}</Typography>
                      </Paper>
                    </Box>
                  </Box>
                ))}
              </Box>
            )}
            <div ref={bottomRef} />
          </Box>
          <Box sx={{ p: 2, borderTop: '1px solid', borderColor: 'divider', bgcolor: 'rgba(255,255,255,0.015)' }}>
            <Box sx={{ display: 'flex', gap: 1.25, alignItems: 'flex-end' }}>
              <TextField fullWidth multiline maxRows={5} placeholder="给 AI 助理发送消息…" value={input} onChange={e => setInput(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send() } }} disabled={busy} />
              <Button variant="contained" endIcon={<SendIcon />} onClick={() => void send()} disabled={busy || !input.trim()} sx={{ height: 42, px: 2.5, minWidth: 100, flexShrink: 0, whiteSpace: 'nowrap' }}>发送</Button>
            </Box>
            <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 1, px: 0.5 }}>Enter 发送 · Shift + Enter 换行</Typography>
          </Box>
        </Paper>
      </Box>
    </Box>
  )
}
