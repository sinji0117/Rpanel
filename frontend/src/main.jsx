import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import './index.css'
import { applyTheme } from './lib/prefs'
import { ready } from './i18n'

applyTheme()

// 고른 언어의 사전을 받은 뒤 그림 (영어·한국어는 바로)
ready.then(() =>
  createRoot(document.getElementById('root')).render(
    <StrictMode>
      <App />
    </StrictMode>,
  ),
)
