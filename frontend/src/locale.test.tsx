// @vitest-environment jsdom
import React from 'react'
import { afterEach, expect, test, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { api } from './api'
import { LocaleProvider, useLocale } from './locale'

vi.mock('./api', () => ({ api: { getUILocale: vi.fn(), setUILocale: vi.fn() } }))
afterEach(() => { cleanup(); localStorage.clear(); vi.resetAllMocks() })

function Settings() {
  const { locale, setLocale, reloadLocale } = useLocale()
  const [error, setError] = React.useState('')
  return <><output>{locale}</output><button onClick={() => { void setLocale('zh-CN').catch(error => setError(String(error))) }}>Chinese</button><button onClick={() => { void reloadLocale().catch(error => setError(String(error))) }}>Reload</button><span>{error}</span></>
}

test('directory changes reload native language without rewriting the destination locale', async () => {
  vi.mocked(api.getUILocale).mockResolvedValueOnce('en').mockResolvedValueOnce('zh-CN')
  render(<LocaleProvider><Settings /></LocaleProvider>)
  await waitFor(() => expect(api.getUILocale).toHaveBeenCalledOnce())
  fireEvent.click(screen.getByRole('button', { name: 'Reload' }))
  await waitFor(() => expect(screen.getByRole('status').textContent).toBe('zh-CN'))
  expect(api.setUILocale).not.toHaveBeenCalled()
})

test('native preference overrides stale WebView storage on startup', async () => {
  localStorage.setItem('ssh-tunnel.locale', 'en')
  vi.mocked(api.getUILocale).mockResolvedValue('zh-CN')
  render(<LocaleProvider><Settings /></LocaleProvider>)
  await waitFor(() => expect(screen.getByRole('status').textContent).toBe('zh-CN'))
  await waitFor(() => expect(document.documentElement.lang).toBe('zh-CN'))
})

test('failed native persistence leaves the current language unchanged', async () => {
  vi.mocked(api.getUILocale).mockResolvedValue('en')
  vi.mocked(api.setUILocale).mockRejectedValue(new Error('Cannot save UI language'))
  render(<LocaleProvider><Settings /></LocaleProvider>)
  fireEvent.click(screen.getByRole('button', { name: 'Chinese' }))
  await screen.findByText('Error: Cannot save UI language')
  expect(screen.getByRole('status').textContent).toBe('en')
})

test('late startup preference does not overwrite a user language change', async () => {
  let finish!: (value: string) => void
  vi.mocked(api.getUILocale).mockReturnValue(new Promise(resolve => { finish = resolve }))
  vi.mocked(api.setUILocale).mockResolvedValue(undefined)
  render(<LocaleProvider><Settings /></LocaleProvider>)
  fireEvent.click(screen.getByRole('button', { name: 'Chinese' }))
  await waitFor(() => expect(screen.getByRole('status').textContent).toBe('zh-CN'))
  await act(async () => { finish('en') })
  await waitFor(() => expect(document.documentElement.lang).toBe('zh-CN'))
})
