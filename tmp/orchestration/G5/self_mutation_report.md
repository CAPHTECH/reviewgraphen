# G5 self-mutation report

All mutations below were made to production evaluator source, the named test
was run and failed, then the original source was restored.

| # | Production mutation | Detecting test | Result |
|---:|---|---|---|
| 1 | Drop question ID from pair signature | `test_pair_qid_is_part_of_signature` | detected |
| 2 | Truncate derived support IDs to one | `test_loss_support_ids_are_retained_in_derived_identity` | detected |
| 3 | Lower judge total threshold 6 to 5 | `test_total_five_fails_judge_threshold` | detected |
| 4 | Disable sealed permutation reversal | `test_permutation_low_bit_controls_order` | detected |
| 5 | Invert raw-response hash comparison | `test_raw_hash_probe_is_mechanical_failure` | detected |
| 6 | Remove frozen instruction closure | `test_recomputed_packet_forgery_is_failure` | detected |
| 7 | Remove all exact-two primary guards | `test_primary_requires_exact_two_complete_mapping` | detected |
| 8 | Remove duplicate payload closure | `test_duplicate_payload_probe_is_failure` | detected |
| 9 | Allow boolean judge dimensions | `test_boolean_dimensions_and_extra_fields_are_failures` | detected |
| 10 | Ignore symlink directories during freeze walk | `test_symlink_directory_is_rejected` | detected |

Undetected mutations: 0.
