import '@testing-library/jest-dom/vitest'
import { cleanup } from '@testing-library/react'
import { afterEach } from 'vitest'

afterEach(() => cleanup())
import { setLang } from '../i18n'

// 테스트는 한국어 화면 기준 (jsdom 의 navigator.language 는 en-US)
setLang('ko')
