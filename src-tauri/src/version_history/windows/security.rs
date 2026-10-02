//! Current-token identity and protected, current-user-only object security.
use super::{blocked, handle, own, win_error};
use std::{io, mem::size_of, os::windows::io::OwnedHandle, ptr};
use windows::Win32::{
    Foundation::{LocalFree, ERROR_INSUFFICIENT_BUFFER, HANDLE, HLOCAL},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SE_FILE_OBJECT, SE_KERNEL_OBJECT, SE_OBJECT_TYPE,
        },
        EqualSid, GetAce, GetLengthSid, GetSecurityDescriptorControl, GetTokenInformation,
        IsValidSecurityDescriptor, IsValidSid, TokenElevation, TokenUser, ACCESS_ALLOWED_ACE, ACL,
        DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        SECURITY_ATTRIBUTES, SE_DACL_PROTECTED, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
    },
    Storage::FileSystem::FILE_ALL_ACCESS,
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};
use windows_core::{PCWSTR, PWSTR};

/// Retains a copied SID, not a pointer into a temporary token buffer. Merely
/// capturing the token does not approve elevation or installation scope.
pub(crate) struct CurrentUser {
    sid: Vec<u32>,
    text: String,
    elevated: bool,
    _token: OwnedHandle,
}
impl CurrentUser {
    pub(crate) fn capture() -> io::Result<Self> {
        unsafe {
            let mut raw = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw).map_err(win_error)?;
            let token = own(raw);
            let mut length = 0;
            let sizing = GetTokenInformation(handle(&token), TokenUser, None, 0, &mut length);
            if sizing.is_ok()
                || sizing.err().map(|e| e.code()) != Some(ERROR_INSUFFICIENT_BUFFER.to_hresult())
                || length == 0
                || length > 65536
            {
                return Err(blocked("unsupported token identity"));
            }
            let mut buffer = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
            GetTokenInformation(
                handle(&token),
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                length,
                &mut length,
            )
            .map_err(win_error)?;
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            if !IsValidSid(user.User.Sid).as_bool() {
                return Err(blocked("invalid token SID"));
            }
            let sid_length = GetLengthSid(user.User.Sid) as usize;
            if sid_length == 0 || sid_length > 1024 {
                return Err(blocked("unsupported token SID"));
            }
            let mut sid = vec![0u32; sid_length.div_ceil(4)];
            ptr::copy_nonoverlapping(
                user.User.Sid.0.cast::<u8>(),
                sid.as_mut_ptr().cast(),
                sid_length,
            );
            let mut text = PWSTR::null();
            ConvertSidToStringSidW(PSID(sid.as_mut_ptr().cast()), &mut text).map_err(win_error)?;
            let allocation = LocalAllocation(text.0.cast());
            let text = text
                .to_string()
                .map_err(|_| blocked("unrepresentable token SID"))?;
            drop(allocation);
            let mut elevation = TOKEN_ELEVATION::default();
            GetTokenInformation(
                handle(&token),
                TokenElevation,
                Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut length,
            )
            .map_err(win_error)?;
            Ok(Self {
                sid,
                text,
                elevated: elevation.TokenIsElevated != 0,
                _token: token,
            })
        }
    }
    pub(crate) fn require_unelevated(&self) -> io::Result<()> {
        if self.elevated || !cfg!(target_arch = "x86_64") {
            return Err(blocked("recovery requires an unelevated x64 user"));
        }
        Ok(())
    }
    pub(crate) fn sid_text(&self) -> &str {
        &self.text
    }
    fn sid(&self) -> PSID {
        PSID(self.sid.as_ptr().cast_mut().cast())
    }
    pub(crate) fn descriptor(&self, inherit_files: bool) -> io::Result<PrivateDescriptor> {
        let flags = if inherit_files { "OICI" } else { "" };
        let rights = "FA";
        let text: Vec<u16> = format!("O:{}D:P(A;{};{};;;{})", self.text, flags, rights, self.text)
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                1,
                &mut descriptor,
                None,
            )
            .map_err(win_error)?;
        }
        Ok(PrivateDescriptor(LocalAllocation(descriptor.0)))
    }
    pub(crate) fn job_descriptor(&self) -> io::Result<PrivateDescriptor> {
        let text: Vec<u16> = format!("O:{}D:P(A;;GA;;;{})", self.text, self.text)
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                1,
                &mut descriptor,
                None,
            )
            .map_err(win_error)?;
        }
        Ok(PrivateDescriptor(LocalAllocation(descriptor.0)))
    }
    pub(crate) fn verify_private_file(&self, file: HANDLE, directory: bool) -> io::Result<()> {
        self.verify_private(
            file,
            SE_FILE_OBJECT,
            if directory { 3 } else { 0 },
            FILE_ALL_ACCESS.0,
        )
    }
    pub(crate) fn verify_private_job(&self, job: HANDLE) -> io::Result<()> {
        // JOB_OBJECT_ALL_ACCESS: STANDARD_RIGHTS_REQUIRED | SYNCHRONIZE | 0x3f.
        self.verify_private(job, SE_KERNEL_OBJECT, 0, 0x001f003f)
    }
    fn verify_private(
        &self,
        file: HANDLE,
        object: SE_OBJECT_TYPE,
        flags: u8,
        mask: u32,
    ) -> io::Result<()> {
        unsafe {
            let mut owner = PSID::default();
            let mut acl: *mut ACL = ptr::null_mut();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            GetSecurityInfo(
                file,
                object,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                Some(&mut owner),
                None,
                Some(&mut acl),
                None,
                Some(&mut descriptor),
            )
            .ok()
            .map_err(win_error)?;
            let _allocation = LocalAllocation(descriptor.0);
            if descriptor.0.is_null()
                || acl.is_null()
                || !IsValidSecurityDescriptor(descriptor).as_bool()
                || EqualSid(owner, self.sid()).is_err()
                || (*acl).AceCount != 1
            {
                return Err(blocked("recovery object owner or ACL differs"));
            }
            let mut control = 0;
            let mut revision = 0;
            GetSecurityDescriptorControl(descriptor, &mut control, &mut revision)
                .map_err(win_error)?;
            if control & SE_DACL_PROTECTED.0 == 0 {
                return Err(blocked("recovery ACL is not protected"));
            }
            let mut ace = ptr::null_mut();
            GetAce(acl, 0, &mut ace).map_err(win_error)?;
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            if ace.Header.AceType != 0
                || ace.Header.AceFlags != flags
                || ace.Header.AceSize < size_of::<ACCESS_ALLOWED_ACE>() as u16
                || ace.Mask != mask
                || EqualSid(
                    PSID((&ace.SidStart as *const u32).cast_mut().cast()),
                    self.sid(),
                )
                .is_err()
            {
                return Err(blocked("recovery ACL is not current-user-only"));
            }
            Ok(())
        }
    }
}
pub(super) struct LocalAllocation(pub(super) *mut core::ffi::c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0)));
        }
    }
}
pub(crate) struct PrivateDescriptor(LocalAllocation);
impl PrivateDescriptor {
    pub(super) fn pointer(&self) -> *const windows::Win32::Security::SECURITY_DESCRIPTOR {
        self.0 .0.cast()
    }
    pub(super) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0 .0,
            bInheritHandle: false.into(),
        }
    }
}
