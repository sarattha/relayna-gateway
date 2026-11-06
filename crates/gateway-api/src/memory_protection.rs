//! Applied before configuration loads credentials or runtime threads start.
#[cfg(target_os = "linux")]
pub fn disable_process_dumps() -> std::io::Result<()> {
    // SAFETY: prctl takes scalar arguments; set/getrlimit receive valid pointers
    // to initialized rlimit structures. These calls affect only this process.
    unsafe {
        if libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::setrlimit(libc::RLIMIT_CORE, &limit) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut actual = limit;
        if libc::getrlimit(libc::RLIMIT_CORE, &mut actual) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) != 0
            || actual.rlim_cur != 0
            || actual.rlim_max != 0
        {
            return Err(std::io::Error::other(
                "process dump protection did not take effect",
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn disable_process_dumps() -> std::io::Result<()> {
    // Development platforms require their own OS crash-report/debugger policy.
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn dump_protection_is_verified_in_an_isolated_process() {
        const CHILD: &str = "RELAYNA_TEST_DUMP_PROTECTION_CHILD";
        if std::env::var_os(CHILD).is_some() {
            super::disable_process_dumps().unwrap();
            // Repeat verifies idempotence with an already-zero hard limit.
            super::disable_process_dumps().unwrap();
            return;
        }
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("dump_protection_is_verified_in_an_isolated_process")
            .env(CHILD, "1")
            .status()
            .unwrap();
        assert!(status.success());
    }
}
