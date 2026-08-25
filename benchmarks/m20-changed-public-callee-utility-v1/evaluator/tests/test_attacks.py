import unittest
from .attack_probe import probe

class AttackTest(unittest.TestCase):
    def test_boundary_oracles(self):
        for identifier in ("P06","P07","N02","N05","N06","N14","N15","N16","N18","N19","N20","N21","N22","N23","N25","H3R01","H3R02","H3D01","H3D02","H3N01","OCC01","OCC02","CTX01","CTX02","CTX03","CTX04","CTX05","CTX06","CTX07","SC01","SC02"):
            self.assertTrue(probe(identifier),identifier)
