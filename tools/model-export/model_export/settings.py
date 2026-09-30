"""Locations shared by every command."""
from dataclasses import dataclass, field
from pathlib import Path


@dataclass(frozen=True)
class ReferenceSettings:
    project: Path = field(default_factory=lambda: Path(__file__).resolve().parents[3])
    repository: str = "convaiinnovations/laya"
    checkpoint: str = "multilingual"
    source: Path | None = None
    bundle_name: str | None = None

    @property
    def name(self) -> str:
        return self.bundle_name or f"laya-{self.checkpoint}"

    @property
    def model_directory(self) -> Path:
        """Self-contained model bundle the Rust engine loads: ONNX graphs, tokenizer and decision config."""
        return self.project / "models" / self.name

    @property
    def fixtures_path(self) -> Path:
        return self.project / "crates" / "s1-engine" / "tests" / "fixtures" / f"{self.name}.json"
