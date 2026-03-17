use super::common::next_handle;
use super::WasiHost;
use crate::bindings::imports::{
    wasi_filesystem_preopens as filesystem_preopens, wasi_filesystem_types as filesystem_types,
};
use crate::bindings::types::{
    WasiClocksWallClockDatetime, WasiFilesystemTypesDescriptor,
    WasiFilesystemTypesDescriptorBorrow, WasiFilesystemTypesDescriptorFlags,
    WasiFilesystemTypesDescriptorStat, WasiFilesystemTypesDescriptorType,
    WasiFilesystemTypesDirectoryEntry, WasiFilesystemTypesDirectoryEntryStream,
    WasiFilesystemTypesDirectoryEntryStreamBorrow, WasiFilesystemTypesErrorCode,
    WasiFilesystemTypesOpenFlags, WasiFilesystemTypesPathFlags, WasiIoErrorErrorBorrow,
    WasiIoStreamsInputStream,
};
use crate::state::{
    DescriptorEntry, DirectoryEntryStreamEntry, InputStreamEntry, InputStreamSource,
};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::UNIX_EPOCH;
use telomere_component::{ComponentError, ComponentFuture, ComponentLinker, Store};

pub(super) fn add_to_linker_sync(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    filesystem_types::add_to_linker(linker, Rc::clone(&host));
    filesystem_preopens::add_to_linker(linker, host);
}

pub(super) fn add_to_linker_async(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    filesystem_types::add_to_linker_async(linker, Rc::clone(&host));
    filesystem_preopens::add_to_linker_async(linker, host);
}

impl WasiHost {
    fn filesystem_error_code(
        &self,
        error: WasiIoErrorErrorBorrow,
    ) -> Result<Option<WasiFilesystemTypesErrorCode>, ComponentError> {
        self.state
            .inner
            .borrow()
            .errors
            .get(&error.handle())
            .map(|entry| entry.filesystem_code.clone())
            .ok_or_else(|| ComponentError::Trap(format!("unknown error handle {}", error.handle())))
    }

    fn preopen_directories(&self) -> Vec<(WasiFilesystemTypesDescriptor, String)> {
        self.state
            .inner
            .borrow()
            .preopen_handles
            .iter()
            .map(|(handle, guest_path)| {
                (
                    WasiFilesystemTypesDescriptor::new(*handle),
                    guest_path.clone(),
                )
            })
            .collect()
    }

    fn descriptor_entry(
        &self,
        handle: u32,
    ) -> Result<DescriptorEntry, WasiFilesystemTypesErrorCode> {
        self.state
            .inner
            .borrow()
            .descriptors
            .get(&handle)
            .cloned()
            .ok_or(WasiFilesystemTypesErrorCode::BadDescriptor)
    }

    fn descriptor_get_type(
        &self,
        descriptor: WasiFilesystemTypesDescriptorBorrow,
    ) -> Result<
        Result<WasiFilesystemTypesDescriptorType, WasiFilesystemTypesErrorCode>,
        ComponentError,
    > {
        Ok(self
            .descriptor_entry(descriptor.handle())
            .map(|entry| entry.descriptor_type))
    }

    fn descriptor_get_flags(
        &self,
        descriptor: WasiFilesystemTypesDescriptorBorrow,
    ) -> Result<
        Result<WasiFilesystemTypesDescriptorFlags, WasiFilesystemTypesErrorCode>,
        ComponentError,
    > {
        Ok(self
            .descriptor_entry(descriptor.handle())
            .map(|entry| entry.flags))
    }

    fn descriptor_stat(
        &self,
        descriptor: WasiFilesystemTypesDescriptorBorrow,
    ) -> Result<
        Result<WasiFilesystemTypesDescriptorStat, WasiFilesystemTypesErrorCode>,
        ComponentError,
    > {
        Ok(self
            .descriptor_entry(descriptor.handle())
            .and_then(|entry| stat_for_path(&entry.host_path, false)))
    }

    fn descriptor_stat_at(
        &self,
        descriptor: WasiFilesystemTypesDescriptorBorrow,
        path_flags: WasiFilesystemTypesPathFlags,
        path: String,
    ) -> Result<
        Result<WasiFilesystemTypesDescriptorStat, WasiFilesystemTypesErrorCode>,
        ComponentError,
    > {
        Ok(self
            .descriptor_child_path(descriptor.handle(), &path, path_flags.symlink_follow)
            .and_then(|path| stat_for_path(&path, path_flags.symlink_follow)))
    }

    fn descriptor_open_at(
        &self,
        descriptor: WasiFilesystemTypesDescriptorBorrow,
        path_flags: WasiFilesystemTypesPathFlags,
        path: String,
        open_flags: WasiFilesystemTypesOpenFlags,
        flags: WasiFilesystemTypesDescriptorFlags,
    ) -> Result<Result<WasiFilesystemTypesDescriptor, WasiFilesystemTypesErrorCode>, ComponentError>
    {
        if open_flags.create || open_flags.exclusive || open_flags.truncate || flags.write {
            return Ok(Err(WasiFilesystemTypesErrorCode::ReadOnly));
        }

        let base = match self.descriptor_entry(descriptor.handle()) {
            Ok(base) => base,
            Err(error) => return Ok(Err(error)),
        };
        if base.descriptor_type != WasiFilesystemTypesDescriptorType::Directory {
            return Ok(Err(WasiFilesystemTypesErrorCode::NotDirectory));
        }

        let host_path = match resolve_guest_path(&base.host_path, &path, path_flags.symlink_follow)
        {
            Ok(host_path) => host_path,
            Err(error) => return Ok(Err(error)),
        };
        let descriptor_type = match descriptor_type_for_path(&host_path, path_flags.symlink_follow)
        {
            Ok(descriptor_type) => descriptor_type,
            Err(error) => return Ok(Err(error)),
        };
        if open_flags.directory && descriptor_type != WasiFilesystemTypesDescriptorType::Directory {
            return Ok(Err(WasiFilesystemTypesErrorCode::NotDirectory));
        }

        let mut inner = self.state.inner.borrow_mut();
        let handle = next_handle(&mut inner.next_handle);
        inner.descriptors.insert(
            handle,
            DescriptorEntry {
                host_path,
                descriptor_type,
                flags,
            },
        );
        Ok(Ok(WasiFilesystemTypesDescriptor::new(handle)))
    }

    fn descriptor_read_via_stream(
        &self,
        descriptor: WasiFilesystemTypesDescriptorBorrow,
        offset: u64,
    ) -> Result<Result<WasiIoStreamsInputStream, WasiFilesystemTypesErrorCode>, ComponentError>
    {
        let entry = match self.descriptor_entry(descriptor.handle()) {
            Ok(entry) => entry,
            Err(error) => return Ok(Err(error)),
        };
        if !entry.flags.read {
            return Ok(Err(WasiFilesystemTypesErrorCode::NotPermitted));
        }
        if entry.descriptor_type != WasiFilesystemTypesDescriptorType::RegularFile {
            return Ok(Err(WasiFilesystemTypesErrorCode::BadDescriptor));
        }

        let mut inner = self.state.inner.borrow_mut();
        let handle = next_handle(&mut inner.next_handle);
        inner.input_streams.insert(
            handle,
            InputStreamEntry {
                source: InputStreamSource::File(entry.host_path),
                position: offset,
                closed: false,
            },
        );
        Ok(Ok(WasiIoStreamsInputStream::new(handle)))
    }

    fn descriptor_read(
        &self,
        descriptor: WasiFilesystemTypesDescriptorBorrow,
        length: u64,
        offset: u64,
    ) -> Result<Result<(Vec<u8>, bool), WasiFilesystemTypesErrorCode>, ComponentError> {
        let entry = match self.descriptor_entry(descriptor.handle()) {
            Ok(entry) => entry,
            Err(error) => return Ok(Err(error)),
        };
        if !entry.flags.read {
            return Ok(Err(WasiFilesystemTypesErrorCode::NotPermitted));
        }
        if entry.descriptor_type != WasiFilesystemTypesDescriptorType::RegularFile {
            return Ok(Err(WasiFilesystemTypesErrorCode::BadDescriptor));
        }
        Ok(read_file_range(&entry.host_path, offset, length))
    }

    fn descriptor_read_directory(
        &self,
        descriptor: WasiFilesystemTypesDescriptorBorrow,
    ) -> Result<
        Result<WasiFilesystemTypesDirectoryEntryStream, WasiFilesystemTypesErrorCode>,
        ComponentError,
    > {
        let entry = match self.descriptor_entry(descriptor.handle()) {
            Ok(entry) => entry,
            Err(error) => return Ok(Err(error)),
        };
        if !entry.flags.read {
            return Ok(Err(WasiFilesystemTypesErrorCode::NotPermitted));
        }
        if entry.descriptor_type != WasiFilesystemTypesDescriptorType::Directory {
            return Ok(Err(WasiFilesystemTypesErrorCode::NotDirectory));
        }

        let mut entries = Vec::new();
        let iter = fs::read_dir(&entry.host_path).map_err(|error| map_io_error(&error));
        let iter = match iter {
            Ok(iter) => iter,
            Err(error) => return Ok(Err(error)),
        };

        for child in iter {
            let child = match child {
                Ok(child) => child,
                Err(error) => return Ok(Err(map_io_error(&error))),
            };
            let name = child.file_name().to_string_lossy().into_owned();
            let child_type = match descriptor_type_for_path(&child.path(), false) {
                Ok(child_type) => child_type,
                Err(error) => return Ok(Err(error)),
            };
            entries.push(WasiFilesystemTypesDirectoryEntry {
                type_: child_type,
                name,
            });
        }

        let mut inner = self.state.inner.borrow_mut();
        let handle = next_handle(&mut inner.next_handle);
        inner
            .directory_entry_streams
            .insert(handle, DirectoryEntryStreamEntry { entries, cursor: 0 });
        Ok(Ok(WasiFilesystemTypesDirectoryEntryStream::new(handle)))
    }

    fn directory_entry_stream_read_directory_entry(
        &self,
        stream: WasiFilesystemTypesDirectoryEntryStreamBorrow,
    ) -> Result<
        Result<Option<WasiFilesystemTypesDirectoryEntry>, WasiFilesystemTypesErrorCode>,
        ComponentError,
    > {
        let mut inner = self.state.inner.borrow_mut();
        let entry = inner
            .directory_entry_streams
            .get_mut(&stream.handle())
            .ok_or_else(|| {
                ComponentError::Trap(format!(
                    "unknown directory-entry-stream handle {}",
                    stream.handle()
                ))
            })?;
        if entry.cursor >= entry.entries.len() {
            return Ok(Ok(None));
        }
        let value = entry.entries[entry.cursor].clone();
        entry.cursor += 1;
        Ok(Ok(Some(value)))
    }

    fn descriptor_child_path(
        &self,
        handle: u32,
        path: &str,
        follow_symlink: bool,
    ) -> Result<PathBuf, WasiFilesystemTypesErrorCode> {
        let entry = self.descriptor_entry(handle)?;
        if entry.descriptor_type != WasiFilesystemTypesDescriptorType::Directory {
            return Err(WasiFilesystemTypesErrorCode::NotDirectory);
        }
        resolve_guest_path(&entry.host_path, path, follow_symlink)
    }
}

impl filesystem_preopens::Host for WasiHost {
    fn get_directories(
        &self,
        _store: &Store,
    ) -> Result<Vec<(WasiFilesystemTypesDescriptor, String)>, ComponentError> {
        Ok(self.preopen_directories())
    }
}

impl filesystem_preopens::HostAsync for WasiHost {
    fn get_directories<'a>(
        &'a self,
        _store: &'a Store,
    ) -> ComponentFuture<'a, Result<Vec<(WasiFilesystemTypesDescriptor, String)>, ComponentError>>
    {
        Box::pin(async move { Ok(self.preopen_directories()) })
    }
}

impl filesystem_types::Host for WasiHost {
    fn descriptor_read_via_stream(
        &self,
        _store: &Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
        offset: u64,
    ) -> Result<Result<WasiIoStreamsInputStream, WasiFilesystemTypesErrorCode>, ComponentError>
    {
        self.descriptor_read_via_stream(self_, offset)
    }

    fn descriptor_get_type(
        &self,
        _store: &Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
    ) -> Result<
        Result<WasiFilesystemTypesDescriptorType, WasiFilesystemTypesErrorCode>,
        ComponentError,
    > {
        self.descriptor_get_type(self_)
    }

    fn descriptor_get_flags(
        &self,
        _store: &Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
    ) -> Result<
        Result<WasiFilesystemTypesDescriptorFlags, WasiFilesystemTypesErrorCode>,
        ComponentError,
    > {
        self.descriptor_get_flags(self_)
    }

    fn descriptor_read(
        &self,
        _store: &Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
        length: u64,
        offset: u64,
    ) -> Result<Result<(Vec<u8>, bool), WasiFilesystemTypesErrorCode>, ComponentError> {
        self.descriptor_read(self_, length, offset)
    }

    fn descriptor_read_directory(
        &self,
        _store: &Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
    ) -> Result<
        Result<WasiFilesystemTypesDirectoryEntryStream, WasiFilesystemTypesErrorCode>,
        ComponentError,
    > {
        self.descriptor_read_directory(self_)
    }

    fn descriptor_stat(
        &self,
        _store: &Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
    ) -> Result<
        Result<WasiFilesystemTypesDescriptorStat, WasiFilesystemTypesErrorCode>,
        ComponentError,
    > {
        self.descriptor_stat(self_)
    }

    fn descriptor_stat_at(
        &self,
        _store: &Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
        path_flags: WasiFilesystemTypesPathFlags,
        path: String,
    ) -> Result<
        Result<WasiFilesystemTypesDescriptorStat, WasiFilesystemTypesErrorCode>,
        ComponentError,
    > {
        self.descriptor_stat_at(self_, path_flags, path)
    }

    fn descriptor_open_at(
        &self,
        _store: &Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
        path_flags: WasiFilesystemTypesPathFlags,
        path: String,
        open_flags: WasiFilesystemTypesOpenFlags,
        flags: WasiFilesystemTypesDescriptorFlags,
    ) -> Result<Result<WasiFilesystemTypesDescriptor, WasiFilesystemTypesErrorCode>, ComponentError>
    {
        self.descriptor_open_at(self_, path_flags, path, open_flags, flags)
    }

    fn directory_entry_stream_read_directory_entry(
        &self,
        _store: &Store,
        self_: WasiFilesystemTypesDirectoryEntryStreamBorrow,
    ) -> Result<
        Result<Option<WasiFilesystemTypesDirectoryEntry>, WasiFilesystemTypesErrorCode>,
        ComponentError,
    > {
        self.directory_entry_stream_read_directory_entry(self_)
    }

    fn filesystem_error_code(
        &self,
        _store: &Store,
        err: WasiIoErrorErrorBorrow,
    ) -> Result<Option<WasiFilesystemTypesErrorCode>, ComponentError> {
        self.filesystem_error_code(err)
    }
}

impl filesystem_types::HostAsync for WasiHost {
    fn descriptor_read_via_stream<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
        offset: u64,
    ) -> ComponentFuture<
        'a,
        Result<Result<WasiIoStreamsInputStream, WasiFilesystemTypesErrorCode>, ComponentError>,
    > {
        Box::pin(async move { self.descriptor_read_via_stream(self_, offset) })
    }

    fn descriptor_get_type<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
    ) -> ComponentFuture<
        'a,
        Result<
            Result<WasiFilesystemTypesDescriptorType, WasiFilesystemTypesErrorCode>,
            ComponentError,
        >,
    > {
        Box::pin(async move { self.descriptor_get_type(self_) })
    }

    fn descriptor_get_flags<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
    ) -> ComponentFuture<
        'a,
        Result<
            Result<WasiFilesystemTypesDescriptorFlags, WasiFilesystemTypesErrorCode>,
            ComponentError,
        >,
    > {
        Box::pin(async move { self.descriptor_get_flags(self_) })
    }

    fn descriptor_read<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
        length: u64,
        offset: u64,
    ) -> ComponentFuture<
        'a,
        Result<Result<(Vec<u8>, bool), WasiFilesystemTypesErrorCode>, ComponentError>,
    > {
        Box::pin(async move { self.descriptor_read(self_, length, offset) })
    }

    fn descriptor_read_directory<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
    ) -> ComponentFuture<
        'a,
        Result<
            Result<WasiFilesystemTypesDirectoryEntryStream, WasiFilesystemTypesErrorCode>,
            ComponentError,
        >,
    > {
        Box::pin(async move { self.descriptor_read_directory(self_) })
    }

    fn descriptor_stat<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
    ) -> ComponentFuture<
        'a,
        Result<
            Result<WasiFilesystemTypesDescriptorStat, WasiFilesystemTypesErrorCode>,
            ComponentError,
        >,
    > {
        Box::pin(async move { self.descriptor_stat(self_) })
    }

    fn descriptor_stat_at<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
        path_flags: WasiFilesystemTypesPathFlags,
        path: String,
    ) -> ComponentFuture<
        'a,
        Result<
            Result<WasiFilesystemTypesDescriptorStat, WasiFilesystemTypesErrorCode>,
            ComponentError,
        >,
    > {
        Box::pin(async move { self.descriptor_stat_at(self_, path_flags, path) })
    }

    fn descriptor_open_at<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiFilesystemTypesDescriptorBorrow,
        path_flags: WasiFilesystemTypesPathFlags,
        path: String,
        open_flags: WasiFilesystemTypesOpenFlags,
        flags: WasiFilesystemTypesDescriptorFlags,
    ) -> ComponentFuture<
        'a,
        Result<Result<WasiFilesystemTypesDescriptor, WasiFilesystemTypesErrorCode>, ComponentError>,
    > {
        Box::pin(async move { self.descriptor_open_at(self_, path_flags, path, open_flags, flags) })
    }

    fn directory_entry_stream_read_directory_entry<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiFilesystemTypesDirectoryEntryStreamBorrow,
    ) -> ComponentFuture<
        'a,
        Result<
            Result<Option<WasiFilesystemTypesDirectoryEntry>, WasiFilesystemTypesErrorCode>,
            ComponentError,
        >,
    > {
        Box::pin(async move { self.directory_entry_stream_read_directory_entry(self_) })
    }

    fn filesystem_error_code<'a>(
        &'a self,
        _store: &'a Store,
        err: WasiIoErrorErrorBorrow,
    ) -> ComponentFuture<'a, Result<Option<WasiFilesystemTypesErrorCode>, ComponentError>> {
        Box::pin(async move { self.filesystem_error_code(err) })
    }
}

fn descriptor_type_for_path(
    path: &Path,
    follow_symlink: bool,
) -> Result<WasiFilesystemTypesDescriptorType, WasiFilesystemTypesErrorCode> {
    let metadata = if follow_symlink {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
    .map_err(|error| map_io_error(&error))?;

    let file_type = metadata.file_type();
    Ok(if file_type.is_dir() {
        WasiFilesystemTypesDescriptorType::Directory
    } else if file_type.is_file() {
        WasiFilesystemTypesDescriptorType::RegularFile
    } else if file_type.is_symlink() {
        WasiFilesystemTypesDescriptorType::SymbolicLink
    } else {
        WasiFilesystemTypesDescriptorType::Unknown
    })
}

fn read_file_range(
    path: &Path,
    offset: u64,
    length: u64,
) -> Result<(Vec<u8>, bool), WasiFilesystemTypesErrorCode> {
    let bytes = fs::read(path).map_err(|error| map_io_error(&error))?;
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    let end = start
        .saturating_add(usize::try_from(length).unwrap_or(usize::MAX))
        .min(bytes.len());
    Ok((bytes[start..end].to_vec(), end >= bytes.len()))
}

fn stat_for_path(
    path: &Path,
    follow_symlink: bool,
) -> Result<WasiFilesystemTypesDescriptorStat, WasiFilesystemTypesErrorCode> {
    let metadata = if follow_symlink {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
    .map_err(|error| map_io_error(&error))?;

    Ok(WasiFilesystemTypesDescriptorStat {
        type_: if metadata.file_type().is_dir() {
            WasiFilesystemTypesDescriptorType::Directory
        } else if metadata.file_type().is_file() {
            WasiFilesystemTypesDescriptorType::RegularFile
        } else if metadata.file_type().is_symlink() {
            WasiFilesystemTypesDescriptorType::SymbolicLink
        } else {
            WasiFilesystemTypesDescriptorType::Unknown
        },
        link_count: 1,
        size: metadata.len(),
        data_access_timestamp: metadata.accessed().ok().map(system_time_to_datetime),
        data_modification_timestamp: metadata.modified().ok().map(system_time_to_datetime),
        status_change_timestamp: None,
    })
}

fn system_time_to_datetime(time: std::time::SystemTime) -> WasiClocksWallClockDatetime {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    WasiClocksWallClockDatetime {
        seconds: duration.as_secs(),
        nanoseconds: duration.subsec_nanos(),
    }
}

fn resolve_guest_path(
    base: &Path,
    guest_path: &str,
    follow_symlink: bool,
) -> Result<PathBuf, WasiFilesystemTypesErrorCode> {
    let candidate = lexical_join(base, guest_path)?;
    let canonical_base = base.canonicalize().map_err(|error| map_io_error(&error))?;
    if follow_symlink {
        let resolved = candidate
            .canonicalize()
            .map_err(|error| map_io_error(&error))?;
        ensure_within_preopen(&canonical_base, &resolved)?;
        Ok(resolved)
    } else {
        let resolved_parent = candidate
            .parent()
            .unwrap_or(base)
            .canonicalize()
            .map_err(|error| map_io_error(&error))?;
        ensure_within_preopen(&canonical_base, &resolved_parent)?;
        Ok(candidate)
    }
}

fn ensure_within_preopen(
    canonical_base: &Path,
    resolved_path: &Path,
) -> Result<(), WasiFilesystemTypesErrorCode> {
    if resolved_path.starts_with(canonical_base) {
        Ok(())
    } else {
        Err(WasiFilesystemTypesErrorCode::NotPermitted)
    }
}

fn lexical_join(base: &Path, guest_path: &str) -> Result<PathBuf, WasiFilesystemTypesErrorCode> {
    let path = Path::new(guest_path);
    if path.is_absolute() {
        return Err(WasiFilesystemTypesErrorCode::NotPermitted);
    }

    let mut joined = base.to_path_buf();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => joined.push(part),
            Component::ParentDir => {
                if joined == base {
                    return Err(WasiFilesystemTypesErrorCode::NotPermitted);
                }
                joined.pop();
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(WasiFilesystemTypesErrorCode::NotPermitted);
            }
        }
    }
    Ok(joined)
}

pub(super) fn map_io_error(error: &std::io::Error) -> WasiFilesystemTypesErrorCode {
    use std::io::ErrorKind;

    match error.kind() {
        ErrorKind::NotFound => WasiFilesystemTypesErrorCode::NoEntry,
        ErrorKind::PermissionDenied => WasiFilesystemTypesErrorCode::Access,
        ErrorKind::AlreadyExists => WasiFilesystemTypesErrorCode::Exist,
        ErrorKind::InvalidInput | ErrorKind::InvalidData => WasiFilesystemTypesErrorCode::Invalid,
        ErrorKind::WouldBlock => WasiFilesystemTypesErrorCode::WouldBlock,
        ErrorKind::WriteZero | ErrorKind::UnexpectedEof | ErrorKind::BrokenPipe => {
            WasiFilesystemTypesErrorCode::Pipe
        }
        ErrorKind::NotADirectory => WasiFilesystemTypesErrorCode::NotDirectory,
        ErrorKind::IsADirectory => WasiFilesystemTypesErrorCode::IsDirectory,
        ErrorKind::Unsupported => WasiFilesystemTypesErrorCode::Unsupported,
        _ => WasiFilesystemTypesErrorCode::Io,
    }
}
