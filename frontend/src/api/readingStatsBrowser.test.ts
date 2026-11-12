import { afterEach, expect, it, vi } from 'vitest'
afterEach(() => vi.unstubAllGlobals())
it('persists reading duration through the real browser command adapter', async () => {
  vi.resetModules()
  vi.stubGlobal('window', {})
  const fetch = vi.fn(async () => new Response(JSON.stringify({ isSuccess: true, errorMsg: '', data: { saved: true } })))
  vi.stubGlobal('fetch', fetch)
  const { addReadingStats } = await import('./readingStats')
  await expect(addReadingStats({ seconds: 10, bookUrl: 'book', bookName: '测试' })).resolves.toEqual({ saved: true })
  expect(fetch).toHaveBeenCalledWith('/reader3/addReadingStats', expect.objectContaining({
    method: 'POST', body: JSON.stringify({ seconds: 10, bookUrl: 'book', bookName: '测试' }),
  }))
})
