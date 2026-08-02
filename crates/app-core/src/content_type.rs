//! 按文件扩展名推断默认的 Content-Type。
//!
//! 上传时如果调用方没有显式指定 Content-Type,过去会原样传 `None` 到 provider,
//! 导致对象存储端不下发 `Content-Type` 响应头(通常回退成
//! `application/octet-stream`),图片 / 视频 / PDF 等在预签名链接里因此无法被
//! 浏览器 / WebView 正确识别并预览。这里给上传路径提供一个兜底推断。

/// 从远端路径的扩展名推断 MIME 类型;取文件名的最后一段扩展名,大小写不敏感。
/// 无扩展名或未知扩展名时返回 `None`(调用方保持原来"不设置 Content-Type"的行为)。
pub(crate) fn guess_content_type(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let (_, ext) = name.rsplit_once('.')?;
    Some(match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "tif" | "tiff" => "image/tiff",
        "avif" => "image/avif",

        "mp4" => "video/mp4",
        "m4v" => "video/x-m4v",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "flv" => "video/x-flv",

        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",

        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",

        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",

        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_common_image_types() {
        assert_eq!(guess_content_type("a/b/photo.PNG"), Some("image/png"));
        assert_eq!(guess_content_type("photo.jpeg"), Some("image/jpeg"));
    }

    #[test]
    fn ignores_dots_in_parent_directories() {
        assert_eq!(guess_content_type("my.folder/readme"), None);
        assert_eq!(
            guess_content_type("my.folder/readme.txt"),
            Some("text/plain")
        );
    }

    #[test]
    fn unknown_or_missing_extension_is_none() {
        assert_eq!(guess_content_type("no_extension"), None);
        assert_eq!(guess_content_type("archive.unknownext"), None);
    }
}
