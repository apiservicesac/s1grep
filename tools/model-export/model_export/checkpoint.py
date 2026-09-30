"""Finds the checkpoint to export: a local directory (a fine-tuned s1-code) or one in the Hugging Face cache."""
from pathlib import Path

from huggingface_hub import snapshot_download

from model_export.settings import ReferenceSettings


class CheckpointLocator:
    def __init__(self, settings: ReferenceSettings):
        self.settings = settings

    def directory(self) -> Path:
        if self.settings.source is not None:
            return self.settings.source
        snapshot = snapshot_download(
            self.settings.repository,
            allow_patterns=[f"{self.settings.checkpoint}/*"],
        )
        return Path(snapshot) / self.settings.checkpoint
