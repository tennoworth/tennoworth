//! Launch host applications without the AppImage's private runtime libraries.

use tauri_plugin_opener::OpenerExt;

pub async fn open(app: tauri::AppHandle, url: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if let Some(appdir) = std::env::var_os("APPDIR").filter(|dir| !dir.is_empty()) {
            return tauri::async_runtime::spawn_blocking(move || linux::open(&url, &appdir))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string());
        }
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::path::Path;
    use std::process::{Command, Stdio};

    fn host_paths(value: &OsStr, appdir: &Path) -> io::Result<OsString> {
        std::env::join_paths(std::env::split_paths(value).filter(|path| {
            !path.starts_with(appdir)
                // An in-app update can leave paths from previous mounts in the environment.
                && !path.components().any(|part| part.as_os_str().as_encoded_bytes().starts_with(b".mount_"))
        }))
        .map_err(io::Error::other)
    }

    fn prepare(
        command: &mut Command,
        appdir: &Path,
        env: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> io::Result<()> {
        for (key, value) in env {
            if matches!(
                key.to_str(),
                Some(
                    "PATH"
                        | "LD_LIBRARY_PATH"
                        | "LD_PRELOAD"
                        | "XDG_DATA_DIRS"
                        | "GTK_PATH"
                        | "QT_PLUGIN_PATH"
                        | "QML2_IMPORT_PATH"
                        | "GIO_MODULE_DIR"
                        | "GIO_EXTRA_MODULES"
                        | "GTK_DATA_PREFIX"
                        | "GTK_EXE_PREFIX"
                        | "GTK_IM_MODULE_FILE"
                        | "GSETTINGS_SCHEMA_DIR"
                        | "GDK_PIXBUF_MODULE_FILE"
                        | "GDK_PIXBUF_MODULEDIR"
                )
            ) {
                let cleaned = host_paths(&value, appdir)?;
                if cleaned.is_empty() {
                    command.env_remove(key);
                } else {
                    command.env(key, cleaned);
                }
            }
        }
        Ok(())
    }

    pub(super) fn open(url: &str, appdir: &OsStr) -> io::Result<()> {
        let mut command = Command::new("xdg-open");
        command.arg(url).stdin(Stdio::null()).stdout(Stdio::null());
        prepare(&mut command, Path::new(appdir), std::env::vars_os())?;
        run_launcher(&mut command)
    }

    fn run_launcher(command: &mut Command) -> io::Result<()> {
        // Waiting on the worker observes launcher failures that detached openers hide.
        let status = command.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "browser launcher failed: {status}"
            )))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reports_launcher_failure_instead_of_only_successful_spawn() {
            assert!(run_launcher(Command::new("/bin/sh").args(["-c", "exit 0"])).is_ok());
            assert!(run_launcher(Command::new("/bin/sh").args(["-c", "exit 4"])).is_err());
        }

        #[test]
        fn strips_current_and_previous_appimage_paths_but_preserves_host_paths() {
            let mut command = Command::new("xdg-open");
            prepare(&mut command, Path::new("/opt/TennoWorth.AppDir"), [
                ("PATH".into(), "/opt/TennoWorth.AppDir/usr/bin:/tmp/.mount_TennoWold/usr/bin:/usr/local/bin:/usr/bin".into()),
                ("LD_LIBRARY_PATH".into(), "/opt/TennoWorth.AppDir/usr/lib:/tmp/.mount_TennoWold/usr/lib:/opt/host/lib".into()),
                ("GIO_EXTRA_MODULES".into(), "/opt/TennoWorth.AppDir/usr/lib/gio/modules".into()),
                ("XDG_DATA_DIRS".into(), "/opt/TennoWorth.AppDir/usr/share:/usr/share".into()),
                ("DBUS_SESSION_BUS_ADDRESS".into(), "unix:path=/run/user/1000/bus".into()),
            ]).unwrap();
            let env: std::collections::HashMap<_, _> = command.get_envs().collect();
            assert_eq!(
                env.get(OsStr::new("PATH")),
                Some(&Some(OsStr::new("/usr/local/bin:/usr/bin")))
            );
            assert_eq!(
                env.get(OsStr::new("LD_LIBRARY_PATH")),
                Some(&Some(OsStr::new("/opt/host/lib")))
            );
            assert_eq!(env.get(OsStr::new("GIO_EXTRA_MODULES")), Some(&None));
            assert_eq!(
                env.get(OsStr::new("XDG_DATA_DIRS")),
                Some(&Some(OsStr::new("/usr/share")))
            );
            assert!(!env.contains_key(OsStr::new("DBUS_SESSION_BUS_ADDRESS")));
        }
    }
}
