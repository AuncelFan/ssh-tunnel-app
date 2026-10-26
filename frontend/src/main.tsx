import React from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import './style.css'
import { LocaleProvider } from './locale'

createRoot(document.getElementById('root')!).render(<React.StrictMode><LocaleProvider><App /></LocaleProvider></React.StrictMode>)
