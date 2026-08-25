class BudgetExceeded(RuntimeError):
    def __init__(self, dimension, observed, ceiling):
        self.record = {"schema": "m21.typed_failure.v1", "code": "budget_exceeded", "dimension": dimension, "observed": observed, "ceiling": ceiling}
        super().__init__(dimension)


LIMITS = {"input_tokens": 65536, "output_tokens": 24000, "wall_seconds": 1800, "tool_calls": 32, "tool_response_bytes": 16384}


def enforce(dimension, observed):
    ceiling = LIMITS[dimension]
    if observed > ceiling: raise BudgetExceeded(dimension, observed, ceiling)
