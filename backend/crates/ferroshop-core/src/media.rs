//! Media files: storage behind `object_store` (local directory or S3) and thumbnails.

use std::io::Cursor;
use std::sync::Arc;

use bytes::Bytes;
use image::{GenericImageView, ImageFormat};
use object_store::local::LocalFileSystem;
use object_store::path::Path;
use object_store::prefix::PrefixStore;
use object_store::{ObjectStore, ObjectStoreExt};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::Db;
use crate::context::Context;
use crate::error::{Error, Result, WriteError};

/// Thumbnail widths; only sizes smaller than the original are generated.
pub const THUMBNAIL_SIZES: [u32; 3] = [400, 800, 1920];

/// Allowed uploads. SVG is deliberately missing: it can carry scripts and is served same-origin.
pub const ALLOWED: &[(&str, &str)] = &[
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("png", "image/png"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("pdf", "application/pdf"),
];

pub fn mime_for(extension: &str) -> Option<&'static str> {
    ALLOWED.iter().find(|(ext, _)| ext.eq_ignore_ascii_case(extension)).map(|(_, mime)| *mime)
}

#[derive(Clone)]
pub struct MediaStore {
    store: Arc<dyn ObjectStore>,
}

impl MediaStore {
    /// `s3://bucket/prefix` (credentials from the usual `AWS_*` variables) or a local directory.
    pub fn from_config(location: &str) -> Result<Self, String> {
        if location.starts_with("s3://") {
            let url = url::Url::parse(location).map_err(|e| e.to_string())?;
            let options = std::env::vars().filter(|(k, _)| k.starts_with("AWS_"));
            let (store, prefix) = object_store::parse_url_opts(&url, options).map_err(|e| e.to_string())?;
            let store: Arc<dyn ObjectStore> = Arc::from(store);
            let store: Arc<dyn ObjectStore> =
                if prefix.as_ref().is_empty() { store } else { Arc::new(PrefixStore::new(store, prefix)) };
            return Ok(Self { store });
        }
        let dir = location.strip_prefix("file://").unwrap_or(location);
        std::fs::create_dir_all(dir).map_err(|e| format!("creating media directory `{dir}`: {e}"))?;
        let store = LocalFileSystem::new_with_prefix(dir).map_err(|e| e.to_string())?;
        Ok(Self { store: Arc::new(store) })
    }

    /// In-memory store for tests.
    pub fn memory() -> Self {
        Self { store: Arc::new(object_store::memory::InMemory::new()) }
    }

    pub async fn get(&self, path: &str) -> Option<Bytes> {
        let path = Path::parse(path).ok()?;
        self.store.get(&path).await.ok()?.bytes().await.ok()
    }

    async fn put(&self, path: &str, data: Bytes) -> Result<()> {
        let path = Path::parse(path).map_err(|e| Error::Storage(e.to_string()))?;
        self.store.put(&path, data.into()).await.map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, path: &str) {
        if let Ok(path) = Path::parse(path) {
            let _ = self.store.delete(&path).await;
        }
    }
}

/// Lowercase ascii, digits and dashes only.
fn slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() { "file".into() } else { slug.chars().take(80).collect() }
}

struct Processed {
    width: Option<u32>,
    height: Option<u32>,
    thumbnails: Vec<(u32, u32, Vec<u8>)>,
}

fn process_image(data: &[u8], format: ImageFormat) -> Result<Processed, String> {
    let img = image::load_from_memory_with_format(data, format).map_err(|e| format!("not a valid image: {e}"))?;
    let (width, height) = img.dimensions();
    let mut thumbnails = Vec::new();
    for size in THUMBNAIL_SIZES {
        if size >= width && size >= height {
            continue;
        }
        let thumb = img.thumbnail(size, size);
        let mut out = Cursor::new(Vec::new());
        thumb.write_to(&mut out, format).map_err(|e| e.to_string())?;
        thumbnails.push((thumb.width(), thumb.height(), out.into_inner()));
    }
    Ok(Processed { width: Some(width), height: Some(height), thumbnails })
}

/// Stores an uploaded file for an existing media entity (`POST /api/_action/media/{id}/upload`).
pub async fn upload(
    db: &Db,
    store: &MediaStore,
    media_id: Uuid,
    file_name: &str,
    extension: &str,
    data: Bytes,
    ctx: &Context,
) -> Result<()> {
    let extension = extension.to_ascii_lowercase();
    let Some(mime) = mime_for(&extension) else {
        let allowed: Vec<&str> = ALLOWED.iter().map(|(e, _)| *e).collect();
        return Err(Error::Validation(vec![WriteError::new(
            "/extension",
            "CONTENT__MEDIA_ILLEGAL_FILE_TYPE",
            format!("file type `{extension}` is not allowed ({})", allowed.join(", ")),
        )]));
    };
    if data.is_empty() {
        return Err(Error::Validation(vec![WriteError::new("/", "CONTENT__MEDIA_EMPTY_FILE", "the file is empty")]));
    }
    let old: Option<(Option<String>, Value)> = sqlx::query("SELECT path, thumbnails FROM media WHERE id = $1")
        .bind(media_id)
        .fetch_optional(db)
        .await?
        .map(|row| Ok::<_, sqlx::Error>((row.try_get("path")?, row.try_get("thumbnails")?)))
        .transpose()?;
    let Some((old_path, old_thumbnails)) = old else {
        return Err(Error::NotFound { entity: "media".into(), id: crate::id::format(media_id) });
    };

    let processed = match ImageFormat::from_extension(&extension).filter(|_| mime.starts_with("image/")) {
        Some(format) => {
            let bytes = data.clone();
            tokio::task::spawn_blocking(move || process_image(&bytes, format))
                .await
                .map_err(|e| Error::Storage(e.to_string()))?
                .map_err(|e| Error::Validation(vec![WriteError::new("/", "CONTENT__MEDIA_INVALID_FILE", e)]))?
        }
        None => Processed { width: None, height: None, thumbnails: Vec::new() },
    };

    // A fresh directory per upload keeps URLs cacheable forever.
    let name = slug(file_name);
    let dir = format!("{}/{}", crate::id::format(media_id), chrono::Utc::now().timestamp_millis());
    let path = format!("{dir}/{name}.{extension}");
    store.put(&path, data.clone()).await?;
    let mut thumbnails = Vec::new();
    for (w, h, bytes) in processed.thumbnails {
        let thumb_path = format!("{dir}/thumbnail/{name}_{w}x{h}.{extension}");
        store.put(&thumb_path, Bytes::from(bytes)).await?;
        thumbnails.push(json!({ "width": w, "height": h, "path": thumb_path, "url": format!("/media/{thumb_path}") }));
    }

    sqlx::query(
        "UPDATE media SET file_name = $2, file_extension = $3, mime_type = $4, file_size = $5, path = $6, \
         width = $7, height = $8, thumbnails = $9, uploaded_at = now(), updated_at = now() WHERE id = $1",
    )
    .bind(media_id)
    .bind(&name)
    .bind(&extension)
    .bind(mime)
    .bind(data.len() as i64)
    .bind(&path)
    .bind(processed.width.map(|w| w as i32))
    .bind(processed.height.map(|h| h as i32))
    .bind(Value::Array(thumbnails))
    .execute(db)
    .await?;
    sqlx::query(
        "INSERT INTO audit_log (id, entity, entity_id, action, payload, actor_id) VALUES ($1, 'media', $2, 'upload', $3, $4)",
    )
    .bind(crate::id::new_id())
    .bind(media_id)
    .bind(json!({ "path": path, "size": data.len() }))
    .bind(ctx.actor_id)
    .execute(db)
    .await?;

    // Old files are only removed once the new ones are referenced.
    if let Some(old_path) = old_path {
        store.delete(&old_path).await;
    }
    for thumb in old_thumbnails.as_array().into_iter().flatten() {
        if let Some(p) = thumb["path"].as_str() {
            store.delete(p).await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("My Photo (1)"), "my-photo-1");
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(slug("ÄÖÜ"), "file");
    }

    #[test]
    fn thumbnails_only_downscale() {
        let img = image::DynamicImage::new_rgb8(1000, 500);
        let mut png = Cursor::new(Vec::new());
        img.write_to(&mut png, ImageFormat::Png).unwrap();
        let processed = process_image(png.get_ref(), ImageFormat::Png).unwrap();
        assert_eq!((processed.width, processed.height), (Some(1000), Some(500)));
        let sizes: Vec<(u32, u32)> = processed.thumbnails.iter().map(|(w, h, _)| (*w, *h)).collect();
        assert_eq!(sizes, [(400, 200), (800, 400)]);
        assert!(process_image(b"nope", ImageFormat::Png).is_err());
    }
}
