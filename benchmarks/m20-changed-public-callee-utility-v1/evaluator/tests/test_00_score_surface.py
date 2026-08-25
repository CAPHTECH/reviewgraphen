import unittest
from pathlib import Path

from .score_surface_oracle import EXPECTED_SCORE_SURFACE_SHA256, score_surface_sha256


class ScoreSurfaceContractTest(unittest.TestCase):
    def test_every_score_affecting_ast_decision_matches_the_reviewed_contract(self):
        self.assertEqual(score_surface_sha256(Path(__file__).parents[1]), EXPECTED_SCORE_SURFACE_SHA256)
