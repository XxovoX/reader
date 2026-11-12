//! Merge Mac duration deltas into a complete Legado 3.22.031722 phone backup.
use crate::error::error::AppError;
use crate::service::reading_stats_service::BookStatsRow;

pub const MAX_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;

fn invalid(message: impl Into<String>) -> AppError {
    AppError::BadRequest(message.into())
}

/// Legado 3.22.031722 stores readTime in milliseconds and sums by bookName.
/// A zero-time watermark carries the last imported cumulative Mac duration.
/// See below for the old Android SUM/read-modify-write compatibility constraint.
pub fn merge_backup(
    bytes: &[u8],
    device_id: &str,
    rows: &[BookStatsRow],
) -> Result<Vec<u8>, AppError> {
    use serde_json::{json, Value};
    use std::collections::{BTreeMap, HashSet};
    use std::io::{Cursor, Read, Write};
    use zip::{write::FileOptions, ZipArchive, ZipWriter};

    if bytes.len() > MAX_ARCHIVE_BYTES || !device_id.starts_with("reader-mac-") {
        return Err(invalid("备份过大或 Mac 设备标识无效"));
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| invalid("请提供手机导出的完整 ZIP 备份"))?;
    if zip.len() > 4096 {
        return Err(invalid("备份文件数量过多"));
    }
    let mut names = HashSet::new();
    let mut records: Vec<Value> = Vec::new();
    let mut expanded = 0u64;
    for index in 0..zip.len() {
        let mut file = zip
            .by_index(index)
            .map_err(|_| invalid("备份包含不可读取或加密的文件"))?;
        let name = file.name().to_owned();
        if name.starts_with('/')
            || name.contains(['\\', ':', '\0'])
            || name.split('/').any(|part| part == ".." || part == ".")
            || !names.insert(name.clone())
        {
            return Err(invalid("备份包含重复文件或不安全的路径"));
        }
        expanded = expanded
            .checked_add(file.size())
            .ok_or_else(|| invalid("备份过大"))?;
        if file.size() > 64 * 1024 * 1024 || expanded > MAX_ARCHIVE_BYTES as u64 {
            return Err(invalid("备份解压后超过大小限制"));
        }
        // Read every entry before copying it: checks CRC, unsupported compression,
        // and truncated data. Never extract user paths onto the local filesystem.
        let mut content = Vec::new();
        (&mut file)
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut content)
            .map_err(|_| invalid("备份文件已损坏"))?;
        if content.len() as u64 != file.size() {
            return Err(invalid("备份条目大小不符"));
        }
        if name == "readRecord.json" {
            records = serde_json::from_slice(&content)
                .map_err(|_| invalid("readRecord.json 格式错误，已取消合并"))?;
        }
        if name == "bookshelf.json" || name == "bookSource.json" {
            let value: Value = serde_json::from_slice(&content)
                .map_err(|_| invalid("手机书架或书源数据已损坏"))?;
            if !value.is_array() {
                return Err(invalid("请使用手机的完整备份"));
            }
        }
    }
    if !["bookshelf.json", "bookSource.json", "config.xml"]
        .iter()
        .all(|name| names.contains(*name))
    {
        return Err(invalid(
            "需要手机的完整备份（书架、书源、config.xml），不能使用仅统计文件或 Reader 备份",
        ));
    }
    let mut identities = HashSet::new();
    for record in &records {
        let device = record
            .get("deviceId")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("阅读记录缺少 deviceId"))?;
        let book = record
            .get("bookName")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("阅读记录缺少 bookName"))?;
        let time = record
            .get("readTime")
            .and_then(Value::as_i64)
            .ok_or_else(|| invalid("阅读时长格式无效"))?;
        if time < 0 || !identities.insert((device.to_owned(), book.to_owned())) {
            return Err(invalid("阅读记录包含负数或重复的设备与书名"));
        }
    }
    let mut totals = BTreeMap::<&str, i64>::new();
    for row in rows {
        if row.seconds < 0 {
            return Err(invalid("Mac 阅读统计不能是负数"));
        }
        if row.seconds == 0 {
            continue;
        }
        if row.book_name.trim().is_empty() {
            return Err(invalid("Mac 阅读记录缺少书名"));
        }
        let time = row
            .seconds
            .checked_mul(1000)
            .ok_or_else(|| invalid("阅读时长溢出"))?;
        let total = totals.entry(&row.book_name).or_default();
        *total = total
            .checked_add(time)
            .ok_or_else(|| invalid("累计阅读时长溢出"))?;
    }
    // 3.22.031722 ReadBook.resetData reads SUM(all devices), but writes it
    // back to deviceId="". A positive separate Mac record would be counted
    // again on the next phone session. Add only the new delta to that native
    // record, and encode an idempotency watermark in a ZERO-time device record.
    // The watermark survives Android restore -> backup without polluting sums.
    let marker_prefix = format!("{device_id}@");
    for (name, time) in totals {
        let mut previous = 0;
        for record in &records {
            if record["bookName"] != name {
                continue;
            }
            if let Some(marker) = record["deviceId"]
                .as_str()
                .and_then(|id| id.strip_prefix(&marker_prefix))
            {
                let watermark = marker
                    .parse::<i64>()
                    .map_err(|_| invalid("Mac 时长标记损坏"))?;
                if watermark < 0 || record["readTime"] != 0 {
                    return Err(invalid("Mac 时长标记无效"));
                }
                previous = previous.max(watermark);
            }
        }
        if time <= previous {
            continue;
        }
        let delta = time - previous;
        if let Some(record) = records
            .iter_mut()
            .find(|record| record["deviceId"] == "" && record["bookName"] == name)
        {
            let native = record["readTime"].as_i64().unwrap();
            record["readTime"] = json!(native
                .checked_add(delta)
                .ok_or_else(|| invalid("累计阅读时长溢出"))?);
        } else {
            records.push(json!({"deviceId":"", "bookName":name, "readTime":delta}));
        }
        records.push(
            json!({"deviceId":format!("{marker_prefix}{time}"), "bookName":name, "readTime":0}),
        );
    }
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    out.set_raw_comment(zip.comment().to_vec());
    for index in 0..zip.len() {
        let file = zip.by_index(index).map_err(|_| invalid("无法读取备份"))?;
        if file.name() != "readRecord.json" {
            out.raw_copy_file(file)
                .map_err(|_| invalid("无法保留原备份文件"))?;
        }
    }
    out.start_file(
        "readRecord.json",
        FileOptions::default().compression_method(zip::CompressionMethod::Deflated),
    )
    .map_err(|_| invalid("无法写入阅读统计"))?;
    out.write_all(&serde_json::to_vec(&records).map_err(|_| invalid("无法编码阅读统计"))?)
        .map_err(|_| invalid("无法写入阅读统计"))?;
    let result = out
        .finish()
        .map_err(|_| invalid("无法生成备份"))?
        .into_inner();
    if result.len() > MAX_ARCHIVE_BYTES {
        return Err(invalid("合并后的备份超过 128MB"));
    }
    Ok(result)
}

/// Persist outside the app bundle. A process-wide lock protects simultaneous tabs.
pub fn installation_device_id(root: &std::path::Path) -> Result<String, AppError> {
    use std::io::Write;
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().map_err(|_| invalid("设备标识锁不可用"))?;
    std::fs::create_dir_all(root).map_err(|e| AppError::Internal(e.into()))?;
    let path = root.join("legado-mac-device-id");
    match std::fs::read_to_string(&path) {
        Ok(id) if id.trim().starts_with("reader-mac-") && id.trim().len() <= 64 => {
            Ok(id.trim().to_owned())
        }
        Ok(_) => Err(invalid("Mac 设备标识文件已损坏，请从原数据目录恢复")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let id = format!("reader-mac-{}", uuid::Uuid::new_v4());
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| AppError::Internal(e.into()))?;
            file.write_all(id.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|e| AppError::Internal(e.into()))?;
            Ok(id)
        }
        Err(e) => Err(AppError::Internal(e.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::{Cursor, Read, Write};
    use zip::{write::FileOptions, ZipArchive, ZipWriter};

    fn archive(records: Option<Value>, extras: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [
            ("bookshelf.json", b"[]".as_slice()),
            ("bookSource.json", b"[]"),
            ("config.xml", b"<map/>"),
        ] {
            zip.start_file(name, FileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        if let Some(value) = records {
            zip.start_file("readRecord.json", FileOptions::default())
                .unwrap();
            zip.write_all(value.to_string().as_bytes()).unwrap();
        }
        for (name, data) in extras {
            zip.start_file(*name, FileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn entry(bytes: &[u8], name: &str) -> Vec<u8> {
        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = Vec::new();
        zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
        out
    }

    fn rows(seconds: i64) -> Vec<BookStatsRow> {
        vec![BookStatsRow {
            date: "2026-11-11".into(),
            book_url: "book-a".into(),
            book_name: "测试书籍".into(),
            book_author: "作者".into(),
            seconds,
            characters: 0,
        }]
    }

    #[test]
    fn retains_phone_records_and_unknown_files_and_converts_seconds_to_milliseconds() {
        let phone =
            json!({"deviceId":"phone", "bookName":"测试书籍", "readTime":9000, "futureField":true});
        let input = archive(
            Some(json!([phone.clone()])),
            &[("unknown/binary.dat", &[0, 255, 1, 128])],
        );
        let mut data = rows(10);
        data.extend(rows(7)); // same title, multiple dates or sources
        let merged = merge_backup(&input, "reader-mac-test", &data).unwrap();
        let records: Value = serde_json::from_slice(&entry(&merged, "readRecord.json")).unwrap();
        assert_eq!(records[0], phone);
        assert_eq!(
            records[1],
            json!({"deviceId":"", "bookName":"测试书籍", "readTime":17000})
        );
        assert_eq!(
            records[2],
            json!({"deviceId":"reader-mac-test@17000", "bookName":"测试书籍", "readTime":0})
        );
        for name in [
            "config.xml",
            "bookshelf.json",
            "bookSource.json",
            "unknown/binary.dat",
        ] {
            assert_eq!(entry(&merged, name), entry(&input, name));
        }
        // The input archive is never mutated.
        assert_eq!(
            serde_json::from_slice::<Value>(&entry(&input, "readRecord.json")).unwrap(),
            json!([phone])
        );
    }

    #[test]
    fn repeated_merge_replaces_mac_total_and_never_decreases_it() {
        let input = archive(Some(json!([])), &[]);
        let first = merge_backup(&input, "reader-mac-test", &rows(10)).unwrap();
        let second = merge_backup(&first, "reader-mac-test", &rows(10)).unwrap();
        assert_eq!(
            entry(&first, "readRecord.json"),
            entry(&second, "readRecord.json")
        );
        let stale = merge_backup(&second, "reader-mac-test", &rows(5)).unwrap();
        assert_eq!(
            entry(&second, "readRecord.json"),
            entry(&stale, "readRecord.json")
        );
        let next = merge_backup(&second, "reader-mac-test", &rows(15)).unwrap();
        let records: Value = serde_json::from_slice(&entry(&next, "readRecord.json")).unwrap();
        assert_eq!(records.as_array().unwrap().len(), 3);
        assert_eq!(records[0]["readTime"], 15000);
    }

    #[test]
    fn old_phone_can_keep_reading_after_import_without_recounting_mac_time() {
        let input = archive(
            Some(json!([{"deviceId":"", "bookName":"测试书籍", "readTime":9000}])),
            &[],
        );
        let first = merge_backup(&input, "reader-mac-test", &rows(10)).unwrap();
        let mut records: Vec<Value> =
            serde_json::from_slice(&entry(&first, "readRecord.json")).unwrap();
        // Exact 3.22 ReadBook.resetData: reads SUM(all device records), then
        // upReadTime writes that SUM + elapsed into deviceId="".
        let total: i64 = records
            .iter()
            .map(|r| r["readTime"].as_i64().unwrap())
            .sum();
        records.iter_mut().find(|r| r["deviceId"] == "").unwrap()["readTime"] = json!(total + 3000);
        assert_eq!(
            records
                .iter()
                .map(|r| r["readTime"].as_i64().unwrap())
                .sum::<i64>(),
            22000
        );
        let fresh_phone = archive(Some(json!(records)), &[]);
        let next = merge_backup(&fresh_phone, "reader-mac-test", &rows(15)).unwrap();
        let records: Vec<Value> = serde_json::from_slice(&entry(&next, "readRecord.json")).unwrap();
        assert_eq!(
            records
                .iter()
                .map(|r| r["readTime"].as_i64().unwrap())
                .sum::<i64>(),
            27000
        );
        let repeated = merge_backup(&next, "reader-mac-test", &rows(15)).unwrap();
        assert_eq!(
            entry(&next, "readRecord.json"),
            entry(&repeated, "readRecord.json")
        );
    }

    #[test]
    fn device_identity_is_stable_and_library_preview_does_not_change_stats() {
        let root = std::env::temp_dir().join(format!("reader-id-test-{}", uuid::Uuid::new_v4()));
        let id = installation_device_id(&root).unwrap();
        assert_eq!(id, installation_device_id(&root).unwrap());
        let records = json!([{"deviceId":"", "bookName":"测试书籍", "readTime":9000}]);
        let input = archive(Some(records.clone()), &[]);
        let preview = inspect_library(&input).unwrap();
        assert_eq!(preview, json!({"books":[],"sources":[]}));
        assert_eq!(
            serde_json::from_slice::<Value>(&entry(&input, "readRecord.json")).unwrap(),
            records
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn supports_phone_backup_with_no_previous_reading_records() {
        let input = archive(None, &[]);
        let merged = merge_backup(&input, "reader-mac-test", &rows(2)).unwrap();
        let records: Value = serde_json::from_slice(&entry(&merged, "readRecord.json")).unwrap();
        assert_eq!(records[0]["readTime"], 2000);
    }

    #[test]
    fn rejects_corrupt_records_unsafe_paths_duplicates_and_partial_backups() {
        for records in [
            json!({}),
            json!([{"deviceId":"phone","bookName":"book","readTime":-1}]),
            json!([{}, {}]),
        ] {
            assert!(
                merge_backup(&archive(Some(records), &[]), "reader-mac-test", &rows(1)).is_err()
            );
        }
        for name in [
            "../config.xml",
            "..\\config.xml",
            "bookshelf.json",
            "readRecord.json",
        ] {
            assert!(merge_backup(
                &archive(Some(json!([])), &[(name, b"[]")]),
                "reader-mac-test",
                &rows(1)
            )
            .is_err());
        }
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("readRecord.json", FileOptions::default())
            .unwrap();
        zip.write_all(b"[]").unwrap();
        let partial = zip.finish().unwrap().into_inner();
        assert!(merge_backup(&partial, "reader-mac-test", &rows(1)).is_err());
        assert!(merge_backup(b"not a zip", "reader-mac-test", &rows(1)).is_err());
        assert!(merge_backup(
            &archive(Some(json!([])), &[]),
            "reader-mac-test",
            &rows(i64::MAX)
        )
        .is_err());
        assert!(
            merge_backup(&archive(Some(json!([])), &[]), "reader-mac-test", &rows(-1)).is_err()
        );
    }
}

/// Validate a phone backup and preview the online library without changing data.
pub fn inspect_library(bytes: &[u8]) -> Result<serde_json::Value, AppError> {
    let _ = merge_backup(bytes, "reader-mac-validation", &[])?;
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| invalid("ZIP 无效"))?;
    let books: serde_json::Value = serde_json::from_reader(
        zip.by_name("bookshelf.json")
            .map_err(|_| invalid("缺少书架"))?,
    )
    .map_err(|_| invalid("书架格式错误"))?;
    let sources: serde_json::Value = serde_json::from_reader(
        zip.by_name("bookSource.json")
            .map_err(|_| invalid("缺少书源"))?,
    )
    .map_err(|_| invalid("书源格式错误"))?;
    Ok(serde_json::json!({"books": books, "sources": sources}))
}
