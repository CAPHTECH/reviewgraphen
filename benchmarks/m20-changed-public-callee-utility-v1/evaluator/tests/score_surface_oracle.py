"""Alpha-normalized AST contract for code that can change an m20 score."""
import ast
from pathlib import Path

from evaluator.canonical import hash_json


SCORING_MODULES = (
    "canonical.py",
    "source_payload.py",
    "textnorm.py",
    "repository.py",
    "model_boundary.py",
    "stage0_contract.py",
    "pipeline.py",
)

# These values can change process scheduling or the public error channel, but
# cannot change eligibility, packets, losses, opportunity, arm/judge decisions,
# or a primary score cell.
NON_SCORE_VALUE_CONTEXTS = {
    "subprocess_timeout",
    "pipeline_error_exit_code",
    "dataclass_configuration",
}


class _Normalize(ast.NodeTransformer):
    def __init__(self, loop_test: bool = False):
        self.loop_test = loop_test

    def visit_Name(self, node):
        return ast.copy_location(ast.Name(id="_", ctx=node.ctx), node)

    def visit_arg(self, node):
        return ast.copy_location(ast.arg(arg="_", annotation=None), node)

    def visit_Compare(self, node):
        node = self.generic_visit(node)
        if (
            len(node.ops) == 1
            and isinstance(node.ops[0], ast.IsNot)
            and len(node.comparators) == 1
            and isinstance(node.comparators[0], ast.Constant)
            and node.comparators[0].value is None
            and isinstance(node.left, ast.Call)
            and isinstance(node.left.func, ast.Attribute)
            and node.left.func.attr == "isascii"
        ):
            return ast.copy_location(ast.Constant(True), node)
        if (
            self.loop_test
            and len(node.ops) == 1
            and isinstance(node.ops[0], (ast.Lt, ast.NotEq))
            and len(node.comparators) == 1
            and isinstance(node.left, ast.Name)
            and isinstance(node.comparators[0], ast.Call)
            and isinstance(node.comparators[0].func, ast.Name)
        ):
            node.ops[0] = ast.Lt()
        return node

    def visit_BoolOp(self, node):
        node = self.generic_visit(node)
        if isinstance(node.op, ast.And):
            node.values = [value for value in node.values if not (isinstance(value, ast.Constant) and value.value is True)]
        elif isinstance(node.op, ast.Or):
            node.values = [value for value in node.values if not (isinstance(value, ast.Constant) and value.value is False)]
        if len(node.values) == 1:
            return node.values[0]
        return node

    def _sequence(self, node):
        node = self.generic_visit(node)
        values = [item.value for item in node.elts if isinstance(item, ast.Constant)]
        if values and all(isinstance(value, int) and not isinstance(value, bool) for value in values if value != "__m20_mutant_enum_extension__"):
            node.elts = [item for item in node.elts if not (isinstance(item, ast.Constant) and item.value == "__m20_mutant_enum_extension__")]
        return node

    visit_Set = _sequence
    visit_List = _sequence
    visit_Tuple = _sequence


def _dump(node: ast.AST, loop_test: bool = False) -> str:
    normalized = _Normalize(loop_test).visit(ast.fix_missing_locations(ast.parse(ast.unparse(node), mode="eval").body))
    return ast.dump(normalized, annotate_fields=True, include_attributes=False)


def _call_name(node: ast.AST) -> str | None:
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        return node.attr
    return None


def _non_score_integer(node: ast.Constant, parents: list[ast.AST]) -> str | None:
    parent = parents[-1] if parents else None
    if isinstance(parent, ast.keyword) and parent.arg == "timeout":
        return "subprocess_timeout"
    for ancestor in reversed(parents):
        if not isinstance(ancestor, ast.Call):
            continue
        name = _call_name(ancestor.func)
        if name == "PipelineError" and node in ancestor.args[1:]:
            return "pipeline_error_exit_code"
        break
    return None


def score_atoms_bytes(raw: bytes, filename: str) -> list[tuple]:
    tree = ast.parse(raw, filename=filename)
    atoms: list[tuple] = []

    class Visitor(ast.NodeVisitor):
        def __init__(self):
            self.parents: list[ast.AST] = []

        def visit(self, node):
            if isinstance(node, ast.If) and isinstance(node.test, ast.Constant) and node.test.value is False:
                return
            self.parents.append(node)
            try:
                return super().visit(node)
            finally:
                self.parents.pop()

        def visit_FunctionDef(self, node):
            for default in (*node.args.defaults, *node.args.kw_defaults):
                if default is not None:
                    self.visit(default)
            for statement in node.body:
                self.visit(statement)

        visit_AsyncFunctionDef = visit_FunctionDef

        def visit_ClassDef(self, node):
            for statement in node.body:
                self.visit(statement)

        def visit_Compare(self, node):
            normalized = _dump(node, loop_test=any(isinstance(parent, ast.While) for parent in self.parents[:-1]))
            if normalized != "Constant(value=True)":
                atoms.append(("compare", normalized))
            self.generic_visit(node)

        def visit_BoolOp(self, node):
            atoms.append(("boolop", _dump(node)))
            self.generic_visit(node)

        def visit_If(self, node):
            atoms.append(("condition", "if", _dump(node.test)))
            self.generic_visit(node)

        def visit_IfExp(self, node):
            atoms.append(("condition", "ifexp", _dump(node.test)))
            self.generic_visit(node)

        def visit_While(self, node):
            atoms.append(("condition", "while", _dump(node.test, loop_test=True)))
            self.generic_visit(node)

        def visit_Raise(self, node):
            atoms.append(("raise",))
            self.generic_visit(node)

        def visit_Constant(self, node):
            if isinstance(node.value, bool):
                atoms.append(("boolean", node.value))
            elif isinstance(node.value, int):
                context = _non_score_integer(node, self.parents[:-1])
                if context is None:
                    atoms.append(("integer", node.value))

    Visitor().visit(tree)
    return sorted(atoms, key=repr)


def score_atoms(path: Path) -> list[tuple]:
    return score_atoms_bytes(path.read_bytes(), path.name)


def score_surface(root: Path) -> dict:
    modules = {name: [list(atom) for atom in score_atoms(root / name)] for name in SCORING_MODULES}
    return {
        "schema": "m20.score-surface-ast.v1",
        "normalizations": [
            "alpha_rename",
            "independent_statement_order",
            "unreachable_if_false",
            "monotone_index_lt_ne_len",
            "bool_return_is_not_none",
            "wrong_typed_integer_membership_extension",
        ],
        "non_score_value_contexts": sorted(NON_SCORE_VALUE_CONTEXTS),
        "modules": modules,
    }


def score_surface_sha256(root: Path) -> str:
    return hash_json(score_surface(root))


EXPECTED_SCORE_SURFACE_SHA256 = "sha256:bc1b8493c84c13b80e5eb2139e54837fbe928fd78232f0c96405e03ec35d6933"
