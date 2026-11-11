//! Keeping `CAP_SYS_PTRACE` out of the effective set except while the scan
//! opens the game's memory.
//!
//! A locally built binary may carry `cap_sys_ptrace` as a file capability, and
//! with the file's effective flag the kernel makes it effective from `exec` on.
//! That would leave the whole GUI - a networked webview that loads GTK modules
//! and plugins - able to trace any process on the machine, root's included, for
//! as long as it runs, when only the moment `/proc/<pid>/mem` is opened needs
//! it: the kernel checks ptrace access once at open and later reads use the
//! address space it pinned then.
//!
//! What this buys is limited, and the docs say so. Lowering the effective bit
//! stops code that merely runs in the process from using the capability by
//! accident. It does not stop code injected into the process, which can raise a
//! permitted capability again with one `capset`. Dropping it from the permitted
//! set instead would make the next scan impossible.
//!
//! Capability sets are per thread, and the raw `capget`/`capset` syscalls act
//! on the calling thread only. Threads inherit their creator's sets, so the
//! startup drop must run before any thread exists; the scan then raises and
//! lowers on whichever worker thread runs it.

/// Called once, first thing in `main`, before any thread is spawned and before
/// GTK or Tauri initialises. A no-op off Linux and wherever the process holds
/// no permitted `CAP_SYS_PTRACE` - the AppImage and `ptrace_scope` cases.
pub fn restrict_ptrace_at_startup() {
    #[cfg(target_os = "linux")]
    linux::restrict_at_startup();
}

#[cfg(target_os = "linux")]
pub(crate) use linux::PtraceRaised;

#[cfg(target_os = "linux")]
mod linux {
    use std::io;
    use std::marker::PhantomData;

    /// `_LINUX_CAPABILITY_VERSION_3` from `<linux/capability.h>`; `libc` does
    /// not export it. Version 3 carries 64 capability bits as two 32-bit words.
    const CAPABILITY_VERSION_3: u32 = 0x2008_0522;
    /// `CAP_SYS_PTRACE` from `<linux/capability.h>`; below 32, so word 0.
    const CAP_SYS_PTRACE: u32 = 19;
    const PTRACE_BIT: u32 = 1 << CAP_SYS_PTRACE;

    /// `struct __user_cap_header_struct`.
    #[repr(C)]
    struct CapHeader {
        version: u32,
        /// 0 names the calling thread.
        pid: libc::c_int,
    }

    /// `struct __user_cap_data_struct`; version 3 passes two of them.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub(crate) struct CapData {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }

    pub(crate) type CapSets = [CapData; 2];

    /// The calling thread's capability sets, behind a seam so the transitions
    /// can be tested without a capability-bearing binary.
    pub(crate) trait ThreadCaps {
        fn get(&self) -> io::Result<CapSets>;
        fn set(&self, sets: &CapSets) -> io::Result<()>;
    }

    pub(crate) struct Kernel;

    impl ThreadCaps for Kernel {
        fn get(&self) -> io::Result<CapSets> {
            let mut header = CapHeader {
                version: CAPABILITY_VERSION_3,
                pid: 0,
            };
            let mut sets = CapSets::default();
            // SAFETY: capget writes one header and, for version 3, exactly two
            // data structs; both pointers are to live locals of those
            // `#[repr(C)]` layouts. A kernel that does not speak version 3
            // rewrites `version` and fails with EINVAL, which is returned.
            let rc = unsafe {
                libc::syscall(
                    libc::SYS_capget,
                    &mut header as *mut CapHeader,
                    sets.as_mut_ptr(),
                )
            };
            if rc == 0 {
                Ok(sets)
            } else {
                Err(io::Error::last_os_error())
            }
        }

        fn set(&self, sets: &CapSets) -> io::Result<()> {
            let mut header = CapHeader {
                version: CAPABILITY_VERSION_3,
                pid: 0,
            };
            // SAFETY: capset reads one header and two data structs from live
            // locals of the matching layouts. pid 0 confines the change to the
            // calling thread, which is the intent.
            let rc = unsafe {
                libc::syscall(
                    libc::SYS_capset,
                    &mut header as *mut CapHeader,
                    sets.as_ptr(),
                )
            };
            if rc == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
    }

    /// `sets` with the ptrace bit made effective or not, or `None` when there
    /// is nothing to do: the bit is not permitted (raising it would only fail),
    /// or it is already in the wanted state.
    pub(super) fn with_ptrace_effective(sets: CapSets, effective: bool) -> Option<CapSets> {
        let [mut low, high] = sets;
        if low.permitted & PTRACE_BIT == 0 {
            return None;
        }
        let now = low.effective & PTRACE_BIT != 0;
        if now == effective {
            return None;
        }
        if effective {
            low.effective |= PTRACE_BIT;
        } else {
            low.effective &= !PTRACE_BIT;
        }
        Some([low, high])
    }

    /// Returns whether the bit is effective on this thread afterwards. A
    /// failure is not reported: without the capability the open fails with
    /// the ordinary permission error and its guidance, which is the right
    /// outcome, and a scan must never fail here.
    pub(super) fn set_ptrace_effective(caps: &impl ThreadCaps, effective: bool) -> bool {
        let Ok(sets) = caps.get() else { return false };
        let before = sets[0].effective & PTRACE_BIT != 0;
        match with_ptrace_effective(sets, effective) {
            Some(next) if caps.set(&next).is_ok() => effective,
            _ => before,
        }
    }

    /// Module search paths and module lists that the desktop's libraries load
    /// code from and still honour in secure-execution mode. Each was read in
    /// the library's source: GTK 3 refuses to start only when the user ids
    /// differ (`check_setugid` in gtkmain.c), which a file capability does not
    /// cause, and none of these reads checks `AT_SECURE`. GLib's own
    /// `GIO_EXTRA_MODULES` and `GIO_MODULE_DIR` are absent because GLib already
    /// ignores them when `g_check_setuid()` - `getauxval(AT_SECURE)` on Linux -
    /// is true. The dynamic loader drops `LD_PRELOAD` and `LD_LIBRARY_PATH`
    /// itself.
    const MODULE_LOADING_VARS: [&str; 12] = [
        // gtkmain.c: module names loaded at gtk_init.
        "GTK_MODULES",
        "GTK3_MODULES",
        // gtkmodules.c: where those modules and the print backends are found.
        "GTK_PATH",
        "GTK_EXE_PREFIX",
        // gtkrc.c: the cache naming the input-method .so files to dlopen.
        "GTK_IM_MODULE_FILE",
        // gdk-pixbuf-io.c: the cache naming the image loader .so files.
        "GDK_PIXBUF_MODULE_FILE",
        // gstregistry.c: plugin directories and the registry naming plugin .so
        // files, in case the UI process initialises GStreamer.
        "GST_PLUGIN_PATH",
        "GST_PLUGIN_PATH_1_0",
        "GST_PLUGIN_SYSTEM_PATH",
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        "GST_REGISTRY",
        "GST_REGISTRY_1_0",
    ];

    fn secure_execution() -> bool {
        // SAFETY: getauxval only reads the auxiliary vector the kernel placed
        // at exec; it returns 0 for an absent entry.
        unsafe { libc::getauxval(libc::AT_SECURE) != 0 }
    }

    pub(super) fn restrict_at_startup() {
        if secure_execution() {
            for name in MODULE_LOADING_VARS {
                // Sound only because no other thread exists yet: the caller
                // runs this first thing in main.
                std::env::remove_var(name);
            }
        }
        set_ptrace_effective(&Kernel, false);
    }

    /// `CAP_SYS_PTRACE` effective on this thread until dropped.
    ///
    /// Not `Send`: the drop must lower the bit on the thread that raised it,
    /// since `capset` only ever touches the calling thread.
    pub(crate) struct PtraceRaised<C: ThreadCaps = Kernel> {
        caps: C,
        _same_thread: PhantomData<*const ()>,
    }

    impl PtraceRaised<Kernel> {
        pub(crate) fn raise() -> Self {
            Self::raise_with(Kernel)
        }
    }

    impl<C: ThreadCaps> PtraceRaised<C> {
        pub(super) fn raise_with(caps: C) -> Self {
            set_ptrace_effective(&caps, true);
            Self {
                caps,
                _same_thread: PhantomData,
            }
        }
    }

    impl<C: ThreadCaps> Drop for PtraceRaised<C> {
        fn drop(&mut self) {
            set_ptrace_effective(&self.caps, false);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::RefCell;

        /// A thread's sets as the kernel would keep them: `set` refuses to
        /// raise an effective bit that is not permitted, as capset does.
        struct FakeThread {
            sets: RefCell<CapSets>,
            set_calls: RefCell<usize>,
        }

        impl FakeThread {
            fn new(effective: u32, permitted: u32) -> Self {
                let low = CapData {
                    effective,
                    permitted,
                    inheritable: 0,
                };
                Self {
                    sets: RefCell::new([low, CapData::default()]),
                    set_calls: RefCell::new(0),
                }
            }
            fn effective(&self) -> u32 {
                self.sets.borrow()[0].effective
            }
        }

        impl ThreadCaps for &FakeThread {
            fn get(&self) -> io::Result<CapSets> {
                Ok(*self.sets.borrow())
            }
            fn set(&self, sets: &CapSets) -> io::Result<()> {
                *self.set_calls.borrow_mut() += 1;
                if sets[0].effective & !sets[0].permitted != 0 {
                    return Err(io::Error::from_raw_os_error(libc::EPERM));
                }
                *self.sets.borrow_mut() = *sets;
                Ok(())
            }
        }

        const OTHER: u32 = 1 << 12;

        #[test]
        fn without_the_capability_nothing_is_attempted() {
            let thread = FakeThread::new(0, 0);
            drop(PtraceRaised::raise_with(&thread));
            assert!(!set_ptrace_effective(&&thread, false));
            assert_eq!(*thread.set_calls.borrow(), 0);
        }

        #[test]
        fn the_guard_raises_then_lowers_only_the_ptrace_bit() {
            let thread = FakeThread::new(OTHER, PTRACE_BIT | OTHER);
            {
                let _raised = PtraceRaised::raise_with(&thread);
                assert_eq!(thread.effective(), PTRACE_BIT | OTHER);
            }
            assert_eq!(thread.effective(), OTHER);
            assert_eq!(thread.sets.borrow()[0].permitted, PTRACE_BIT | OTHER);
        }

        #[test]
        fn the_guard_lowers_on_an_error_path() {
            let thread = FakeThread::new(0, PTRACE_BIT);
            let failing = || -> Result<(), ()> {
                let _raised = PtraceRaised::raise_with(&thread);
                Err(())
            };
            assert!(failing().is_err());
            assert_eq!(thread.effective(), 0);
        }

        #[test]
        fn the_startup_drop_keeps_the_capability_permitted() {
            let thread = FakeThread::new(PTRACE_BIT, PTRACE_BIT);
            assert!(!set_ptrace_effective(&&thread, false));
            assert_eq!(thread.effective(), 0);
            assert_eq!(thread.sets.borrow()[0].permitted, PTRACE_BIT);
            assert!(set_ptrace_effective(&&thread, true));
        }

        #[test]
        fn the_kernel_answers_version_3_for_this_thread() {
            // Exercises the real struct layout and version against the running
            // kernel. An unprivileged test process normally holds nothing, and
            // both operations must then be silent no-ops.
            let sets = Kernel.get().expect("capget with version 3");
            assert_eq!(
                sets[0].effective & !sets[0].permitted,
                0,
                "effective is a subset of permitted"
            );
            let had = sets[0].effective & PTRACE_BIT != 0;
            drop(PtraceRaised::raise());
            let after = Kernel.get().expect("capget");
            assert_eq!(after[0].permitted, sets[0].permitted);
            if sets[0].permitted & PTRACE_BIT == 0 {
                assert_eq!(after, sets, "a no-op without the capability");
            } else {
                assert_eq!(after[0].effective & PTRACE_BIT, 0, "lowered (was {had})");
            }
        }

        #[test]
        fn a_kernel_write_of_the_unchanged_sets_is_accepted() {
            let sets = Kernel.get().expect("capget");
            Kernel.set(&sets).expect("capset of the current sets");
        }
    }
}
