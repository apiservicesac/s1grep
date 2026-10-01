use std::time::{Duration, Instant};

use clap::Args;
use s1_engine::{Accelerator, EngineOptions, LayaEngine, ModelBundle, Question, QuestionSet};
use serde_json::{Value, json};

use crate::model_locator::ModelLocator;

#[derive(Args)]
pub struct BenchCommand {
    /// Thread counts to compare, e.g. 4,6,8 (default: half the logical CPUs)
    #[arg(long, value_delimiter = ',')]
    threads: Vec<usize>,
    /// Sequence lengths in tokens (question + code)
    #[arg(long, value_delimiter = ',', default_values_t = [128, 256, 512])]
    lengths: Vec<usize>,
    /// Timed runs per measurement; the median is reported
    #[arg(long, default_value_t = 5)]
    repetitions: usize,
    /// Where the model runs: cpu, directml, openvino-cpu or openvino-gpu
    #[arg(long, default_value_t = Accelerator::Cpu)]
    device: Accelerator,
    /// Print the results as JSON
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    model: ModelLocator,
}

struct Measurement {
    threads: usize,
    batch: usize,
    tokens: usize,
    median: Duration,
}

impl BenchCommand {
    const BATCH_SIZE: usize = 8;
    const BATCH_LENGTH: usize = 128;

    pub fn run(self) -> anyhow::Result<()> {
        let bundle = self.model.open()?;
        let thread_counts = if self.threads.is_empty() {
            vec![EngineOptions::default().threads]
        } else {
            self.threads.clone()
        };
        let machine = MachineInfo::detect();
        if !self.json {
            println!("CPU: {} ({} logical CPUs)", machine.cpu, machine.logical_cpus);
            println!("instruction sets: {}", machine.instruction_sets.join(", "));
            println!("model: {}", bundle.directory.display());
            println!("device: {}\n", self.device);
            println!(
                "{:>7} {:>6} {:>7} {:>10} {:>14}",
                "threads", "batch", "tokens", "median", "per fragment"
            );
        }
        let mut measurements = Vec::new();
        for threads in thread_counts {
            let mut engine = self.load(&bundle, threads)?;
            let mut cases: Vec<(usize, usize)> = self.lengths.iter().map(|length| (1, *length)).collect();
            cases.push((Self::BATCH_SIZE, Self::BATCH_LENGTH));
            for (batch, length) in cases {
                let measurement = self.measure(&mut engine, threads, batch, length)?;
                if !self.json {
                    let per_fragment = measurement.median.as_secs_f64() * 1000.0 / batch as f64;
                    println!(
                        "{:>7} {:>6} {:>7} {:>8.0}ms {:>12.0}ms",
                        threads,
                        batch,
                        measurement.tokens,
                        measurement.median.as_secs_f64() * 1000.0,
                        per_fragment
                    );
                }
                measurements.push(measurement);
            }
        }
        if self.json {
            let rows: Vec<Value> = measurements
                .iter()
                .map(|measurement| {
                    json!({
                        "threads": measurement.threads, "batch": measurement.batch, "tokens": measurement.tokens,
                        "median_ms": measurement.median.as_secs_f64() * 1000.0,
                        "per_fragment_ms": measurement.median.as_secs_f64() * 1000.0 / measurement.batch as f64,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"device": self.device.name(), "cpu": machine.cpu, "logical_cpus": machine.logical_cpus,
                                                                 "instruction_sets": machine.instruction_sets, "results": rows})
                )?
            );
        }
        Ok(())
    }

    fn load(&self, bundle: &ModelBundle, threads: usize) -> anyhow::Result<LayaEngine> {
        let options = EngineOptions {
            threads,
            accelerator: self.device,
            ..EngineOptions::default()
        };
        Ok(LayaEngine::load(bundle, &options)?)
    }

    fn measure(
        &self,
        engine: &mut LayaEngine,
        threads: usize,
        batch: usize,
        length: usize,
    ) -> anyhow::Result<Measurement> {
        let mut questions = QuestionSet::default();
        for index in 0..batch {
            questions.push(
                format!("relevant_{index}"),
                Question::noul("This code validates user input before saving it."),
            );
        }
        let state = SampleCode::with_length(engine, &questions, length)?;
        let tokens = engine.encode(&state, &questions)?[0].input_ids.len();
        engine.decide(&state, &questions)?;
        let mut timings = Vec::with_capacity(self.repetitions);
        for _ in 0..self.repetitions.max(1) {
            let started = Instant::now();
            engine.decide(&state, &questions)?;
            timings.push(started.elapsed());
        }
        timings.sort();
        Ok(Measurement {
            threads,
            batch,
            tokens,
            median: timings[timings.len() / 2],
        })
    }
}

/// Realistic source text grown line by line until the encoded sequence reaches a target length.
struct SampleCode;

impl SampleCode {
    const LINES: [&'static str; 8] = [
        "def save_customer(repository, payload):",
        "    if not payload.get(\"email\"):",
        "        raise ValidationError(\"email is required\")",
        "    customer = Customer(name=payload[\"name\"].strip(), email=payload[\"email\"].lower())",
        "    repository.add(customer)",
        "    logger.info(\"customer %s saved\", customer.id)",
        "    return customer",
        "",
    ];

    fn with_length(engine: &LayaEngine, questions: &QuestionSet, target: usize) -> anyhow::Result<Value> {
        let mut text = String::new();
        for line in Self::LINES.iter().cycle() {
            let state = Value::String(text.clone());
            if engine.encode(&state, questions)?[0].input_ids.len() >= target {
                return Ok(state);
            }
            text.push_str(line);
            text.push('\n');
        }
        unreachable!("cycle never ends")
    }
}

struct MachineInfo {
    cpu: String,
    logical_cpus: usize,
    instruction_sets: Vec<&'static str>,
}

impl MachineInfo {
    fn detect() -> Self {
        let logical_cpus = std::thread::available_parallelism().map(usize::from).unwrap_or(1);
        let (cpu, instruction_sets) = Self::processor();
        Self {
            cpu,
            logical_cpus,
            instruction_sets,
        }
    }

    /// Brand string and the vector extensions that matter for ONNX Runtime kernels, read with CPUID.
    #[cfg(target_arch = "x86_64")]
    fn processor() -> (String, Vec<&'static str>) {
        use std::arch::x86_64::{__cpuid, __cpuid_count};

        let brand_bytes: Vec<u8> = (0x8000_0002_u32..=0x8000_0004)
            .flat_map(|leaf| {
                let registers = __cpuid(leaf);
                [registers.eax, registers.ebx, registers.ecx, registers.edx]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
            })
            .collect();
        let brand = String::from_utf8_lossy(&brand_bytes)
            .trim_matches(char::from(0))
            .trim()
            .to_string();
        let extended = __cpuid_count(7, 0);
        let extended_second = __cpuid_count(7, 1);
        let flags = [
            ("AVX2", extended.ebx & (1 << 5) != 0),
            ("AVX-512", extended.ebx & (1 << 16) != 0),
            ("AVX-512 VNNI", extended.ecx & (1 << 11) != 0),
            ("AVX-VNNI", extended_second.eax & (1 << 4) != 0),
            ("AMX-INT8", extended.edx & (1 << 25) != 0),
        ];
        let instruction_sets = flags
            .into_iter()
            .filter(|(_, present)| *present)
            .map(|(name, _)| name)
            .collect();
        (brand, instruction_sets)
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn processor() -> (String, Vec<&'static str>) {
        (std::env::consts::ARCH.to_string(), Vec::new())
    }
}
