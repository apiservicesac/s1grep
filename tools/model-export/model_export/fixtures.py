"""Records what the Python runtimes produce so the Rust engine can be checked token by token."""
import json
from dataclasses import dataclass
from typing import Any

from laya.common import build_sequence
from laya.onnx_agent import ONNXAgent

from model_export.exporter import ReferenceAgents
from model_export.settings import ReferenceSettings

LONG_SOURCE = "\n".join(
    f"def handler_{index}(request):\n    value = request.get('field_{index}')\n    return normalize(value) * {index}\n"
    for index in range(160)
)


@dataclass(frozen=True)
class ParityCase:
    name: str
    state: Any
    questions: dict


class CodeSearchCases:
    """The exact shape s1-code is used with: one candidate function as state and a noul relevance question."""

    TEMPLATE = "This code answers the search: {query}"
    CANDIDATES = {
        "retry_send": ("billing/gateway/client.py", "GatewayClient.send_with_retry",
                       "def send_with_retry(self, document, attempts=3):\n    for attempt in range(attempts):\n"
                       "        response = self.post(document)\n        if response.ok:\n            return response\n"
                       "        time.sleep(2 ** attempt)\n    raise GatewayUnavailable(document.number)\n"),
        "sibling_send": ("billing/gateway/client.py", "GatewayClient.send",
                         "def send(self, document):\n    return self.post(document)\n"),
        "invoice_total": ("billing/models/invoice.py", "Invoice.compute_total",
                          "def compute_total(self):\n    subtotal = sum(line.amount for line in self.lines)\n"
                          "    return round(subtotal * (1 + self.tax_rate), 2)\n"),
    }
    QUERIES = {
        "en": "where do we retry sending an invoice to the payment gateway when it fails",
        "es": "dónde se reintenta el envío de la factura a la pasarela de pago cuando falla",
        "mixed": "cómo hago el retry del envío a la pasarela",
    }

    def cases(self) -> list[ParityCase]:
        cases = []
        for candidate, (path, name, source) in self.CANDIDATES.items():
            state = f"{path}\n{name}\n\n{source}"
            questions = {language: {"type": "noul", "instructions": self.TEMPLATE.format(query=query)}
                         for language, query in self.QUERIES.items()}
            cases.append(ParityCase(f"code_search_{candidate}", state, questions))
        return cases


class ParityCaseCatalog:
    """Inputs chosen to exercise every branch of the sequence builder, plus code-search shaped questions."""

    def cases(self) -> list[ParityCase]:
        return CodeSearchCases().cases() + [
            ParityCase(
                "noul_python_auth",
                "def verify_token(token: str) -> bool:\n    claims = jwt.decode(token, SECRET, algorithms=['HS256'])\n"
                "    return claims['exp'] > time.time()\n",
                {
                    "relevant": {"type": "noul", "instructions": "This code validates authentication tokens."},
                    "unrelated": {"type": "noul", "instructions": "This code renders a chart."},
                },
            ),
            ParityCase(
                "noul_spanish_query_typescript",
                "export async function saveInvoice(db: Db, invoice: Invoice) {\n\tawait db.insert('invoices', invoice);\n"
                "\treturn invoice.id;\n}\n",
                {"relevant": {"type": "noul", "instructions": "¿Este código guarda una factura en la base de datos?"}},
            ),
            ParityCase(
                "choice_list_and_described",
                "fn main() {\n    let args: Vec<String> = std::env::args().collect();\n    println!(\"{:?}\", args);\n}\n",
                {
                    "language": {"type": "choice", "instructions": "Which language is this?",
                                 "criteria": ["python", "rust", "go", "typescript"]},
                    "role": {"type": "choice", "instructions": "What does this code do?",
                             "criteria": {"entrypoint": "program entry point", "test": "a unit test",
                                          "library": ""}},
                },
            ),
            ParityCase(
                "score_levels",
                "class Cache:\n    def __init__(self):\n        self.items = {}\n",
                {"usefulness": {"type": "score", "instructions": "How relevant is this to cache eviction?",
                                "criteria": ["not related", "mentions caching", "implements storage",
                                             "implements eviction"]}},
            ),
            ParityCase(
                "noul_custom_labels_and_criteria",
                "SELECT * FROM users WHERE id = %s",
                {"sql": {"type": "noul", "instructions": "This is a SQL query.", "labels": {"false": "no", "true": "yes"},
                         "criteria": {"true": "it is SQL", "false": ""}}},
            ),
            ParityCase(
                "mask_token_in_text",
                "tokenizer.mask_token = '<mask>'  # <mask> appears here",
                {"mask": {"type": "noul", "instructions": "It mentions <mask> tokens."}},
            ),
            ParityCase(
                "long_state_truncated_right",
                LONG_SOURCE,
                {"handlers": {"type": "noul", "instructions": "This file defines request handlers."}},
            ),
            ParityCase(
                "list_state_truncated_left",
                [{"role": "user", "text": f"message number {index} about deployment logs"} for index in range(200)],
                {"deploy": {"type": "noul", "instructions": "The conversation is about deployments."}},
            ),
            ParityCase(
                "dict_state",
                {"path": "src/billing/tax.py", "symbol": "compute_vat", "lines": [10, 42], "note": "año ñandú"},
                {"tax": {"type": "noul", "instructions": "This symbol computes taxes."}},
            ),
            ParityCase(
                "option_budget_overflow",
                "export default function App() { return <Layout /> }",
                {"area": {"type": "choice", "instructions": "Which area does this belong to?",
                          "criteria": {f"area_{index}": "a very long description of a product area " * 6
                                       for index in range(9)}}},
            ),
            ParityCase(
                "unicode_and_whitespace",
                "  // 日本語 コメント 🚀\n\t\tlet café = \"naïve\";\r\n    return café;   \n\n\n",
                {"literal": {"type": "noul", "instructions": "The code defines a string variable."}},
            ),
            ParityCase(
                "empty_state",
                "",
                {"empty": {"type": "noul", "instructions": "There is code here."}},
            ),
        ]


class FixtureRecorder:

    def __init__(self, settings: ReferenceSettings):
        self.settings = settings
        self.agents = ReferenceAgents(settings)
        self.catalog = ParityCaseCatalog()

    def record(self) -> None:
        encoder = self.agents.onnx("model.onnx")
        torch_agent = self.agents.torch()
        records = []
        for case in self.catalog.cases():
            answers = {
                "onnx": encoder.system_one(case.state, case.questions)["answers"],
                "torch": torch_agent.system_one(case.state, case.questions)["answers"],
            }
            records.append({
                "name": case.name,
                "state": case.state,
                "questions": case.questions,
                "sequences": self.sequences(encoder, case),
                "answers": answers,
            })
        document = {
            "max_len": encoder.cfg.get("max_len", 512),
            "head_max_len": encoder.cfg.get("head_max_len", 192),
            "cases": records,
        }
        self.settings.fixtures_path.parent.mkdir(parents=True, exist_ok=True)
        self.settings.fixtures_path.write_text(json.dumps(document, ensure_ascii=False, indent=1) + "\n")
        print(f"{len(records)} cases written to {self.settings.fixtures_path}")

    def sequences(self, agent: ONNXAgent, case: ParityCase) -> dict:
        max_len = agent.cfg.get("max_len", 512)
        head_max_len = agent.cfg.get("head_max_len", 192)
        sequences = {}
        for question_id, definition in case.questions.items():
            internal = agent._to_internal(definition)
            input_ids, markers = build_sequence(agent.tok, case.state, internal, max_len, head_max_len,
                                                truncate_left=isinstance(case.state, list))
            sequences[question_id] = {"input_ids": input_ids, "markers": markers}
        return sequences
