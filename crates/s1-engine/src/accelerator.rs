use std::fmt;
use std::str::FromStr;

use ort::session::builder::SessionBuilder;

use crate::error::EngineError;

/// Where ONNX Runtime runs the model. Anything other than `Cpu` must load or the session fails:
/// a silent fallback to the CPU would make every measurement meaningless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Accelerator {
    #[default]
    Cpu,
    /// Any DirectX 12 GPU on Windows, integrated graphics included.
    DirectMl,
    /// Intel's OpenVINO runtime on the CPU.
    OpenVinoCpu,
    /// Intel's OpenVINO runtime on the integrated or discrete Intel GPU.
    OpenVinoGpu,
}

impl Accelerator {
    pub const NAMES: [&'static str; 4] = ["cpu", "directml", "openvino-cpu", "openvino-gpu"];

    pub fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::DirectMl => "directml",
            Self::OpenVinoCpu => "openvino-cpu",
            Self::OpenVinoGpu => "openvino-gpu",
        }
    }

    pub fn configure(self, builder: SessionBuilder) -> Result<SessionBuilder, EngineError> {
        match self {
            Self::Cpu => Ok(builder),
            #[cfg(windows)]
            Self::DirectMl => {
                let provider = ort::ep::DirectML::default().build().error_on_failure();
                Ok(builder.with_execution_providers([provider]).map_err(ort::Error::from)?)
            }
            #[cfg(windows)]
            Self::OpenVinoCpu | Self::OpenVinoGpu => {
                let device = if self == Self::OpenVinoGpu { "GPU" } else { "CPU" };
                let provider = ort::ep::OpenVINO::default()
                    .with_device_type(device)
                    .build()
                    .error_on_failure();
                Ok(builder.with_execution_providers([provider]).map_err(ort::Error::from)?)
            }
            #[cfg(not(windows))]
            other => Err(EngineError::UnsupportedAccelerator(other.name())),
        }
    }
}

impl fmt::Display for Accelerator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl FromStr for Accelerator {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "cpu" => Ok(Self::Cpu),
            "directml" => Ok(Self::DirectMl),
            "openvino-cpu" => Ok(Self::OpenVinoCpu),
            "openvino-gpu" => Ok(Self::OpenVinoGpu),
            _ => Err(format!(
                "unknown device {name:?}; use one of {}",
                Self::NAMES.join(", ")
            )),
        }
    }
}
