//! Seccomp-BPF syscall filter for the [`BwrapSandbox`](crate::BwrapSandbox) backend
//! (C23-3b — the tuned-syscall part of the process/syscall pillar).
//!
//! bubblewrap already drops host *capabilities* via rootless namespaces, but the
//! child can still issue the full syscall surface. This module builds a
//! **default-allow + curated deny-list** seccomp filter (the flatpak/bubblewrap
//! shape): everything is permitted *except* a fixed set of dangerous syscalls, each
//! of which returns **`EPERM`** rather than `SIGSYS`-killing the process — so a
//! build/test toolchain that merely *probes* for a denied capability degrades
//! gracefully instead of dying, and no legitimate compiler/test syscall is blocked.
//!
//! The compiled BPF program is handed to bwrap over an inherited file descriptor
//! (`--seccomp <fd>`); bwrap installs it on the child right before `exec`, after its
//! own namespace setup (so denying `mount`/`pivot_root` here never breaks bwrap's
//! own bind mounts — only the child is constrained).
//!
//! **Fail-closed:** every step (unsupported arch, filter build, BPF compile, empty
//! program, `memfd`/`write`/`lseek`) returns `Err`; the caller (`BwrapSandbox::exec`)
//! turns that into a sandbox error *before* spawning, so the untrusted child never
//! runs un-filtered. Pure-Rust (`seccompiler`) + `libc` for the anonymous fd — no
//! system `libseccomp`, matching the crate's hermetic/pure-Rust convention.

use agent_core::{Error, Result};
use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, SeccompRule, TargetArch};
use std::collections::BTreeMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

/// The curated deny-list: dangerous syscalls with no place in reviewed-code build/test
/// runs. Each entry is `(name, number)` where the number comes from `libc::SYS_*` for
/// the **compile-time arch** (so it is always correct for the running binary; the name
/// is carried only for readability + the membership test). Mirrors flatpak's seccomp
/// deny list. Kept to syscalls that exist on every arch we target (x86_64, aarch64) so
/// the crate compiles on both; `EPERM`, not kill, so a probe degrades not dies.
/// (`libc::SYS_*` is `c_long` = `i64` on every 64-bit Linux target we support, so the
/// numbers slot straight into seccompiler's `i64`-keyed rule map.)
fn denied_syscalls() -> Vec<(&'static str, i64)> {
    vec![
        // Kernel keyring — credential exfiltration / poisoning surface.
        ("add_key", libc::SYS_add_key),
        ("keyctl", libc::SYS_keyctl),
        ("request_key", libc::SYS_request_key),
        // Cross-process memory / tracing — sandbox-escape and secret-theft surface.
        ("ptrace", libc::SYS_ptrace),
        ("process_vm_readv", libc::SYS_process_vm_readv),
        ("process_vm_writev", libc::SYS_process_vm_writev),
        // Mount table manipulation — filesystem-boundary escape surface.
        ("mount", libc::SYS_mount),
        ("umount2", libc::SYS_umount2),
        ("pivot_root", libc::SYS_pivot_root),
        // Swap / power / kexec — host-integrity + DoS surface.
        ("swapon", libc::SYS_swapon),
        ("swapoff", libc::SYS_swapoff),
        ("reboot", libc::SYS_reboot),
        ("kexec_load", libc::SYS_kexec_load),
        ("kexec_file_load", libc::SYS_kexec_file_load),
        // Kernel module (un)loading — arbitrary-kernel-code surface.
        ("init_module", libc::SYS_init_module),
        ("finit_module", libc::SYS_finit_module),
        ("delete_module", libc::SYS_delete_module),
        // fd-by-handle — bypasses path-based confinement.
        ("open_by_handle_at", libc::SYS_open_by_handle_at),
        ("name_to_handle_at", libc::SYS_name_to_handle_at),
        // eBPF / perf — kernel-programming + broad-observability surface.
        ("bpf", libc::SYS_bpf),
        ("perf_event_open", libc::SYS_perf_event_open),
        // Clock / accounting / quota — host-state tampering surface.
        ("settimeofday", libc::SYS_settimeofday),
        ("clock_settime", libc::SYS_clock_settime),
        ("acct", libc::SYS_acct),
        ("quotactl", libc::SYS_quotactl),
    ]
}

/// Map a Rust target-arch string ([`std::env::consts::ARCH`]) to a seccompiler
/// [`TargetArch`]. **Fail-closed** on anything we don't target: an unrecognised arch
/// yields `Err`, so `exec` refuses to run rather than installing a wrong-arch (⇒
/// ineffective) filter. Factored out so the failure path is unit-testable.
fn target_arch(arch: &str) -> Result<TargetArch> {
    match arch {
        "x86_64" => Ok(TargetArch::x86_64),
        "aarch64" => Ok(TargetArch::aarch64),
        other => Err(Error::Sandbox(format!(
            "seccomp: unsupported target arch `{other}` (no filter installed)"
        ))),
    }
}

/// Compile the default-allow + deny-list filter to the raw `struct sock_filter[]` byte
/// stream bubblewrap reads from the `--seccomp` fd (host-native, 8 bytes per
/// instruction). Fail-closed on every error, and rejects an empty program (a filter
/// that would enforce nothing).
fn seccomp_bpf() -> Result<Vec<u8>> {
    let arch = target_arch(std::env::consts::ARCH)?;
    // Each denied syscall maps to an EMPTY rule vector = "match unconditionally", so
    // the match action (Errno) applies whenever it is called; everything else falls
    // through to the mismatch action (Allow).
    let rules: BTreeMap<i64, Vec<SeccompRule>> = denied_syscalls()
        .into_iter()
        .map(|(_, nr)| (nr, Vec::new()))
        .collect();
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,                     // mismatch (un-listed) ⇒ allow
        SeccompAction::Errno(libc::EPERM as u32), // match (listed) ⇒ EPERM, not kill
        arch,
    )
    .map_err(|e| Error::Sandbox(format!("seccomp: building filter: {e}")))?;
    let prog: BpfProgram = filter
        .try_into()
        .map_err(|e| Error::Sandbox(format!("seccomp: compiling BPF: {e}")))?;
    if prog.is_empty() {
        return Err(Error::Sandbox(
            "seccomp: refusing to install an empty BPF program".into(),
        ));
    }
    // `BpfProgram` is `Vec<sock_filter>`; `sock_filter` is a fixed-size `repr(C)` POD
    // (u16 code, u8 jt, u8 jf, u32 k = 8 bytes). bwrap derives the instruction count
    // from the byte length, host-native — so a flat byte view of the slice is exactly
    // the wire format. We never name the element type (size_of_val on the slice).
    let byte_len = std::mem::size_of_val(prog.as_slice());
    // SAFETY: `prog` is a live `Vec` of `repr(C)` POD; reading its backing store as
    // `byte_len` bytes is in-bounds and every bit pattern is a valid `u8`.
    let bytes =
        unsafe { std::slice::from_raw_parts(prog.as_ptr().cast::<u8>(), byte_len) }.to_vec();
    Ok(bytes)
}

/// Build the seccomp profile and stage it in an anonymous in-memory file, returning an
/// owned fd positioned at offset 0. The fd is created **without** `MFD_CLOEXEC` so it
/// is inherited across the `bwrap` spawn (the caller keeps the [`OwnedFd`] alive for
/// the whole exec and passes its number to `--seccomp`). All errors fail closed.
pub(crate) fn seccomp_memfd() -> Result<OwnedFd> {
    let bytes = seccomp_bpf()?;
    // SAFETY: a valid NUL-terminated name + flags=0. Returns a fresh fd or -1/errno.
    // flags=0 ⇒ the fd is NOT close-on-exec, so the spawned bwrap inherits it.
    let raw = unsafe { libc::memfd_create(c"agent-seccomp".as_ptr(), 0) };
    if raw < 0 {
        return Err(Error::Sandbox(format!(
            "seccomp: memfd_create: {}",
            std::io::Error::last_os_error()
        )));
    }
    // Own the fd immediately so any early return below closes it (no leak on error).
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let mut off = 0usize;
    while off < bytes.len() {
        // SAFETY: writing `bytes[off..]` (valid for `len-off` bytes) into our own memfd.
        let n = unsafe {
            libc::write(
                fd.as_raw_fd(),
                bytes[off..].as_ptr().cast::<libc::c_void>(),
                bytes.len() - off,
            )
        };
        if n < 0 {
            return Err(Error::Sandbox(format!(
                "seccomp: write: {}",
                std::io::Error::last_os_error()
            )));
        }
        off += n as usize;
    }
    // Rewind so bwrap reads the program from the start.
    // SAFETY: seeking our own fd to an absolute offset.
    if unsafe { libc::lseek(fd.as_raw_fd(), 0, libc::SEEK_SET) } < 0 {
        return Err(Error::Sandbox(format!(
            "seccomp: lseek: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(fd)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// desc: the deny-list is non-empty, name↔number consistent, and every number
    /// resolves to a distinct syscall (no accidental duplicate/aliased entry).
    #[test]
    fn positive_denied_syscalls_are_distinct_and_nonempty() {
        let denied = denied_syscalls();
        assert!(!denied.is_empty(), "the deny-list must not be empty");
        let mut nums: Vec<i64> = denied.iter().map(|(_, n)| *n).collect();
        nums.sort_unstable();
        let before = nums.len();
        nums.dedup();
        assert_eq!(
            before,
            nums.len(),
            "deny-list has a duplicate syscall number"
        );
    }

    /// desc: the curated set covers the syscalls we mean to block (a representative
    /// subset — keyring, ptrace, mount, module load, bpf), by name.
    #[test]
    fn positive_denylist_covers_dangerous_syscalls() {
        let names: Vec<&str> = denied_syscalls().iter().map(|(n, _)| *n).collect();
        for want in [
            "add_key",
            "keyctl",
            "ptrace",
            "mount",
            "umount2",
            "init_module",
            "bpf",
            "perf_event_open",
            "kexec_load",
        ] {
            assert!(
                names.contains(&want),
                "deny-list must include {want}: {names:?}"
            );
        }
    }

    /// desc: arch mapping is fail-closed — the two supported arches resolve, anything
    /// else errors (so `exec` refuses rather than installing a wrong-arch filter).
    #[test]
    fn negative_unknown_arch_fails_closed() {
        assert!(target_arch("x86_64").is_ok());
        assert!(target_arch("aarch64").is_ok());
        for bad in ["mips", "riscv64", "", "X86_64", "amd64"] {
            assert!(target_arch(bad).is_err(), "arch `{bad}` must fail closed");
        }
    }

    /// desc (adversarial): the compiled BPF is well-formed — a non-empty program whose
    /// byte length is an exact multiple of the 8-byte `sock_filter` instruction size.
    /// A truncated/empty program can never be emitted (it would enforce nothing).
    #[test]
    fn adversarial_bpf_is_wellformed_nonempty() {
        // Runs on the gate arch (x86_64); on any supported arch it must succeed.
        let bytes = seccomp_bpf().expect("BPF compiles on a supported arch");
        assert!(!bytes.is_empty(), "BPF program must not be empty");
        assert_eq!(
            bytes.len() % 8,
            0,
            "BPF byte length must be a multiple of 8"
        );
    }

    /// desc: the staged profile lands in a readable anonymous fd positioned at 0, whose
    /// contents byte-match the compiled program (so bwrap reads the exact filter).
    #[test]
    fn positive_memfd_holds_the_program_at_offset_zero() {
        use std::io::Read;
        let want = seccomp_bpf().expect("BPF compiles");
        let fd = seccomp_memfd().expect("memfd staged");
        // Read the fd back from the start (seccomp_memfd rewinds it).
        let mut f = std::fs::File::from(fd);
        let mut got = Vec::new();
        f.read_to_end(&mut got).expect("read memfd");
        assert_eq!(got, want, "memfd contents must equal the compiled BPF");
    }
}
