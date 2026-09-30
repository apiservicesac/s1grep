"""Builds the model bundle: the FP32 ONNX graph plus the files needed to encode questions.

INT8 is not shipped: mmBERT activations have outliers that dynamic quantization destroys, and
weight-only INT8 gave no speedup on AVX2 CPUs (docs/decisions.md).

Replaces scripts/export_onnx.py from github.com/NandhaKishorM/laya (Apache 2.0): its TorchScript
export freezes the decision head at 16 tokens, so any longer input fails in ONNX Runtime.
"""
import shutil
from pathlib import Path

import laya
import torch
from laya.agent import Agent
from laya.onnx_agent import ONNXAgent

from model_export.checkpoint import CheckpointLocator
from model_export.settings import ReferenceSettings


class OnnxGraphExporter:
    INPUT_NAMES = ["input_ids", "attention_mask", "marker_pos", "marker_mask", "qtype"]
    OUTPUT_NAMES = ["logits", "act_logits"]

    def export(self, checkpoint_directory: Path, output_path: Path) -> None:
        """Uses the dynamo exporter: the TorchScript tracer used upstream freezes the head's sequence length."""
        agent = Agent(str(checkpoint_directory), compile=False, device="cpu")
        agent.model.eval()
        batch = torch.export.Dim("batch_size", min=1, max=4096)
        sequence = torch.export.Dim("seq_len", min=4, max=8192)
        markers = torch.export.Dim("num_markers", min=1, max=255)
        sample_inputs = (
            torch.randint(0, 100, (2, 96), dtype=torch.long),
            torch.ones((2, 96), dtype=torch.long),
            torch.tensor([[1, 5, 9], [1, 5, 0]], dtype=torch.long),
            torch.tensor([[True, True, True], [True, True, False]], dtype=torch.bool),
            torch.tensor([0, 2], dtype=torch.long),
        )
        dynamic_shapes = {
            "input_ids": {0: batch, 1: sequence},
            "attention_mask": {0: batch, 1: sequence},
            "marker_pos": {0: batch, 1: markers},
            "marker_mask": {0: batch, 1: markers},
            "qtype": {0: batch},
        }
        with torch.no_grad():
            program = torch.onnx.export(
                agent.model,
                sample_inputs,
                input_names=self.INPUT_NAMES,
                output_names=self.OUTPUT_NAMES,
                dynamic_shapes=dynamic_shapes,
                opset_version=18,
                dynamo=True,
                external_data=False,
            )
        program.optimize()
        program.save(str(output_path))


class BundleExporter:
    TOKENIZER_FILES = ("tokenizer.json", "tokenizer_config.json")

    def __init__(self, settings: ReferenceSettings):
        self.settings = settings
        self.locator = CheckpointLocator(settings)
        self.graph_exporter = OnnxGraphExporter()

    def export(self) -> None:
        checkpoint_directory = self.locator.directory()
        bundle = self.settings.model_directory
        bundle.mkdir(parents=True, exist_ok=True)
        self.graph_exporter.export(checkpoint_directory, bundle / "model.onnx")
        for name in self.TOKENIZER_FILES:
            shutil.copyfile(checkpoint_directory / "tokenizer" / name, bundle / name)
        shutil.copyfile(checkpoint_directory / "rl_agent_config.json", bundle / "decision_config.json")
        print(f"bundle written to {bundle} (laya {laya.__version__})")


class ReferenceAgents:
    """The runtimes whose outputs the Rust engine is compared against."""

    def __init__(self, settings: ReferenceSettings):
        self.settings = settings
        self.checkpoint_directory = str(CheckpointLocator(settings).directory())

    def torch(self) -> Agent:
        return Agent(self.checkpoint_directory, device="cpu")

    def onnx(self, graph: str) -> ONNXAgent:
        return ONNXAgent(self.checkpoint_directory, onnx_path=str(self.settings.model_directory / graph))
