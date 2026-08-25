import unittest
from .vector_runner import run_vectors

class ReferenceVectors(unittest.TestCase):
    def test_all_74_with_original_52_unchanged(self):
        result=run_vectors(); self.assertEqual((result["total"],result["failed"]),(74,0),result)
        self.assertEqual([row["vector_id"] for row in result["results"][:52]],[*[f"T{i:02d}" for i in range(1,21)],*[f"S{i:02d}" for i in range(1,11)],*[f"L{i:02d}" for i in range(1,11)],*[f"J{i:02d}" for i in range(1,13)]])
