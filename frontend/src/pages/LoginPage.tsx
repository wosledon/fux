import { useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { Box, Tab, Tabs, TextField, Button, Typography, Alert, Stack, Chip, IconButton, Tooltip } from '@mui/material'
import ViewKanbanIcon from '@mui/icons-material/ViewKanban'
import SmartToyIcon from '@mui/icons-material/SmartToy'
import HubIcon from '@mui/icons-material/Hub'
import ShieldIcon from '@mui/icons-material/Shield'
import LightModeIcon from '@mui/icons-material/LightMode'
import DarkModeIcon from '@mui/icons-material/DarkMode'
import { useAuth } from '../auth'
import { useColorMode } from '../ThemeModeProvider'
import { Logo } from '../ui'

const FEATURES = [
  { icon: <ViewKanbanIcon />, t: '自驱动看板', d: '任务自动认领、流转、交付，全程可视' },
  { icon: <SmartToyIcon />, t: '标准工作流', d: 'Codex / OpenCode / Pi / Copilot CLI 即插即用' },
  { icon: <HubIcon />, t: '多节点执行', d: '另一台电脑装个 runtime 即可加入' },
  { icon: <ShieldIcon />, t: '质量阀护航', d: '循环 / 停滞 / 中断自动熔断，PR/MR 强制审核' },
]

export default function LoginPage() {
  const { login, register } = useAuth()
  const { mode, toggleMode } = useColorMode()
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
    <Box sx={{ height: '100dvh', display: 'flex', bgcolor: 'background.paper' }}>
      <Box sx={{
        flex: 1, display: { xs: 'none', md: 'flex' }, flexDirection: 'column', justifyContent: 'center', px: { md: 6, lg: 9 },
        position: 'relative', overflow: 'hidden', bgcolor: '#151b28',
        '&::before': { content: '""', position: 'absolute', width: 620, height: 620, left: -220, top: -240, borderRadius: '50%', background: 'radial-gradient(circle, rgba(37,99,235,0.25), transparent 68%)' },
        '&::after': { content: '""', position: 'absolute', width: 520, height: 520, right: -180, bottom: -260, borderRadius: '50%', background: 'radial-gradient(circle, rgba(56,189,248,0.10), transparent 70%)' },
      }}>
        <Box sx={{ position: 'relative', zIndex: 1, maxWidth: 540 }}>
          <Logo size={38} inverted />
          <Chip label="AI-NATIVE WORKSPACE" size="small" sx={{ mt: 5, color: '#a9c5ff', bgcolor: 'rgba(96,165,250,0.10)', border: '1px solid rgba(96,165,250,0.18)', letterSpacing: '0.08em', fontSize: 10 }} />
          <Typography variant="h4" sx={{ mt: 2.5, mb: 2, maxWidth: 500, color: '#f5f7fb', fontSize: { md: 34, lg: 40 } }}>
            让智能体工作流<br />替你推进每一张卡片
          </Typography>
          <Typography sx={{ mb: 5, maxWidth: 460, color: '#9aa7b8', lineHeight: 1.8 }}>
            人定义目标、评审结果；智能体认领任务、执行工作，并实时交付完整过程。
          </Typography>
          <Stack spacing={2.25}>
            {FEATURES.map(f => (
              <Box key={f.t} sx={{ display: 'flex', gap: 1.75, alignItems: 'center' }}>
                <Box sx={{ width: 38, height: 38, borderRadius: 1.5, display: 'grid', placeItems: 'center', bgcolor: 'rgba(255,255,255,0.06)', color: '#7db4ff', flexShrink: 0 }}>
                  {f.icon}
                </Box>
                <Box>
                  <Typography fontWeight={600} sx={{ color: '#e7ebf2' }}>{f.t}</Typography>
                  <Typography variant="body2" sx={{ color: '#8f9ab0' }}>{f.d}</Typography>
                </Box>
              </Box>
            ))}
          </Stack>
        </Box>
      </Box>

      <Box sx={{ width: { xs: '100%', md: 500 }, display: 'grid', placeItems: 'center', p: 3, position: 'relative', bgcolor: 'background.paper' }}>
        <Tooltip title={mode === 'dark' ? '切换到浅色模式' : '切换到深色模式'}>
          <IconButton aria-label="切换明暗主题" onClick={toggleMode} sx={{ position: 'absolute', top: 20, right: 20 }}>
            {mode === 'dark' ? <LightModeIcon /> : <DarkModeIcon />}
          </IconButton>
        </Tooltip>
        <Box sx={{ width: '100%', maxWidth: 380 }}>
          <Box sx={{ display: { xs: 'block', md: 'none' }, mb: 3 }}><Logo size={34} /></Box>
          <Typography variant="h5" gutterBottom>{tab === 0 ? '欢迎回来' : '创建账号'}</Typography>
          <Typography color="text.secondary" sx={{ mb: 3 }}>
            {tab === 0 ? '登录以继续你的工作区' : '注册后即可创建工作区并邀请成员'}
          </Typography>
          <Tabs value={tab} onChange={(_, v) => { setTab(v); setErr('') }} variant="fullWidth" sx={{ mb: 3 }}>
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