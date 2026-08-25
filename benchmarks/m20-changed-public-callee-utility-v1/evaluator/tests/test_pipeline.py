import shutil
import unittest
from evaluator import pipeline
from evaluator.artifacts import verify_run
from .support import FIXTURE_ROOT, Transport, fixture_run, launch

class PipelineTest(unittest.TestCase):
    def tearDown(self):
        if FIXTURE_ROOT.exists(): shutil.rmtree(FIXTURE_ROOT)
    def test_end_to_end_seals_two_primary_cells(self):
        selected,root=launch("pipeline"); old=pipeline._FIXTURE_EXECUTION_IDENTITY; pipeline._FIXTURE_EXECUTION_IDENTITY="sha256:"+"d"*64
        try: result=fixture_run(selected,Transport(),root)
        finally: pipeline._FIXTURE_EXECUTION_IDENTITY=old
        self.assertEqual(len(result["primary"]["scores"]),2); self.assertTrue(all(x["completed"] for x in result["primary"]["scores"])); self.assertTrue(verify_run(root)["ok"])
