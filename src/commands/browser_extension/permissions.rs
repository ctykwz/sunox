use std::fs::File;
use std::path::Path;

use crate::core::CliError;

#[cfg(any(windows, test))]
const LOCAL_SYSTEM_SID: &str = "S-1-5-18";
#[cfg(any(windows, test))]
const BUILTIN_ADMINISTRATORS_SID: &str = "S-1-5-32-544";

#[cfg(any(windows, test))]
fn private_owner_is_allowed(actual_owner: &str, current_user: &str) -> bool {
    matches!(actual_owner, LOCAL_SYSTEM_SID | BUILTIN_ADMINISTRATORS_SID)
        || actual_owner == current_user
}

pub(super) fn harden_private_directory(path: &Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    {
        let directory = windows::open_private_object(path, true, true).map_err(|error| {
            CliError::Config(format!(
                "could not open private directory {} for protection: {error}",
                path.display()
            ))
        })?;
        windows::apply_private_dacl(&directory, true).map_err(|error| {
            CliError::Config(format!(
                "could not protect private directory {}: {error}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

pub(super) fn harden_private_file(path: &Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(windows)]
    {
        let file = windows::open_private_object(path, false, true).map_err(|error| {
            CliError::Config(format!(
                "could not open private file {} for protection: {error}",
                path.display()
            ))
        })?;
        windows::apply_private_dacl(&file, false).map_err(|error| {
            CliError::Config(format!(
                "could not protect private file {}: {error}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn open_and_harden_locked_directory(path: &Path) -> Result<File, CliError> {
    let directory = windows::open_private_object(path, true, false).map_err(|error| {
        CliError::Config(format!(
            "could not lock private directory {} against replacement: {error}",
            path.display()
        ))
    })?;
    windows::apply_private_dacl(&directory, true).map_err(|error| {
        CliError::Config(format!(
            "could not protect private directory {}: {error}",
            path.display()
        ))
    })?;
    Ok(directory)
}

pub(super) fn harden_private_file_handle(file: &File, _path: &Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(windows)]
    {
        let security_handle = windows::reopen_private_object(file, false).map_err(|error| {
            CliError::Config(format!(
                "could not reopen private file {} for protection: {error}",
                _path.display()
            ))
        })?;
        windows::apply_private_dacl(&security_handle, false).map_err(|error| {
            CliError::Config(format!(
                "could not protect private file {}: {error}",
                _path.display()
            ))
        })?;
    }
    Ok(())
}

#[cfg(all(test, windows))]
pub(super) fn assert_private_acl(path: &Path, directory: bool) {
    windows::assert_private_acl(path, directory);
}

#[cfg(all(test, windows))]
pub(super) fn make_world_readable_for_test(path: &Path, directory: bool) {
    windows::make_world_readable_for_test(path, directory);
}

#[cfg(windows)]
mod windows {
    use super::{BUILTIN_ADMINISTRATORS_SID, LOCAL_SYSTEM_SID, private_owner_is_allowed};
    use std::ffi::c_void;
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::iter;
    use std::mem::size_of;
    #[cfg(test)]
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::path::Path;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HANDLE, INVALID_HANDLE_VALUE,
        LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
    };
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, OWNER_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, READ_CONTROL, ReOpenFile, WRITE_DAC,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    struct LocalAllocation(*mut c_void);

    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    LocalFree(self.0);
                }
            }
        }
    }

    pub(super) fn open_private_object(
        path: &Path,
        directory: bool,
        share_delete: bool,
    ) -> io::Result<File> {
        let share_mode =
            FILE_SHARE_READ | FILE_SHARE_WRITE | if share_delete { FILE_SHARE_DELETE } else { 0 };
        let mut options = OpenOptions::new();
        options
            .read(true)
            .access_mode(FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC)
            .share_mode(share_mode)
            .custom_flags(
                FILE_FLAG_OPEN_REPARSE_POINT
                    | if directory {
                        FILE_FLAG_BACKUP_SEMANTICS
                    } else {
                        0
                    },
            );
        options.open(path)
    }

    pub(super) fn reopen_private_object(file: &File, directory: bool) -> io::Result<File> {
        let handle = unsafe {
            ReOpenFile(
                file.as_raw_handle(),
                READ_CONTROL | WRITE_DAC,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    pub(super) fn apply_private_dacl(file: &File, directory: bool) -> io::Result<()> {
        apply_dacl(file, directory, false)
    }

    fn apply_dacl(file: &File, directory: bool, world_readable: bool) -> io::Result<()> {
        let user_sid = current_user_sid_string()?;
        verify_allowed_owner(file, &user_sid)?;
        let inheritance = if directory { "OICI" } else { "" };
        let world_ace = if world_readable {
            format!("(A;{inheritance};GR;;;WD)")
        } else {
            String::new()
        };
        let descriptor = format!(
            "D:P(A;{inheritance};FA;;;{user_sid})(A;{inheritance};FA;;;{LOCAL_SYSTEM_SID})(A;{inheritance};FA;;;{BUILTIN_ADMINISTRATORS_SID}){world_ace}"
        );
        let descriptor = wide_string(&descriptor);
        let mut security_descriptor = null_mut();
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                descriptor.as_ptr(),
                SDDL_REVISION_1,
                &mut security_descriptor,
                null_mut(),
            )
        };
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        if security_descriptor.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned an empty security descriptor",
            ));
        }
        let _security_descriptor = LocalAllocation(security_descriptor);

        let mut dacl_present = 0;
        let mut dacl_defaulted = 0;
        let mut dacl = null_mut();
        let found_dacl = unsafe {
            GetSecurityDescriptorDacl(
                security_descriptor,
                &mut dacl_present,
                &mut dacl,
                &mut dacl_defaulted,
            )
        };
        if found_dacl == 0 {
            return Err(io::Error::last_os_error());
        }
        if dacl_present == 0 || dacl.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "generated security descriptor has no DACL",
            ));
        }

        let status = unsafe {
            SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        Ok(())
    }

    fn verify_allowed_owner(file: &File, current_user: &str) -> io::Result<()> {
        let mut owner = null_mut();
        let mut security_descriptor = null_mut();
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                null_mut(),
                null_mut(),
                &mut security_descriptor,
            )
        };
        let _security_descriptor = LocalAllocation(security_descriptor);
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        if security_descriptor.is_null() || owner.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned an empty object owner",
            ));
        }
        let actual_owner = sid_to_string(owner)?;
        if !private_owner_is_allowed(&actual_owner, current_user) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "refusing to change a private object's DACL because its owner {actual_owner} is not the current user, LocalSystem, or Builtin Administrators"
                ),
            ));
        }
        Ok(())
    }

    fn current_user_sid_string() -> io::Result<String> {
        let mut token = null_mut();
        let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
        if opened == 0 {
            return Err(io::Error::last_os_error());
        }
        let _token = Handle(token);

        let mut required_bytes = 0;
        let queried = unsafe {
            windows_sys::Win32::Security::GetTokenInformation(
                token,
                TokenUser,
                null_mut(),
                0,
                &mut required_bytes,
            )
        };
        if queried != 0 || required_bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned an invalid token-user size",
            ));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
            return Err(error);
        }
        if (required_bytes as usize) < size_of::<TOKEN_USER>() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned a truncated token-user record",
            ));
        }

        // TOKEN_USER contains a pointer, so use pointer-sized storage rather
        // than a byte vector to guarantee suitable alignment.
        let word_count = (required_bytes as usize).div_ceil(size_of::<usize>());
        let mut token_user = vec![0usize; word_count];
        let queried = unsafe {
            windows_sys::Win32::Security::GetTokenInformation(
                token,
                TokenUser,
                token_user.as_mut_ptr().cast(),
                required_bytes,
                &mut required_bytes,
            )
        };
        if queried == 0 {
            return Err(io::Error::last_os_error());
        }
        let token_user = unsafe { &*(token_user.as_ptr().cast::<TOKEN_USER>()) };
        if token_user.User.Sid.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows token has no user SID",
            ));
        }
        sid_to_string(token_user.User.Sid)
    }

    fn sid_to_string(sid: PSID) -> io::Result<String> {
        let mut string_sid = null_mut();
        let converted = unsafe { ConvertSidToStringSidW(sid, &mut string_sid) };
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        if string_sid.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned an empty SID string",
            ));
        }
        let _string_sid = LocalAllocation(string_sid.cast());
        let mut length = 0;
        unsafe {
            while *string_sid.add(length) != 0 {
                length += 1;
            }
            String::from_utf16(std::slice::from_raw_parts(string_sid, length))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        }
    }

    #[cfg(test)]
    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(iter::once(0))
            .collect()
    }

    fn wide_string(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(iter::once(0)).collect()
    }

    #[cfg(test)]
    pub(super) fn assert_private_acl(path: &Path, directory: bool) {
        use std::collections::BTreeSet;
        use std::mem::size_of_val;
        use std::ptr::addr_of;

        use windows_sys::Win32::Security::Authorization::GetNamedSecurityInfoW;
        use windows_sys::Win32::Security::{
            ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
            CONTAINER_INHERIT_ACE, GetAce, GetAclInformation, GetSecurityDescriptorControl,
            INHERITED_ACE, OBJECT_INHERIT_ACE, SE_DACL_PROTECTED,
        };
        use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
        use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;

        let path = wide_path(path);
        let mut owner = null_mut();
        let mut dacl: *mut ACL = null_mut();
        let mut security_descriptor = null_mut();
        let status = unsafe {
            GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut security_descriptor,
            )
        };
        assert_eq!(
            status,
            ERROR_SUCCESS,
            "could not inspect private ACL: {}",
            io::Error::from_raw_os_error(status as i32)
        );
        assert!(!security_descriptor.is_null());
        let _security_descriptor = LocalAllocation(security_descriptor);
        assert!(!dacl.is_null());

        let mut control = 0;
        let mut revision = 0;
        let found_control = unsafe {
            GetSecurityDescriptorControl(security_descriptor, &mut control, &mut revision)
        };
        assert_ne!(
            found_control,
            0,
            "could not inspect security descriptor control: {}",
            io::Error::last_os_error()
        );
        assert_ne!(
            control & SE_DACL_PROTECTED,
            0,
            "private DACL still inherits from its parent"
        );

        let mut acl_size = ACL_SIZE_INFORMATION::default();
        let found_acl = unsafe {
            GetAclInformation(
                dacl,
                (&mut acl_size as *mut ACL_SIZE_INFORMATION).cast(),
                size_of_val(&acl_size) as u32,
                AclSizeInformation,
            )
        };
        assert_ne!(
            found_acl,
            0,
            "could not inspect DACL entries: {}",
            io::Error::last_os_error()
        );
        assert_eq!(acl_size.AceCount, 3, "private DACL has unexpected ACEs");

        let user_sid = current_user_sid_string().expect("current user SID");
        assert!(!owner.is_null());
        let owner_sid = sid_to_string(owner).expect("owner SID");
        assert!(
            private_owner_is_allowed(&owner_sid, &user_sid),
            "private object owner {owner_sid} is not one of its allowed full-control trustees"
        );
        let expected = BTreeSet::from([
            user_sid,
            LOCAL_SYSTEM_SID.to_string(),
            BUILTIN_ADMINISTRATORS_SID.to_string(),
        ]);
        let inheritance = (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u8;
        let mut trustees = Vec::new();
        for index in 0..acl_size.AceCount {
            let mut raw_ace = null_mut();
            let found_ace = unsafe { GetAce(dacl, index, &mut raw_ace) };
            assert_ne!(
                found_ace,
                0,
                "could not inspect DACL entry {index}: {}",
                io::Error::last_os_error()
            );
            let header = unsafe { &*(raw_ace.cast::<ACE_HEADER>()) };
            assert_eq!(
                header.AceType, ACCESS_ALLOWED_ACE_TYPE as u8,
                "private DACL contains a non-allow ACE"
            );
            assert!(
                header.AceSize as usize >= size_of::<ACCESS_ALLOWED_ACE>(),
                "private DACL contains a truncated allow ACE"
            );
            let ace = unsafe { &*(raw_ace.cast::<ACCESS_ALLOWED_ACE>()) };
            assert_eq!(
                ace.Header.AceFlags & INHERITED_ACE as u8,
                0,
                "private DACL contains an inherited ACE"
            );
            if directory {
                assert_eq!(
                    ace.Header.AceFlags & inheritance,
                    inheritance,
                    "directory ACE does not protect descendants"
                );
            } else {
                assert_eq!(
                    ace.Header.AceFlags & inheritance,
                    0,
                    "file ACE unexpectedly has inheritance flags"
                );
            }
            assert_eq!(
                ace.Mask, FILE_ALL_ACCESS,
                "private DACL does not grant the intended full-control mask"
            );
            let sid = addr_of!(ace.SidStart) as PSID;
            trustees.push(sid_to_string(sid).expect("ACE trustee SID"));
        }

        assert!(
            trustees.iter().all(|sid| expected.contains(sid)),
            "private DACL contains an unexpected trustee: {trustees:?}"
        );
        for sid in expected {
            assert!(
                trustees.contains(&sid),
                "private DACL is missing required trustee {sid}"
            );
        }
        assert!(
            !trustees.iter().any(|sid| sid == "S-1-1-0"),
            "Everyone must not have an ACE on a private object"
        );
    }

    #[cfg(test)]
    pub(super) fn make_world_readable_for_test(path: &Path, directory: bool) {
        let file =
            open_private_object(path, directory, true).expect("open fixture for ACL mutation");
        apply_dacl(&file, directory, true).expect("add explicit Everyone read ACE");
    }
}

#[cfg(test)]
mod tests {
    use super::{BUILTIN_ADMINISTRATORS_SID, LOCAL_SYSTEM_SID, private_owner_is_allowed};

    const CURRENT_USER_SID: &str = "S-1-5-21-1000-1000-1000-1001";

    #[test]
    fn private_owner_allowlist_matches_every_full_control_trustee() {
        assert!(private_owner_is_allowed(CURRENT_USER_SID, CURRENT_USER_SID));
        assert!(private_owner_is_allowed(LOCAL_SYSTEM_SID, CURRENT_USER_SID));
        assert!(private_owner_is_allowed(
            BUILTIN_ADMINISTRATORS_SID,
            CURRENT_USER_SID
        ));
    }

    #[test]
    fn private_owner_allowlist_rejects_unrelated_trustees() {
        assert!(!private_owner_is_allowed(
            "S-1-5-21-2000-2000-2000-2002",
            CURRENT_USER_SID
        ));
        assert!(!private_owner_is_allowed("S-1-1-0", CURRENT_USER_SID));
    }
}
