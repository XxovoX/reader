<template>
  <section class="workspace">
    <div v-if="covered" class="workspace-cover">
      <div class="work-mark">›_</div>
      <h1>工作记录</h1>
      <p>当前没有待处理的任务。</p>
      <button @click="covered = false">继续</button>
    </div>
    <div v-else class="workspace-page">
      <header>
        <span class="workspace-wordmark">›_ <strong>工作区</strong></span>
        <div class="workspace-actions">
          <button @click="router.push({ name: 'home' })">返回书架</button>
          <button @click="manage = !manage">{{ manage ? '返回列表' : '管理' }}</button>
          <button @click="covered = true">隐藏 <kbd>Esc</kbd></button>
        </div>
      </header>
      <p v-if="notice" class="workspace-notice" role="status">{{ notice }}</p>
      <template v-if="!manage">
        <div class="workspace-intro"><h1>继续上次的记录</h1><p>{{ app.isWebMode ? '选择一项，在当前面板中继续。悬浮模式请打开桌面应用。' : '选择一本书，启动鼠标移入才显示的透明悬浮层。' }}</p></div>
        <p v-if="busy">正在打开…</p>
        <p v-else-if="!shelf.books.length" class="workspace-empty">列表为空。到「管理」导入本地文件，或在桌面 Reader 中导入手机书架与书源。</p>
        <div class="workspace-list">
          <button v-for="book in shelf.books" :key="book.bookUrl" :disabled="busy" @click="openBook(book)">
            <div><strong>{{ book.name }}</strong><small>{{ book.author || '未署名' }} · {{ book.durChapterTitle || '尚未开始' }}</small></div><span aria-hidden="true">↗</span>
          </button>
        </div>
        <footer>{{ app.isWebMode ? '阅读位置自动保存 · 切换窗口暂停计时' : '移出隐藏 · 滚轮阅读 · 按住 Option 拖动 · 右键打开设置' }}</footer>
      </template>
      <div v-else class="workspace-settings">
        <section>
          <h1>进度同步</h1>
          <p>填写与手机开源阅读相同的 WebDAV 地址、账号和目录。手机也需开启进度同步；两端使用同一本书和相同目录。</p>
          <form @submit.prevent="saveSync">
            <label>WebDAV 地址<input v-model.trim="dav.url" placeholder="https://dav.example.com/dav/" type="url" required></label>
            <div class="workspace-fields"><label>账号<input v-model.trim="dav.account" autocomplete="username" required></label><label>密码 / 应用密码<input v-model="dav.password" type="password" autocomplete="current-password" required></label></div>
            <label>备份目录<input v-model.trim="dav.directory" placeholder="legado"></label>
            <label class="workspace-check"><input v-model="syncEnabled" type="checkbox">开启阅读进度同步</label>
            <p class="workspace-hint">保存后，凭据保存在当前浏览器的本地数据中。浏览器和桌面应用需要分别配置。</p>
            <button class="workspace-primary" :disabled="busy" type="submit">保存设置</button>
            <button :disabled="busy" type="button" @click="testSync">测试连接</button>
          </form>
        </section>
        <section>
          <h2>合并到手机阅读时长</h2>
          <p>适配开源阅读 3.22.031722。先在手机生成最新完整备份，把 ZIP 文件传到 Mac，再选择该文件。</p>
          <ol><li>选择最新手机备份，生成合并文件。</li><li>把新文件传回手机，用开源阅读的「恢复」导入。</li></ol>
          <p class="workspace-hint">保留原备份。恢复完整备份也会恢复当时的书架和设置，请在生成新备份后尽快操作。同一 Mac 的累计时长重复导入不会重复增加。</p>
          <label class="workspace-file">选择手机备份 ZIP<input type="file" accept=".zip,application/zip" :disabled="busy" @change="mergeTime"></label>
          <div v-if="merged" class="workspace-result"><p>已生成合并文件，包含本机累计 {{ formatDuration(merged.macSeconds) }}。原备份未修改。</p><button class="workspace-primary" @click="downloadMergedBackup(merged)">下载 {{ merged.filename }}</button></div>
        </section>
        <section>
          <h2>导入手机书架</h2>
          <p>读取完整备份中的在线书架与书源，已有的同地址条目会保留。请只导入可信的书源。</p>
          <label class="workspace-file">读取手机备份<input type="file" accept=".zip" :disabled="busy" @change="previewLibrary"></label>
          <div v-if="library"><p>已读取 {{ library.books.length }} 本书、{{ library.sources.length }} 个书源。</p><button :disabled="busy" @click="importLibrary">导入缺少的在线书籍与书源</button></div>
        </section>
        <section>
          <h2>导入本地文件</h2><p>支持 TXT、EPUB；也可以直接从手机完整备份中导入在线书架与书源。手机本地书的正文需另行导入。</p>
          <label class="workspace-file">选择文件<input type="file" accept=".txt,.epub" :disabled="busy" @change="importFile"></label>
          <button @click="router.push('/')">打开普通书架</button>
        </section>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import { useAppStore } from '../stores/app'
import { useReaderStore } from '../stores/reader'
import { useBookshelfStore } from '../stores/bookshelf'
import { uploadTxtBook, uploadEpubBook, saveBooks } from '../api/bookshelf'
import { testLegadoWebdav } from '../api/webdav'
import { mergeLegadoReadingTime, downloadMergedBackup, type MergedReadingTimeBackup } from '../api/legadoTime'
import { invoke } from '@tauri-apps/api/core'
import { invokeUpload } from '../api/invoke'
import { getBookSources, saveBookSources } from '../api/source'
import type { Book, BookSource } from '../types'
const router = useRouter()
const app = useAppStore()
const reader = useReaderStore()
const shelf = useBookshelfStore()
const manage = ref(false)
const covered = ref(false)
const busy = ref(false)
const notice = ref('')
const dav = reactive({ ...app.legadoWebdavConfig })
const syncEnabled = ref(app.legadoSyncEnabled)
const library = ref<{ books: Book[]; sources: BookSource[] } | null>(null)
const merged = ref<MergedReadingTimeBackup | null>(null)
const formatDuration = (seconds: number) => `${Math.floor(seconds / 3600)} 小时 ${Math.floor(seconds % 3600 / 60)} 分 ${seconds % 60} 秒`
function showError(error: unknown) { notice.value = error instanceof Error ? error.message : String(error) }
function saveSync() {
  app.setLegadoWebdavConfig(dav)
  app.setLegadoSyncEnabled(syncEnabled.value)
  notice.value = '设置已保存；开启时会读取云端进度，阅读中及退出时保存。'
}
async function testSync() {
  busy.value = true
  try { await testLegadoWebdav(dav); notice.value = 'WebDAV 连接成功' } catch (e) { showError(e) } finally { busy.value = false }
}
async function openBook(book: Book) {
  busy.value = true
  try {
    await reader.loadBook(book)
    if (!app.isWebMode) {
      await invoke('open_hover_reader', { bookUrl: book.bookUrl })
      notice.value = '悬浮区已打开。鼠标移入显示，移开隐藏。右键可返回管理。'
    } else {
      await router.push({ name: 'reader', query: { mode: 'codex', ...(covered.value ? { covered: '1' } : {}) } })
    }
  }
  catch (e) { showError(e) } finally { busy.value = false }
}
async function mergeTime(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  busy.value = true
  merged.value = null
  try { await app.settleReadingSession(); merged.value = await mergeLegadoReadingTime(file); notice.value = '合并完成，请下载后在手机恢复。' }
  catch (e) { showError(e) } finally { busy.value = false }
}
async function importFile(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  busy.value = true
  try {
    await (/\.epub$/i.test(file.name) ? uploadEpubBook(file) : uploadTxtBook(file))
    await shelf.fetchBooks({ prefetch: false })
    notice.value = '文件已导入，返回列表即可打开。'
  } catch (e) { showError(e) } finally { busy.value = false }
}
async function previewLibrary(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  busy.value = true
  library.value = null
  try {
    if (file.size > 128 * 1024 * 1024) throw new Error('备份不能超过 128MB')
    library.value = await invokeUpload('inspect_legado_library', { 'x-reader-file-name': encodeURIComponent(file.name) }, file)
  } catch (e) { showError(e) } finally { busy.value = false }
}
async function importLibrary() {
  if (!library.value) return
  busy.value = true
  try {
    const existing = new Set((await getBookSources()).map(source => source.bookSourceUrl))
    const sources = library.value.sources.filter(source => !existing.has(source.bookSourceUrl))
    if (sources.length) await saveBookSources(sources)
    const shelfUrls = new Set(shelf.books.map(book => book.bookUrl))
    const books = library.value.books.filter(book => book.origin && book.origin !== 'loc_book' && /^https?:/.test(book.bookUrl) && !shelfUrls.has(book.bookUrl))
    if (books.length) await saveBooks(books)
    await shelf.fetchBooks({ prefetch: false })
    notice.value = `已导入 ${books.length} 本在线书籍、${sources.length} 个书源；已有条目与手机本地书未导入。`
    library.value = null
  } catch (e) { showError(e) } finally { busy.value = false }
}
function keydown(event: KeyboardEvent) { if (event.key === 'Escape') { covered.value = true; event.preventDefault() } }
onMounted(async () => { document.title = '工作记录'; window.addEventListener('keydown', keydown); try { await shelf.fetchBooks({ prefetch: false }) } catch (e) { showError(e) } })
onUnmounted(() => window.removeEventListener('keydown', keydown))
</script>

<style>
.workspace { height:100%; overflow:auto; background:#fff; color:#292929; font:14px/1.7 -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif; }
.workspace-page { max-width:800px; margin:auto; padding:24px 32px 48px; }
.workspace header { display:flex; flex-wrap:wrap; gap:12px; align-items:center; justify-content:space-between; padding-bottom:40px; }
.workspace-wordmark { font-size:17px; display:flex; gap:12px; align-items:center; }
.workspace-wordmark strong { font-size:13px; font-weight:500; }
.workspace button, .workspace-file { background:transparent; border:1px solid #e4e4e4; border-radius:7px; padding:7px 12px; color:inherit; cursor:pointer; font:inherit; }
.workspace button:hover, .workspace-file:hover { background:#f6f6f6; }
.workspace button:disabled { opacity:.5; cursor:wait; }
.workspace button:focus-visible, .workspace-file:focus-within { outline:2px solid #777; outline-offset:3px; }
.workspace-actions { display:flex; flex-wrap:wrap; gap:8px; }
.workspace kbd { font:11px/1.5 inherit; color:#888; margin-left:6px; }
.workspace h1 { font-size:21px; line-height:1.4; font-weight:550; margin:0 0 10px; }
.workspace h2 { font-size:16px; font-weight:550; margin:0 0 12px; }
.workspace p { margin:0 0 18px; color:#777; }
.workspace-intro { margin:8px 0 36px; }
.workspace-list > button { display:flex; align-items:center; justify-content:space-between; width:100%; text-align:left; border:0; border-bottom:1px solid #eee; border-radius:0; padding:18px 4px; }
.workspace-list strong { font-weight:500; display:block; }
.workspace-list small { display:block; color:#8b8b8b; font-size:12px; margin-top:4px; }
.workspace-list span { color:#aaa; }
.workspace footer { margin-top:44px; font-size:12px; color:#999; }
.workspace-settings > section { padding:0 0 32px; margin-bottom:32px; border-bottom:1px solid #ededed; }
.workspace-settings label { display:block; margin:0 0 16px; font-size:13px; }
.workspace-settings input:not([type=checkbox]):not([type=file]) { display:block; margin-top:6px; width:100%; border:1px solid #ddd; border-radius:6px; background:#fff; color:#292929; padding:9px 11px; font:inherit; box-sizing:border-box; }
.workspace-fields { display:flex; gap:16px; }.workspace-fields > label { flex:1; min-width:0; }
.workspace-check { display:flex !important; gap:8px; align-items:center; }
.workspace button.workspace-primary { background:#292929; border-color:#292929; color:white; margin-right:10px; }
.workspace .workspace-hint { font-size:12px; color:#888; }
.workspace .workspace-notice { padding:12px 14px; background:#f5f5f5; color:#444; border-radius:7px; overflow-wrap:anywhere; }
.workspace-file { display:inline-block !important; position:relative; margin-right:10px !important; overflow:hidden; }
.workspace-file input { position:absolute; inset:0; opacity:0; width:100%; cursor:pointer; }
.workspace-result { margin-top:16px; }.workspace-result button { overflow-wrap:anywhere; max-width:100%; }
.workspace-cover { max-width:736px; margin:auto; padding:90px 32px; }.work-mark { font-size:24px; color:#888; margin-bottom:32px; }
@media(max-width:500px) { .workspace-page { padding:20px; }.workspace-fields { flex-direction:column; gap:0; } }
</style>
