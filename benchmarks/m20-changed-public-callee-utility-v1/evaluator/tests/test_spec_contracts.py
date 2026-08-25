import unittest
from .contract_runner import execute_case, x_probe
from .spec_contracts import derived_cases, matrix_counts


class SpecificationContractMatrixTest(unittest.TestCase):
    def test_every_derived_case(self):
        for case in derived_cases():
            with self.subTest(kind=case[0],contract=case[1]["id"],value=case[2]): self.assertTrue(execute_case(case))
    def test_x_mutation_oracles_are_table_derived(self):
        for identifier in ("X01","X02","X03","X04","X05","X06","X07","X08"): self.assertTrue(x_probe(identifier),identifier)
    def test_matrix_generates_more_than_ninety_cases(self):
        self.assertGreater(matrix_counts()["derived_cases"],90)
