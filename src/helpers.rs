use anyhow::Result;
use hyper::{
    body::{self, Buf},
    Client, Uri,
};
use hyper_tls::HttpsConnector;
use rayon::iter::{ParallelBridge, ParallelIterator};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

use crate::entity::Zine;

pub fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
    }
}

pub async fn fetch_url(url: &str) -> Result<impl Read> {
    let client = Client::builder().build::<_, hyper::Body>(HttpsConnector::new());
    let resp = client.get(url.parse::<Uri>()?).await?;
    let bytes = body::to_bytes(resp.into_body()).await?;
    Ok(bytes.reader())
}

/// Copy directory recursively.
/// Note: the empty directory is ignored.
pub fn copy_dir(source: &Path, dest: &Path) -> Result<()> {
    let source_parent = source.parent().expect("Can not copy the root dir");
    walkdir::WalkDir::new(source)
        .into_iter()
        .par_bridge()
        .try_for_each(|entry| {
            let entry = entry?;
            let path = entry.path();
            // `path` would be a file or directory. However, we are
            // in a rayon's parallel thread, there is no guarantee
            // that parent directory iterated before the file.
            // So we just ignore the `path.is_dir()` case, when coming
            // across the first file we'll create the parent directory.
            if path.is_file() {
                if let Some(parent) = path.parent() {
                    let dest_parent = dest.join(parent.strip_prefix(source_parent)?);
                    if !dest_parent.exists() {
                        // Create the same dir concurrently is ok according to the docs.
                        fs::create_dir_all(dest_parent)?;
                    }
                }
                let to = dest.join(path.strip_prefix(source_parent)?);
                fs::copy(path, to)?;
            }

            anyhow::Ok(())
        })?;
    Ok(())
}

/// Find the root zine file in current dir and try to parse it
fn try_to_parse_root_file<P: AsRef<Path>>(path: P) -> Option<Zine> {
    // Find the name in current dir
    if WalkDir::new(&path).max_depth(1).into_iter().any(|entry| {
        let entry = entry.as_ref().unwrap();
        entry.file_name().to_str().unwrap() == crate::ZINE_FILE
    }) {
        // Try to parse the root zine.toml as Zine instance
        let root_file = path.as_ref().join(crate::ZINE_FILE);
        let content =
            std::fs::read_to_string(path.as_ref().join(root_file)).unwrap_or_else(|_| "".into());
        if let Ok(zine) = toml::from_str::<Zine>(&content) {
            return Some(zine);
        }
    }
    None
}

/// Find recursively
fn _find_zine_folder(path: PathBuf) -> Option<(PathBuf, Zine)> {
    if let Some(zine) = try_to_parse_root_file(&path) {
        return Some((path, zine));
    }
    match path.parent() {
        Some(parent_path) => _find_zine_folder(parent_path.to_path_buf()),
        None => None,
    }
}

/// Find folder contains `zine.toml` as root path, and return path info and Zine instance
pub fn find_zine_folder(path: impl AsRef<Path>) -> Option<(impl AsRef<Path>, Zine)> {
    _find_zine_folder(std::fs::canonicalize(path).unwrap())
}

/// A serde module to serialize and deserialize [`time::Date`] type.
pub mod serde_date {
    use serde::{de, Serialize, Serializer};
    use time::{format_description, Date};

    pub fn serialize<S: Serializer>(date: &Date, serializer: S) -> Result<S::Ok, S::Error> {
        let format = format_description::parse("[year]-[month]-[day]").expect("Shouldn't happen");
        date.format(&format)
            .expect("Serialize date error")
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Date, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        d.deserialize_any(DateVisitor)
    }

    struct DateVisitor;

    impl<'de> de::Visitor<'de> for DateVisitor {
        type Value = Date;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a date value like YYYY-MM-dd")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let format =
                format_description::parse("[year]-[month]-[day]").expect("Shouldn't happen");
            Ok(Date::parse(v, &format)
                .unwrap_or_else(|_| panic!("The date value {} is invalid", &v)))
        }
    }
}
