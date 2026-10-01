"""Exports sentence embedding models (the retrieval stage of s1grep) to ONNX and records reference vectors.

The graph only runs the transformer and returns `last_hidden_state`; pooling, normalisation and the query prompt
are applied by the Rust engine from `embedder_config.json`, so the same code serves granite (CLS pooling) and
Qwen3-Embedding (last-token pooling).
"""
import json
import shutil
from dataclasses import dataclass
from pathlib import Path

import torch
from huggingface_hub import snapshot_download
from transformers import AutoModel

from model_export.settings import ReferenceSettings


@dataclass(frozen=True)
class EmbedderSpec:
    repository: str
    bundle_name: str
    pooling: str
    max_tokens: int
    query_prompt: str = ""
    document_prompt: str = ""


EMBEDDERS = {
    "granite": EmbedderSpec("ibm-granite/granite-embedding-278m-multilingual", "granite-278m-onnx", "cls", 512),
    "granite-97m": EmbedderSpec("ibm-granite/granite-embedding-97m-multilingual-r2", "granite-97m-r2-onnx", "cls", 512),
    "qwen3": EmbedderSpec("Qwen/Qwen3-Embedding-0.6B", "qwen3-embedding-0.6b-onnx", "last", 512,
                          "Instruct: Given a question about what some code does, retrieve the function that implements it\n"
                          "Query: "),
}


class EmbedderSamples:
    """Code units and searches the parity test compares, including a unit longer than the token limit."""

    UNITS = [
        "billing/gateway/client.py\nGatewayClient.send_with_retry\n\ndef send_with_retry(self, document, attempts=3):\n"
        "    for attempt in range(attempts):\n        response = self.post(document)\n        if response.ok:\n"
        "            return response\n        time.sleep(2 ** attempt)\n    raise GatewayUnavailable(document.number)\n",
        "billing/models/invoice.py\nInvoice.compute_total\n\ndef compute_total(self):\n"
        "    return round(sum(line.amount for line in self.lines) * (1 + self.tax_rate), 2)\n",
        "auth/tokens.py\nverify_token\n\ndef verify_token(token: str) -> bool:\n"
        "    claims = jwt.decode(token, SECRET, algorithms=['HS256'])\n    return claims['exp'] > time.time()\n",
        "reports/export.py\nexport_csv\n\ndef export_csv(rows, path):\n    with open(path, 'w', newline='') as handle:\n"
        "        writer = csv.writer(handle)\n        writer.writerows(rows)\n",
        "src/app.ts\nsaveInvoice\n\nexport async function saveInvoice(db: Db, invoice: Invoice) {\n"
        "  await db.insert('invoices', invoice);\n  return invoice.id;\n}\n",
        "utils/texto.py\nnormalizar_telefono\n\ndef normalizar_telefono(valor):\n    \"\"\"Quita espacios y guiones del teléfono.\"\"\"\n"
        "    return ''.join(caracter for caracter in valor if caracter.isdigit())\n",
        "long/module.py\nhandlers\n\n" + "\n".join(
            f"def handler_{index}(request):\n    value = request.get('field_{index}')\n    return normalize(value) * {index}\n"
            for index in range(120)),
        "",
    ]
    QUERIES = [
        "where do we retry sending an invoice to the payment gateway when it fails",
        "dónde se reintenta el envío de la factura a la pasarela de pago cuando falla",
        "validate jwt expiration",
        "cómo exporto las filas a un archivo csv",
        "quitar guiones del teléfono",
    ]


class EmbedderExporter:
    def __init__(self, settings: ReferenceSettings, spec: EmbedderSpec):
        self.settings = settings
        self.spec = spec
        self.bundle = settings.project / "models" / spec.bundle_name

    WEIGHT_FILES = ["*.json", "*.safetensors", "*.txt", "*.model", "1_Pooling/*", "2_Normalize/*"]

    def source(self) -> Path:
        return Path(snapshot_download(self.spec.repository, allow_patterns=self.WEIGHT_FILES))

    def export(self) -> None:
        source = self.source()
        model = AutoModel.from_pretrained(str(source), torch_dtype=torch.float32).eval()
        self.bundle.mkdir(parents=True, exist_ok=True)
        batch = torch.export.Dim("batch_size", min=1, max=4096)
        sequence = torch.export.Dim("seq_len", min=2, max=self.spec.max_tokens)
        sample = (torch.randint(5, 1000, (2, 16), dtype=torch.long), torch.ones((2, 16), dtype=torch.long))

        class LastHidden(torch.nn.Module):
            def __init__(self, inner):
                super().__init__()
                self.inner = inner

            def forward(self, input_ids, attention_mask):
                return self.inner(input_ids=input_ids, attention_mask=attention_mask).last_hidden_state

        with torch.no_grad():
            program = torch.onnx.export(LastHidden(model), sample, input_names=["input_ids", "attention_mask"],
                                        output_names=["last_hidden_state"],
                                        dynamic_shapes={"input_ids": {0: batch, 1: sequence},
                                                        "attention_mask": {0: batch, 1: sequence}},
                                        opset_version=18, dynamo=True, external_data=False)
        program.optimize()
        program.save(str(self.bundle / "model.onnx"))
        shutil.copyfile(source / "tokenizer.json", self.bundle / "tokenizer.json")
        config = {"repository": self.spec.repository, "pooling": self.spec.pooling, "normalize": True,
                  "max_tokens": self.spec.max_tokens, "dimension": model.config.hidden_size,
                  "query_prompt": self.spec.query_prompt, "document_prompt": self.spec.document_prompt,
                  "padding_side": "left" if self.spec.pooling == "last" else "right"}
        (self.bundle / "embedder_config.json").write_text(json.dumps(config, ensure_ascii=False, indent=1) + "\n")
        print(f"embedder bundle written to {self.bundle}")

    def record_fixtures(self) -> None:
        from sentence_transformers import SentenceTransformer

        encoder = SentenceTransformer(str(self.source()), device="cpu", model_kwargs={"torch_dtype": torch.float32})
        encoder.max_seq_length = self.spec.max_tokens
        documents = [self.spec.document_prompt + unit for unit in EmbedderSamples.UNITS]
        queries = [self.spec.query_prompt + query for query in EmbedderSamples.QUERIES]
        vectors = encoder.encode(documents + queries, normalize_embeddings=True, batch_size=4)
        fixture = {"texts": documents + queries,
                   "vectors": [[round(float(value), 6) for value in vector] for vector in vectors]}
        path = self.settings.project / "crates" / "s1-engine" / "tests" / "fixtures" / f"{self.spec.bundle_name}.json"
        path.write_text(json.dumps(fixture, ensure_ascii=False) + "\n")
        print(f"{len(fixture['texts'])} reference vectors written to {path}")
