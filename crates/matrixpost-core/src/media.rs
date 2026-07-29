//! Bounded, policy-driven remote media staging.

use crate::error::DomainError;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use url::Url;

pub trait RemoteMediaPolicy: Send + Sync {
    fn max_bytes(&self) -> u64;
    fn allows_content_type(&self, content_type: Option<&str>) -> bool;
}
/// A bounded remote-media staging request. It does not perform a fetch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteMediaRequest {
    pub url: Url,
    pub max_bytes: u64,
}
impl RemoteMediaRequest {
    pub fn new(url: Url, policy: &dyn RemoteMediaPolicy) -> Result<Self, DomainError> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(DomainError::UnsupportedRemoteScheme(
                url.scheme().to_owned(),
            ));
        }
        Ok(Self {
            url,
            max_bytes: policy.max_bytes(),
        })
    }
}
/// A staged file is owned by its creator and cleaned up by calling `cleanup`.
pub trait StagedMedia: Send {
    fn path(&self) -> &Path;
    fn cleanup(self: Box<Self>) -> Result<(), DomainError>;
}
/// Boundary for bounded HTTP staging adapters.
pub trait RemoteMediaStager: Send + Sync {
    fn stage(
        &self,
        request: &RemoteMediaRequest,
        policy: &dyn RemoteMediaPolicy,
    ) -> Result<Box<dyn StagedMedia>, DomainError>;
}

/// Metadata policy used before an adapter stages a remote media object.
///
/// Concrete bounded policy used by the daemon and desktop adapters.
#[derive(Clone, Debug)]
pub struct MediaStagingPolicy {
    pub max_bytes: u64,
    pub allowed_content_types: Vec<String>,
}
impl RemoteMediaPolicy for MediaStagingPolicy {
    fn max_bytes(&self) -> u64 {
        self.max_bytes
    }
    fn allows_content_type(&self, value: Option<&str>) -> bool {
        value.is_some_and(|item| {
            self.allowed_content_types
                .iter()
                .any(|allowed| item.starts_with(allowed))
        })
    }
}
/// HTTP-only staging implementation. It is deliberately not connected to providers.
pub struct HttpRemoteMediaStager {
    directory: PathBuf,
}

pub(crate) struct RemoteMediaResponse {
    pub(crate) content_type: Option<String>,
    pub(crate) content_length: Option<String>,
    pub(crate) body: Box<dyn Read>,
}

pub(crate) trait RemoteMediaTransport {
    fn get(&self, url: &Url) -> Result<RemoteMediaResponse, DomainError>;
}

struct UreqRemoteMediaTransport;

impl RemoteMediaTransport for UreqRemoteMediaTransport {
    fn get(&self, url: &Url) -> Result<RemoteMediaResponse, DomainError> {
        let response = ureq::get(url.as_str())
            .call()
            .map_err(|error| DomainError::RemoteMedia(error.to_string()))?;
        let content_type = response.header("content-type").map(str::to_owned);
        let content_length = response.header("content-length").map(str::to_owned);
        Ok(RemoteMediaResponse {
            content_type,
            content_length,
            body: Box::new(response.into_reader()),
        })
    }
}

pub(crate) trait StagingFilesystem {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()>;
    fn create_new(&self, path: &Path) -> std::io::Result<Box<dyn Write>>;
    fn remove_file(&self, path: &Path) -> std::io::Result<()>;
}

struct OsStagingFilesystem;

impl StagingFilesystem for OsStagingFilesystem {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        fs::create_dir_all(path)
    }

    fn create_new(&self, path: &Path) -> std::io::Result<Box<dyn Write>> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map(|file| Box::new(file) as Box<dyn Write>)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }
}

pub(crate) trait StagingNameSource {
    fn next_name(&mut self) -> String;
}

struct RandomStagingNameSource;

impl StagingNameSource for RandomStagingNameSource {
    fn next_name(&mut self) -> String {
        format!("matrixpost-stage-{:032x}", rand::random::<u128>())
    }
}
pub struct OwnedStagedMedia {
    path: PathBuf,
}
impl StagedMedia for OwnedStagedMedia {
    fn path(&self) -> &Path {
        &self.path
    }
    fn cleanup(self: Box<Self>) -> Result<(), DomainError> {
        fs::remove_file(&self.path).map_err(DomainError::io)
    }
}
impl HttpRemoteMediaStager {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub(crate) fn stage_with(
        &self,
        request: &RemoteMediaRequest,
        policy: &dyn RemoteMediaPolicy,
        transport: &dyn RemoteMediaTransport,
        filesystem: &dyn StagingFilesystem,
        names: &mut dyn StagingNameSource,
    ) -> Result<Box<dyn StagedMedia>, DomainError> {
        let response = transport.get(&request.url)?;
        if !policy.allows_content_type(response.content_type.as_deref()) {
            return Err(DomainError::DisallowedContentType(
                response.content_type.unwrap_or_else(|| "missing".into()),
            ));
        }
        if let Some(length) = response.content_length.as_deref() {
            let parsed = length
                .parse::<u64>()
                .map_err(|_| DomainError::RemoteMedia("invalid content-length".into()))?;
            if parsed > request.max_bytes {
                return Err(DomainError::RemoteMediaTooLarge {
                    limit: request.max_bytes,
                    actual: parsed,
                });
            }
        }
        filesystem
            .create_dir_all(&self.directory)
            .map_err(DomainError::io)?;
        let (path, mut output) = (0..16)
            .find_map(|_| {
                let path = self.directory.join(names.next_name());
                match filesystem.create_new(&path) {
                    Ok(file) => Some(Ok((path, file))),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(error) => Some(Err(DomainError::io(error))),
                }
            })
            .transpose()?
            .ok_or_else(|| {
                DomainError::RemoteMedia("could not allocate unique staging file".into())
            })?;
        let mut reader = response.body.take(request.max_bytes.saturating_add(1));
        let copied = match std::io::copy(&mut reader, &mut output).and_then(|value| {
            output.flush()?;
            Ok(value)
        }) {
            Ok(value) => value,
            Err(error) => {
                let _ = filesystem.remove_file(&path);
                return Err(DomainError::io(error));
            }
        };
        if copied > request.max_bytes {
            let _ = filesystem.remove_file(&path);
            return Err(DomainError::RemoteMediaTooLarge {
                limit: request.max_bytes,
                actual: copied,
            });
        }
        Ok(Box::new(OwnedStagedMedia { path }))
    }
}
impl RemoteMediaStager for HttpRemoteMediaStager {
    fn stage(
        &self,
        request: &RemoteMediaRequest,
        policy: &dyn RemoteMediaPolicy,
    ) -> Result<Box<dyn StagedMedia>, DomainError> {
        self.stage_with(
            request,
            policy,
            &UreqRemoteMediaTransport,
            &OsStagingFilesystem,
            &mut RandomStagingNameSource,
        )
    }
}
