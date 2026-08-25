import tempfile
import unittest
from pathlib import Path
from evaluator.freeze import RUNTIME_REQUIREMENTS, identities, runtime_compatible
from evaluator.spec_contract import FREEZE_MANIFEST_KEYS, execution_hash, verify_spec_contract

class FreezeTest(unittest.TestCase):
    def test_runtime_contract_and_portable_bundle(self):
        self.assertTrue(runtime_compatible()); self.assertEqual(RUNTIME_REQUIREMENTS["python_version"],[3,13,5])
        root=Path(__file__).parents[1]; first=identities(root)[1]; first_again=identities(root)[1]; self.assertEqual(first,first_again)
    def test_symlink_rejected_before_directory(self):
        from evaluator.freeze import file_records
        with tempfile.TemporaryDirectory() as value:
            root=Path(value); (root/"real").mkdir(); (root/"link").symlink_to(root/"real",target_is_directory=True)
            with self.assertRaises(ValueError): file_records(root)
    def test_normative_spec_literals_match_implementation(self):
        design=Path(__file__).parents[2]/"EVALUATOR_SPEC.md"
        observed=verify_spec_contract(design)
        self.assertEqual(observed["freeze_manifest_keys"],list(FREEZE_MANIFEST_KEYS))
        self.assertEqual(execution_hash("sha256:"+"1"*64,"sha256:"+"2"*64,"sha256:"+"3"*64),"sha256:705059fcb911daf5a4b9ca9f04335e029d39c5d2bf1178596d10e8998330fa46")
    def test_normative_spec_literal_drift_is_rejected(self):
        design=Path(__file__).parents[2]/"EVALUATOR_SPEC.md"; text=design.read_text()
        mutations=(
            text.replace('M20_EXECUTION_PREIMAGE_KEYS_JSON=["schema","evaluator_bundle_sha256","runtime_requirements_sha256","semantic_acceptance_reference_sha256"]','M20_EXECUTION_PREIMAGE_KEYS_JSON=["schema","evaluator_bundle_sha256","runtime_requirements_sha256"]',1),
            text.replace(',"measurement_record_sha256"',"",1),
        )
        for changed in mutations:
            with tempfile.TemporaryDirectory() as value:
                path=Path(value)/"EVALUATOR_SPEC.md"; path.write_text(changed)
                with self.assertRaises(ValueError): verify_spec_contract(path)
