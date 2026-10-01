/// Loads the ONNX Runtime library shipped next to the executable (Windows builds link it at runtime).
pub struct RuntimeLibrary;

impl RuntimeLibrary {
    #[cfg(windows)]
    pub fn load() -> anyhow::Result<()> {
        use anyhow::Context;

        let executable = std::env::current_exe().context("locating s1grep.exe")?;
        let library = std::env::var_os("ORT_DYLIB_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| executable.with_file_name(crate::settings::PlatformSettings::WINDOWS_RUNTIME_LIBRARY));
        ort::init_from(&library)
            .with_context(|| format!("could not load {} (it must sit next to s1grep.exe)", library.display()))?
            .commit();
        Ok(())
    }

    #[cfg(not(windows))]
    pub fn load() -> anyhow::Result<()> {
        Ok(())
    }
}
