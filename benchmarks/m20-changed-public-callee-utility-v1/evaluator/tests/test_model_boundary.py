import unittest
from evaluator.canonical import canonical_bytes
from evaluator.model_boundary import TypedError, decode_model
from .attack_probe import _decoder_boundaries, _judge_batch, _judge_response, _reviewer_response

class BoundaryTest(unittest.TestCase):
    def test_judge_bool_is_not_integer(self):
        batch=_judge_batch(); response=_judge_response(batch); response["scores"][0]["dimensions"]["source_specificity"]=True
        with self.assertRaises(TypedError): decode_model(canonical_bytes(response),"judge",{"batch":batch})
    def test_depth_is_total(self):
        with self.assertRaises(TypedError) as exact: decode_model(b"["*32+b"]"*32,"reviewer",{"task_id":"t","source_inventory_id":"i"})
        self.assertNotIn("depth",{item.detail_id for item in exact.exception.errors})
        for depth in (33,2000):
            with self.assertRaises(TypedError) as over: decode_model(b"["*depth+b"]"*depth,"reviewer",{"task_id":"t","source_inventory_id":"i"})
            self.assertIn("depth",{item.detail_id for item in over.exception.errors})
    def test_all_decoder_exact_and_plus_one_boundaries(self):
        self.assertTrue(_decoder_boundaries())
