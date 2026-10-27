//! Bounded ZIP inspection. No extraction to the server filesystem and no arbitrary process calls.
use scribe_shared::{ApiError, ApiResult};
use serde::Serialize;
use std::{
    collections::{BTreeSet, HashSet},
    io::{Cursor, Read},
};
const MIB: usize = 1024 * 1024;
pub const MAX_ZIP_BYTES: usize = 20 * MIB;
pub const MAX_FILE_BYTES: usize = 8 * MIB;
pub const MAX_EXPANDED_BYTES: usize = 32 * MIB;
pub const MAX_ENTRIES: usize = 256;
#[derive(Clone, Copy)]
struct Limits {
    compressed: usize,
    file: usize,
    total: usize,
    entries: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            compressed: MAX_ZIP_BYTES,
            file: MAX_FILE_BYTES,
            total: MAX_EXPANDED_BYTES,
            entries: MAX_ENTRIES,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    pub path: String,
    pub size_bytes: usize,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZipManifest {
    pub files: Vec<ManifestFile>,
    pub directories: Vec<String>,
    pub main_candidates: Vec<String>,
    pub suggested_main_file: Option<String>,
    pub total_bytes: usize,
    pub skipped_metadata: usize,
}
#[derive(Debug)]
pub struct ArchiveFile {
    pub path: String,
    pub bytes: Vec<u8>,
}
#[derive(Debug)]
pub struct ParsedArchive {
    pub files: Vec<ArchiveFile>,
    pub manifest: ZipManifest,
}
fn invalid(message: &str) -> ApiError {
    ApiError::validation(message)
}
/// A leading ./ is harmless, but all other dot components and platform-specific paths are rejected.
pub fn safe_path(raw: &str) -> ApiResult<String> {
    if raw.starts_with('/')
        || raw.contains('\\')
        || raw.contains(':')
        || raw.chars().any(char::is_control)
    {
        return Err(invalid(
            "ZIP包含绝对路径、反斜杠、盘符或控制字符，未导入任何文件",
        ));
    }
    let mut path = raw;
    while let Some(rest) = path.strip_prefix("./") {
        path = rest;
    }
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty()
        || path.len() > 512
        || path.split('/').count() > 32
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".." | ".git"))
    {
        return Err(invalid("ZIP文件路径不安全或过长，未导入任何文件"));
    }
    Ok(path.to_string())
}
fn metadata(path: &str) -> bool {
    path.split('/')
        .any(|part| part == "__MACOSX" || part == ".DS_Store" || part.starts_with("._"))
}
pub fn is_main_candidate(path: &str) -> bool {
    path.rsplit('.').next().is_some_and(|extension| {
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "tex" | "ltx" | "latex"
        )
    })
}
pub fn parse_zip(bytes: Vec<u8>) -> ApiResult<ParsedArchive> {
    parse_with_limits(bytes, Limits::default())
}
fn parse_with_limits(bytes: Vec<u8>, limits: Limits) -> ApiResult<ParsedArchive> {
    if bytes.is_empty() || bytes.len() > limits.compressed {
        return Err(invalid("ZIP压缩包最多20MiB，且不能为空"));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| invalid("压缩包损坏或不是受支持的ZIP格式"))?;
    if zip.len() == 0 || zip.len() > limits.entries {
        return Err(invalid("ZIP最多256个条目，包含目录及系统元数据"));
    }
    let mut files = vec![];
    let mut directories = BTreeSet::new();
    let mut names = HashSet::new();
    let mut total = 0usize;
    let mut skipped = 0usize;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|_| invalid("ZIP条目损坏、已加密或压缩算法不受支持，请解密后重新打包"))?;
        if entry.encrypted() || entry.is_symlink() {
            return Err(invalid("ZIP包含加密条目或符号链接，未导入任何文件"));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000))
        {
            return Err(invalid("ZIP只允许普通文件和目录，不能包含链接或设备条目"));
        }
        if entry.is_dir() && matches!(entry.name(), "./" | ".") {
            continue;
        }
        let path = safe_path(entry.name())?;
        if !names.insert(path.clone()) {
            return Err(invalid("ZIP含重复或归一化后重名的路径，未导入任何文件"));
        }
        if metadata(&path) {
            skipped += 1;
            continue;
        }
        if entry.is_dir() {
            directories.insert(path);
            continue;
        }
        if entry.size() > limits.file as u64
            || entry.size() > limits.total.saturating_sub(total) as u64
        {
            return Err(invalid("ZIP展开配额超限：单文件最多8MiB，总计最多32MiB"));
        }
        // Count actual decompressed bytes, even if central-directory size fields were forged.
        let remaining = limits.file.min(limits.total.saturating_sub(total));
        let mut content = Vec::new();
        entry
            .by_ref()
            .take(remaining as u64 + 1)
            .read_to_end(&mut content)
            .map_err(|_| invalid("ZIP文件解压或校验失败，未导入任何文件"))?;
        if content.len() > remaining {
            return Err(invalid("ZIP实际展开数据超过安全配额，未导入任何文件"));
        }
        total += content.len();
        files.push(ArchiveFile {
            path,
            bytes: content,
        });
    }
    let file_names = files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<HashSet<_>>();
    for name in file_names
        .iter()
        .copied()
        .chain(directories.iter().map(String::as_str))
    {
        let mut prefix = String::new();
        for part in name
            .split('/')
            .take(name.split('/').count().saturating_sub(1))
        {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if file_names.contains(prefix.as_str()) {
                return Err(invalid("ZIP内文件与目录路径冲突，未导入任何文件"));
            }
        }
        if directories.contains(name) && file_names.contains(name) {
            return Err(invalid("ZIP内文件与目录重名，未导入任何文件"));
        }
    }
    if files.is_empty() && directories.is_empty() {
        return Err(invalid("ZIP中没有可导入文件或目录"));
    }
    let main_candidates = files
        .iter()
        .filter(|file| is_main_candidate(&file.path))
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let preferred = if main_candidates.iter().any(|path| path == "main.tex") {
        Some("main.tex".into())
    } else {
        let named = main_candidates
            .iter()
            .filter(|path| {
                path.rsplit('/')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("main.tex"))
            })
            .cloned()
            .collect::<Vec<_>>();
        if named.len() == 1 {
            named.into_iter().next()
        } else {
            let document = files
                .iter()
                .filter(|file| {
                    is_main_candidate(&file.path)
                        && std::str::from_utf8(&file.bytes)
                            .is_ok_and(|text| text.contains("\\documentclass"))
                })
                .map(|file| file.path.clone())
                .collect::<Vec<_>>();
            if document.len() == 1 {
                document.into_iter().next()
            } else if main_candidates.len() == 1 {
                main_candidates.first().cloned()
            } else {
                None
            }
        }
    };
    let manifest = ZipManifest {
        files: files
            .iter()
            .map(|file| ManifestFile {
                path: file.path.clone(),
                size_bytes: file.bytes.len(),
            })
            .collect(),
        directories: directories.into_iter().collect(),
        main_candidates,
        suggested_main_file: preferred,
        total_bytes: total,
        skipped_metadata: skipped,
    };
    Ok(ParsedArchive { files, manifest })
}
pub fn choose_main(archive: &ParsedArchive, requested: Option<&str>) -> ApiResult<String> {
    let main = requested
        .filter(|name| !name.is_empty())
        .map(safe_path)
        .transpose()?
        .or_else(|| archive.manifest.suggested_main_file.clone())
        .ok_or_else(|| invalid("请选择压缩包中的LaTeX主文件后再创建项目"))?;
    if !is_main_candidate(&main) || !archive.files.iter().any(|file| file.path == main) {
        return Err(invalid("所选主文件不在ZIP中，或不是.tex/.ltx/.latex文件"));
    }
    Ok(main)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            zip.start_file(
                *name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    #[test]
    fn preserves_directories_binary_empty_files_and_selects_main() {
        let bytes = archive(&[
            ("paper/main.tex", b"\\documentclass{article}"),
            ("paper/figures/a.png", &[0, 255, 128, 3]),
            ("paper/empty.txt", b""),
        ]);
        let parsed = parse_zip(bytes).unwrap();
        assert_eq!(choose_main(&parsed, None).unwrap(), "paper/main.tex");
        assert_eq!(parsed.files[1].bytes, vec![0, 255, 128, 3]);
        assert_eq!(parsed.files[2].bytes.len(), 0);
    }
    #[test]
    fn rejects_unsafe_paths_duplicates_and_file_directory_collisions() {
        for path in [
            "/absolute.tex",
            "../secret",
            "a/../b",
            "a\\b",
            "C:/main.tex",
            "a//b",
            ".git/config",
            "name\0.tex",
        ] {
            assert!(parse_zip(archive(&[(path, b"x")])).is_err(), "{path}");
        }
        assert!(parse_zip(archive(&[("main.tex", b"a"), ("./main.tex", b"b")])).is_err());
        assert!(parse_zip(archive(&[("folder", b"x"), ("folder/main.tex", b"y")])).is_err());
    }
    #[test]
    fn ambiguous_main_requires_explicit_existing_choice() {
        let parsed = parse_zip(archive(&[("a.tex", b"x"), ("b.tex", b"y")])).unwrap();
        assert!(choose_main(&parsed, None).is_err());
        assert_eq!(choose_main(&parsed, Some("b.tex")).unwrap(), "b.tex");
        assert!(choose_main(&parsed, Some("missing.tex")).is_err());
    }
    #[test]
    fn rejects_corrupt_encrypted_and_symlink_archives() {
        assert!(parse_zip(vec![1, 2, 3]).is_err());
        let mut encrypted = archive(&[("main.tex", b"source")]);
        for at in 0..encrypted.len() - 4 {
            if &encrypted[at..at + 4] == b"PK\x01\x02" {
                encrypted[at + 8] |= 1;
                break;
            }
        }
        assert!(parse_zip(encrypted).is_err());
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.add_symlink("link", "../../outside", SimpleFileOptions::default())
            .unwrap();
        assert!(parse_zip(zip.finish().unwrap().into_inner()).is_err());
    }
    #[test]
    fn actual_expansion_and_entry_limits_do_not_trust_zip_sizes() {
        let payload = vec![b'x'; 8192];
        let mut bytes = archive(&[("main.tex", &payload)]);
        for at in 0..bytes.len() - 4 {
            if &bytes[at..at + 4] == b"PK\x01\x02" {
                bytes[at + 24..at + 28].copy_from_slice(&1u32.to_le_bytes());
                break;
            }
        }
        let limits = Limits {
            compressed: 10000,
            file: 1024,
            total: 1024,
            entries: 8,
        };
        assert!(parse_with_limits(bytes, limits).is_err());
        assert!(parse_with_limits(
            archive(&[("a", b"a"), ("b", b"b")]),
            Limits {
                entries: 1,
                ..limits
            }
        )
        .is_err());
    }
}
