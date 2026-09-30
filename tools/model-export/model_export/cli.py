"""Command line entry point: `python -m model_export export|fixtures`."""
import argparse
from pathlib import Path

from model_export.embedder import EMBEDDERS, EmbedderExporter
from model_export.exporter import BundleExporter
from model_export.fixtures import FixtureRecorder
from model_export.settings import ReferenceSettings


class ReferenceCli:
    def run(self) -> None:
        parser = argparse.ArgumentParser(prog="model_export")
        parser.add_argument("command", choices=["export", "fixtures", "export-embedder", "embedder-fixtures"])
        parser.add_argument("--embedder", default="granite", choices=sorted(EMBEDDERS))
        parser.add_argument("--checkpoint", default="multilingual")
        parser.add_argument("--source", type=Path, help="local checkpoint directory, e.g. a fine-tuned s1-code model")
        parser.add_argument("--name", help="bundle name under models/, e.g. s1-code-v3-onnx")
        arguments = parser.parse_args()
        settings = ReferenceSettings(checkpoint=arguments.checkpoint, source=arguments.source, bundle_name=arguments.name)
        if arguments.command == "export":
            BundleExporter(settings).export()
        elif arguments.command == "fixtures":
            FixtureRecorder(settings).record()
        elif arguments.command == "export-embedder":
            EmbedderExporter(settings, EMBEDDERS[arguments.embedder]).export()
        else:
            EmbedderExporter(settings, EMBEDDERS[arguments.embedder]).record_fixtures()
