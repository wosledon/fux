import { useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { Box, Tab, Tabs, TextField, Button, Typography, Alert, Stack, Chip } from '@mui/material'
import ViewKanbanIcon from '@mui/icons-material/ViewKanban'
import SmartToyIcon from '@mui/icons-material/SmartToy'
import HubIcon from '@mui/icons-material/Hub'
import ShieldIcon from '@mui/icons-material/Shield'
import { useAuth } from '../auth'
import { Logo } from '../ui'

const FEATURES = [
  { icon: <ViewKanbanIcon />, t: '自驱动看板', d: '任务自动认领、流转、交付，全程可视' },
  { icon: <SmartToyIcon />, t: '标准工作流', d: 'Codex / OpenCode / Pi / Copilot CLI 即插即用' },
  { icon: <HubIcon />, t: '多节点执行', d: '另一台电脑装个 runtime 即可加入' },
  { icon: <ShieldIcon />, t: '质量阀护航', d: '循环 / 停滞 / 中断自动熔断，PR/MR 强制审核' },
]

export default function LoginPage() {
  const { login, register } = useAuth()
  const nav = useNavigate()
  const [params] = useSearchParams()
  const [tab, setTab] = useState(0)
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)

  const submit = async () => {
    setErr(''); setBusy(true)
    try {
      if (tab === 0) await login(username, password)
      else await register(username, password, displayName || undefined)
      nav(params.get('next') || '/')
    } catch (e: any) { setErr(e.message) } finally { setBusy(false) }
  }

  return (
    <Box sx={{ height: '100%', display: 'flex' }}>
      <Box sx={{
        flex: 1, display: { xs: 'none', md: 'flex' }, flexDirection: 'column', justifyContent: 'center', px: 8,
        background: 'radial-gradient(900px 540px at 20% 10%, rgba(99,91,255,0.13), transparent), radial-gradient(700px 480px at 80% 90%, rgba(135,91,247,0.09), transparent), #f8f8fc',
        borderRight: '1px solid', borderColor: 'divider',
      }}>
        <Logo size={40} />
        <Typography variant="h4" sx={{ mt: 4, mb: 1.5, maxWidth: 460 }}>
          让智能体工作流<br />替你推进每一张卡片
        </Typography>
        <Typography color="text.secondary" sx={{ mb: 5, maxWidth: 420 }}>
          Fux 是一个 AI 工作台：人定义目标、评审结果；智能体认领任务、执行工作、流式交付过程。
        </Typography>
        <Stack spacing={2.5}>
          {FEATURES.map(f => (
            <Box key={f.t} sx={{ display: 'flex', gap: 2, alignItems: 'flex-start' }}>
              <Box sx={{ width: 38, height: 38, borderRadius: 1.25, display: 'grid', placeItems: 'center', bgcolor: '#eceaff', color: 'primary.main', flexShrink: 0 }}>
                {f.icon}
              </Box>
              <Box>
                <Typography fontWeight={700}>{f.t}</Typography>
                <Typography variant="body2" color="text.secondary">{f.d}</Typography>
              </Box>
            </Box>
          ))}
        </Stack>
      </Box>

      <Box sx={{ width: { xs: '100%', md: 480 }, display: 'grid', placeItems: 'center', p: 3 }}>
        <Box sx={{ width: '100%', maxWidth: 380 }}>
          <Box sx={{ display: { xs: 'block', md: 'none' }, mb: 3 }}><Logo size={34} /></Box>
          <Typography variant="h5" gutterBottom>{tab === 0 ? '欢迎回来' : '创建账号'}</Typography>
          <Typography color="text.secondary" sx={{ mb: 3 }}>
            {tab === 0 ? '登录以继续你的工作区' : '注册后即可创建工作区并邀请成员'}
          </Typography>
          <Tabs value={tab} onChange={(_, v) => { setTab(v); setErr('') }} sx={{ mb: 3 }}>
            <Tab label="登录" /><Tab label="注册" />
          </Tabs>
          <Box sx={{ display: 'grid', gap: 2 }}>
            <TextField label="用户名" value={username} onChange={e => setUsername(e.target.value)} autoFocus fullWidth />
            <TextField label="密码（≥8 位）" type="password" value={password} onChange={e => setPassword(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && submit()} fullWidth />
            {tab === 1 && <TextField label="昵称（可选）" value={displayName} onChange={e => setDisplayName(e.target.value)} fullWidth />}
            {err && <Alert severity="error">{err}</Alert>}
            <Button variant="contained" size="large" disabled={busy || !username || !password} onClick={submit} sx={{ py: 1.2 }}>
              {tab === 0 ? '登录' : '注册并登录'}
            </Button>
            <Chip label="本地部署 · 数据自持 · 无云端依赖" variant="outlined" sx={{ alignSelf: 'center', color: 'text.secondary' }} />
          </Box>
        </Box>
      </Box>
    </Box>
  )
}