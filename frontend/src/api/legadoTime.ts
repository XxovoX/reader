import { invokeUpload } from './invoke'
import { FILE_NAME_HEADER, encodeFileName } from './uploadHeader'

export interface MergedReadingTimeBackup {
  filename: string
  base64: string
  macSeconds: number
  deviceId: string
}

export function mergeLegadoReadingTime(file: File) {
  if (file.size > 128 * 1024 * 1024) throw new Error('备份不能超过 128MB')
  return invokeUpload<MergedReadingTimeBackup>('merge_legado_reading_time', {
    [FILE_NAME_HEADER]: encodeFileName(file.name),
  }, file)
}

export function downloadMergedBackup(result: MergedReadingTimeBackup) {
  const bytes = Uint8Array.from(atob(result.base64), char => char.charCodeAt(0))
  const url = URL.createObjectURL(new Blob([bytes], { type: 'application/zip' }))
  const link = document.createElement('a')
  link.href = url
  link.download = result.filename
  link.click()
  setTimeout(() => URL.revokeObjectURL(url), 60_000)
}
