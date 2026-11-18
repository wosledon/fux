import { useRef, useState } from 'react'
import { Drawer, Box, Typography, TextField, IconButton, Paper, CircularProgress } from '@mui/material'
import SendIcon from '@mui/icons-material/Send'
import { API, getToken } from '../api'

interface Msg { role: 'user' | 'assistant'; content: string }

export default function AssistantPanel({ wsId, open, onClose }: { wsId: string; open: boolean; onClose: () => void }) {
  const [msgs, setMsgs] = useState<Msg[]>([])
  const [input, setInput] = useState('')
  const [busy, setBusy] = useState(false)
  const bottomRef = useRef<HTMLDivElement>(null)

  const send = async () => {
    const text = input.trim()
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
    <Drawer anchor="right" open={open} onClose={onClose} PaperProps={{ sx: { width: 420, display: 'flex', flexDirection: 'column' } }}>
      <Box sx={{ p: 2, borderBottom: '1px solid', borderColor: 'divider' }}>
        <Typography fontWeight={700}>AI 工作助理</Typography>
        <Typography variant="caption" color="text.secondary">可创建任务、移动卡片、评论、查询看板（经内置 workspace MCP）</Typography>
      </Box>
      <Box sx={{ flexGrow: 1, overflow: 'auto', p: 2, display: 'flex', flexDirection: 'column', gap: 1.5 }}>
        {msgs.length === 0 && <Typography color="text.secondary" variant="body2">试试：「把登录页的 bug 建个任务」「现在有哪些任务在阻塞？」</Typography>}
        {msgs.map((m, i) => (
          <Paper key={i} elevation={0} sx={{ p: 1.5, alignSelf: m.role === 'user' ? 'flex-end' : 'flex-start', maxWidth: '92%', bgcolor: m.role === 'user' ? 'primary.dark' : 'background.default', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
            <Typography variant="body2">{m.content || (busy && i === msgs.length - 1 ? '…' : '')}</Typography>
          </Paper>
        ))}
        <div ref={bottomRef} />
      </Box>
      <Box sx={{ p: 2, borderTop: '1px solid', borderColor: 'divider', display: 'flex', gap: 1 }}>
        <TextField fullWidth placeholder="输入消息…" value={input} onChange={e => setInput(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && !e.shiftKey && send()} disabled={busy} />
        <IconButton color="primary" onClick={send} disabled={busy || !input.trim()}>
          {busy ? <CircularProgress size={20} /> : <SendIcon />}
        </IconButton>
      </Box>
    </Drawer>
  )
}